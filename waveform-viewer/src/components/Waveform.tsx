import { MaterialIcon } from "./MaterialIcon";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import type { Row } from "../state";
import type { Signal, Trace, Viewport } from "../trace";
import { clampView, tickStep, tickLabel, valueAt, zoomAt } from "../trace";
import { detectDevice, scrollMotion } from "../scroll";
import type { ScrollMode } from "../scroll";
import { ResizeHandle } from "./ResizeHandle";
import { CommandEvents } from "./CommandEvents";
import { Welcome } from "./Welcome";
import type { EmptyState } from "../empty";
import { busPreview, formatBus } from "../bus";
import type { Bus } from "../bus";
import { signalKey } from "../trace";
import { drawBus } from "./drawBus";

interface Props {
  zeroTick: string;
  trace: Trace;
  rows: Row[];
  buses: Map<string, Bus>;
  signals: Map<string, Signal>;
  clock: string | null;
  selected: string[];
  viewport: Viewport;
  marker: number;
  altMarker: number | null;
  setViewport: (view: Viewport) => void;
  setMarker: (tick: number, alternate: boolean) => void;
  select: (id: string, multiple: boolean, range: boolean) => void;
  reorder: (from: string, to: string) => void;
  context: (event: React.MouseEvent, id?: string) => void;
  toggleGroup: (id: string) => void;
  snap: boolean;
  scrollMode: ScrollMode;
  labelWidth: number;
  setLabelWidth: (width: number) => void;
  eventsExpanded: boolean;
  setEventsExpanded: (expanded: boolean) => void;
  emptyState: EmptyState | null;
}

export function Waveform(props: Props) {
  const { trace, rows, selected, viewport, marker, altMarker } = props;
  const host = useRef<HTMLDivElement>(null),
    scroller = useRef<HTMLDivElement>(null);
  const axis = useRef<HTMLCanvasElement>(null),
    wave = useRef<HTMLCanvasElement>(null);
  const [width, setWidth] = useState(800);
  const tooltip = useRef<HTMLDivElement>(null);
  const [hover, setHover] = useState<{
    row: Row;
    tick: number;
    left: number;
    top: number;
  } | null>(null);
  const showBusPreview = (row: Row, tick: number, x: number, y: number) =>
    setHover({
      row,
      tick,
      left: Math.max(8, Math.min(x + 12, window.innerWidth - 528)),
      top: Math.max(8, Math.min(y + 16, window.innerHeight - 300)),
    });
  const { labelWidth, setLabelWidth } = props;
  const latest = useRef(props);
  latest.current = props;
  const drag = useRef<{
    x: number;
    start: Viewport;
    moved: boolean;
    button: number;
  } | null>(null);
  const positions = rows.map((row, index) => ({
    row,
    y: rows.slice(0, index).reduce((n, item) => n + item.height, 0),
  }));
  const height = Math.max(
    rows.reduce((n, row) => n + row.height, 0),
    100,
  );
  const range = viewport.end - viewport.start;
  const hoverRow = rows.find((row) => row.id === hover?.row.id);
  useLayoutEffect(() => {
    const rect = tooltip.current?.getBoundingClientRect();
    if (rect && rect.bottom > window.innerHeight - 8)
      setHover(
        (hover) =>
          hover && {
            ...hover,
            top: Math.max(
              8,
              hover.top - (rect.bottom - window.innerHeight + 8),
            ),
          },
      );
  }, [hover, props.buses]);
  const xFor = (tick: number) => ((tick - viewport.start) / range) * width;

  useLayoutEffect(() => {
    const observer = new ResizeObserver((entries) =>
      setWidth(Math.max(100, entries[0].contentRect.width - labelWidth - 14)),
    );
    observer.observe(host.current!);
    return () => observer.disconnect();
  }, [labelWidth]);

  useEffect(() => {
    const element = host.current!;
    let detected: "mouse" | "touchpad" = "mouse",
      detectedAt = 0;
    let frame = 0;
    const queueView = (view: Viewport) => {
      latest.current = { ...latest.current, viewport: view };
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() =>
        latest.current.setViewport(latest.current.viewport),
      );
    };
    const wheel = (event: WheelEvent) => {
      if ((event.target as HTMLElement).closest(".event-output")) return;
      event.preventDefault();
      const p = latest.current;
      const onLabels =
        event.clientX - element.getBoundingClientRect().left < labelWidth;
      if (p.scrollMode === "auto" && performance.now() - detectedAt > 100) {
        detected = detectDevice(event as WheelEvent & { wheelDeltaY?: number });
        detectedAt = performance.now();
      }
      const motion = scrollMotion(
        event,
        p.scrollMode,
        detected,
        onLabels,
        scroller.current!.clientHeight,
      );
      const bounds = { start: trace.start, end: trace.end };
      const pixels = Math.max(100, element.clientWidth - labelWidth - 14);
      if (motion.kind === "rows") {
        scroller.current!.scrollTop += motion.y;
        return;
      }
      if (motion.kind === "zoom") {
        const anchor =
          (event.clientX - element.getBoundingClientRect().left - labelWidth) /
          pixels;
        queueView(zoomAt(p.viewport, motion.factor, anchor, bounds));
      } else {
        scroller.current!.scrollTop += motion.y;
        const shift = (motion.x / pixels) * (p.viewport.end - p.viewport.start);
        queueView(
          clampView(
            { start: p.viewport.start + shift, end: p.viewport.end + shift },
            bounds,
          ),
        );
      }
    };
    element.addEventListener("wheel", wheel, { passive: false });
    return () => {
      element.removeEventListener("wheel", wheel);
      cancelAnimationFrame(frame);
    };
  }, [trace.start, trace.end, labelWidth]);

  const draw = useCallback(() => {
    const prepare = (canvas: HTMLCanvasElement, h: number) => {
      const dpr = window.devicePixelRatio || 1;
      canvas.width = Math.round(width * dpr);
      canvas.height = Math.round(h * dpr);
      const context = canvas.getContext("2d")!;
      context.scale(dpr, dpr);
      return context;
    };
    const a = prepare(axis.current!, 44),
      c = prepare(wave.current!, height);
    a.fillStyle = "#1e1e1e";
    a.fillRect(0, 0, width, 44);
    c.fillStyle = "#1e1e1e";
    c.fillRect(0, 0, width, height);
    const step = tickStep(range, width);
    const offset = Number(
      (BigInt(props.zeroTick) - BigInt(trace.tickOrigin ?? "0")) % BigInt(step),
    );
    const first = Math.ceil((viewport.start - offset) / step) * step + offset;
    a.font = "12px Consolas, monospace";
    a.textAlign = "center";
    for (let tick = first; tick <= viewport.end; tick += step) {
      const x = xFor(tick);
      a.fillStyle = "#858585";
      a.fillText(tickLabel(trace, tick, props.zeroTick), x, 18);
      a.strokeStyle = "#777777";
      a.beginPath();
      a.moveTo(x, 28);
      a.lineTo(x, 40);
      a.stroke();
      c.strokeStyle = "#393939";
      c.beginPath();
      c.moveTo(x + 0.5, 0);
      c.lineTo(x + 0.5, height);
      c.stroke();
      for (let j = 1; j < 10; j++) {
        const minor = xFor(tick + (step * j) / 10);
        a.strokeStyle = "#555555";
        a.beginPath();
        a.moveTo(minor, 35);
        a.lineTo(minor, 39);
        a.stroke();
      }
    }
    for (const { row, y } of positions) {
      if (selected.includes(row.id)) {
        c.fillStyle = "#50272a";
        c.fillRect(0, y, width, row.height);
      }
      if (row.kind === "group") {
        c.fillStyle = selected.includes(row.id) ? "#50272a" : "#252526";
        c.fillRect(0, y, width, row.height);
        const bus = props.buses.get(row.id);
        if (bus && row.bus)
          drawBus(c, bus, row.bus, viewport, width, y, row.height, row.color);
        continue;
      }
      if (row.kind === "separator") {
        c.strokeStyle = "#555";
        c.beginPath();
        c.moveTo(0, y + row.height / 2);
        c.lineTo(width, y + row.height / 2);
        c.stroke();
        continue;
      }
      const signal = props.signals.get(row.id)!;
      const high = y + 8,
        low = y + row.height - 8;
      const yFor = (value: boolean | null) =>
        value === null ? (high + low) / 2 : value ? high : low;
      const shine = c.createLinearGradient(0, high, 0, low);
      shine.addColorStop(0, `${row.color}40`);
      shine.addColorStop(1, `${row.color}00`);
      c.fillStyle = shine;
      let value = valueAt(signal, viewport.start),
        previousX = 0;
      c.strokeStyle = row.color;
      c.lineWidth = 1.1;
      c.beginPath();
      c.moveTo(0, yFor(value));
      const changes = signal.changes.filter(
        (change) => change.tick > viewport.start && change.tick <= viewport.end,
      );
      for (const change of changes) {
        const x = xFor(change.tick);
        if (value === true)
          c.fillRect(previousX, high, x - previousX, low - high);
        if (value === null) {
          c.stroke();
          c.setLineDash([3, 3]);
          c.beginPath();
          c.moveTo(previousX, yFor(value));
        }
        c.lineTo(x, yFor(value));
        c.lineTo(x, yFor(change.value));
        value = change.value;
        previousX = x;
      }
      if (value === true)
        c.fillRect(previousX, high, width - previousX, low - high);
      c.lineTo(width, yFor(value));
      c.stroke();
      c.setLineDash([]);
      const counts = new Map<number, number>();
      changes.forEach((change) =>
        counts.set(change.tick, (counts.get(change.tick) ?? 0) + 1),
      );
      counts.forEach((count, tick) => {
        if (count < 2) return;
        const x = xFor(tick);
        c.fillStyle = row.color;
        c.beginPath();
        c.moveTo(x, y + 2);
        c.lineTo(x + 3, y + 5);
        c.lineTo(x, y + 8);
        c.lineTo(x - 3, y + 5);
        c.closePath();
        c.fill();
      });
      if (row.guides !== "none")
        for (const change of changes) {
          if (
            (row.guides === "rising" && !change.value) ||
            (row.guides === "falling" && change.value)
          )
            continue;
          c.save();
          c.globalAlpha = 0.25;
          c.strokeStyle = row.color;
          c.setLineDash([2, 4]);
          c.beginPath();
          c.moveTo(xFor(change.tick), 0);
          c.lineTo(xFor(change.tick), height);
          c.stroke();
          c.restore();
        }
    }
    for (const [tick, color, label] of [
      [marker, "#bdbdbd", "A"],
      [altMarker, "#d7ba7d", "B"],
    ] as const) {
      if (tick === null || tick < viewport.start || tick > viewport.end)
        continue;
      const x = xFor(tick);
      c.strokeStyle = color;
      c.setLineDash([5, 4]);
      c.beginPath();
      c.moveTo(x, 0);
      c.lineTo(x, height);
      c.stroke();
      c.setLineDash([]);
      a.fillStyle = color;
      a.fillRect(x - 19, 23, 38, 19);
      a.fillStyle = "#1e1e1e";
      a.fillText(`${label}`, x, 37);
    }
  }, [
    width,
    height,
    rows,
    selected,
    viewport,
    marker,
    altMarker,
    trace,
    props.buses,
    props.signals,
    props.zeroTick,
  ]);
  useEffect(() => {
    const frame = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(frame);
  }, [draw]);

  const tickAtPointer = (event: React.PointerEvent) => {
    const rect = wave.current!.getBoundingClientRect();
    let tick = Math.round(
      viewport.start + ((event.clientX - rect.left) / width) * range,
    );
    if (props.snap) {
      const y = event.clientY - rect.top;
      const row = positions.find(
        (item) => y >= item.y && y < item.y + item.row.height,
      )?.row;
      const signal = row && props.signals.get(row.id);
      const changes =
        (row && props.buses.get(row.id)?.changes) ?? signal?.changes;
      const nearest = changes?.reduce(
        (best, change) =>
          Math.abs(change.tick - tick) < Math.abs(best - tick)
            ? change.tick
            : best,
        Infinity,
      );
      if (
        nearest !== undefined &&
        (Math.abs(nearest - tick) / range) * width < 10
      )
        tick = nearest;
    }
    return Math.max(trace.start, Math.min(trace.end, tick));
  };
  return (
    <div
      className="waveform-pane"
      ref={host}
      data-view-start={viewport.start}
      data-view-end={viewport.end}
    >
      <div className="wave-axis">
        <div className="label-axis" style={{ width: labelWidth }}>
          <span>Name</span>
          <span>Value</span>
        </div>
        <canvas
          ref={axis}
          style={{ width, height: 44 }}
          aria-label="Time ruler in game ticks"
          onPointerDown={(event) => {
            event.currentTarget.setPointerCapture(event.pointerId);
            props.setMarker(
              tickAtPointer(event),
              event.altKey || event.button === 1,
            );
          }}
          onPointerMove={(event) => {
            if (event.buttons)
              props.setMarker(
                tickAtPointer(event),
                event.altKey || event.buttons === 4,
              );
          }}
        />
        <span className="tick-unit" style={{ left: labelWidth + 9 }}>
          ticks
        </span>
      </div>
      <ResizeHandle
        label="Resize signal labels"
        value={labelWidth}
        min={120}
        max={Math.min(600, labelWidth + width - 160)}
        onChange={setLabelWidth}
        className="label-resizer"
        style={{ left: labelWidth - 2 }}
      />
      <div className="wave-scroll" ref={scroller}>
        <div
          className="wave-content"
          style={{
            height: Math.max(height, 700),
            visibility: props.emptyState ? "hidden" : undefined,
          }}
        >
          <div className="row-labels" style={{ width: labelWidth }}>
            {positions.map(({ row, y }, index) => {
              const signal = props.signals.get(row.id);
              const bus = props.buses.get(row.id);
              const parent = row.groupId
                ? props.buses.get(row.groupId)
                : undefined;
              const bit =
                signal && parent
                  ? parent.members.findIndex(
                      (member) =>
                        member && signalKey(member) === signalKey(signal),
                    )
                  : -1;
              return (
                <div
                  key={row.id}
                  className={`wave-label ${row.kind} ${bus ? "bus" : ""} ${row.groupId ? "group-child" : ""} ${row.groupId && rows[index + 1]?.groupId !== row.groupId ? "group-end" : ""} ${selected.includes(row.id) ? "selected" : ""}`}
                  style={{ top: y, height: row.height, color: row.color }}
                  title={
                    bus
                      ? undefined
                      : signal
                        ? `${signal.scope}.${signal.name}\n${signal.position.join(", ")}`
                        : row.label
                  }
                  draggable
                  tabIndex={bus ? 0 : undefined}
                  onPointerMove={(event) => {
                    if (bus)
                      showBusPreview(row, marker, event.clientX, event.clientY);
                  }}
                  onPointerLeave={() => setHover(null)}
                  onFocus={(event) => {
                    if (bus) {
                      const rect = event.currentTarget.getBoundingClientRect();
                      showBusPreview(row, marker, rect.left, rect.bottom);
                    }
                  }}
                  onBlur={() => setHover(null)}
                  onDragStart={(event) =>
                    event.dataTransfer.setData("text/plain", row.id)
                  }
                  onDragOver={(event) => event.preventDefault()}
                  onDrop={(event) => {
                    event.preventDefault();
                    props.reorder(
                      event.dataTransfer.getData("text/plain"),
                      row.id,
                    );
                  }}
                  onClick={(event) =>
                    props.select(
                      row.id,
                      event.ctrlKey || event.metaKey,
                      event.shiftKey,
                    )
                  }
                  onContextMenu={(event) => props.context(event, row.id)}
                >
                  {row.kind === "group" && (
                    <button
                      aria-label={`Collapse ${row.label}`}
                      onClick={(event) => {
                        event.stopPropagation();
                        props.toggleGroup(row.id);
                      }}
                    >
                      <MaterialIcon
                        name={row.collapsed ? "chevron" : "expand"}
                        size={16}
                      />
                    </button>
                  )}
                  <span className="row-name">
                    {row.kind === "signal" && !row.shortName
                      ? `${signal?.scope}.`
                      : ""}
                    {row.label}
                    {signal && signalKey(signal) === props.clock
                      ? " [clock]"
                      : row.clockEdge
                        ? ` [${row.clockEdge} clock]`
                        : ""}
                    {bus
                      ? ` [${bus.members.length - 1}:0]`
                      : bit >= 0
                        ? ` [${bit}]`
                        : ""}
                  </span>
                  <span className="row-value">
                    <span>
                      {bus && row.bus
                        ? formatBus(valueAt(bus, marker), row.bus)
                        : signal
                          ? valueAt(signal, marker) === null
                            ? "X"
                            : valueAt(signal, marker)
                              ? "1"
                              : "0"
                          : ""}
                    </span>
                  </span>
                </div>
              );
            })}
          </div>
          <canvas
            ref={wave}
            style={{ left: labelWidth, width, height }}
            role="img"
            aria-label="Binary logic waveforms. Click for cursor A; Alt-click for cursor B. Drag to pan time."
            onContextMenu={(event) => {
              const y =
                event.clientY - wave.current!.getBoundingClientRect().top;
              props.context(
                event,
                positions.find(
                  (item) => y >= item.y && y < item.y + item.row.height,
                )?.row.id,
              );
            }}
            onPointerDown={(event) => {
              if (event.button === 2) return;
              event.currentTarget.setPointerCapture(event.pointerId);
              drag.current = {
                x: event.clientX,
                start: viewport,
                moved: false,
                button: event.button,
              };
            }}
            onPointerMove={(event) => {
              if (!drag.current) {
                const rect = wave.current!.getBoundingClientRect();
                const y = event.clientY - rect.top;
                const row = positions.find(
                  (item) => y >= item.y && y < item.y + item.row.height,
                )?.row;
                if (row?.bus) {
                  const bus = props.buses.get(row.id)!;
                  const tick =
                    viewport.start +
                    ((event.clientX - rect.left) / width) * range;
                  const nearby = bus.changes.find(
                    (change) =>
                      Math.abs(
                        xFor(change.tick) - (event.clientX - rect.left),
                      ) < 5,
                  );
                  showBusPreview(
                    row,
                    nearby?.tick ?? Math.floor(tick),
                    event.clientX,
                    event.clientY,
                  );
                } else setHover(null);
              } else setHover(null);
              if (!drag.current || drag.current.button === 1) return;
              const distance = event.clientX - drag.current.x;
              if (Math.abs(distance) > 3) drag.current.moved = true;
              if (drag.current.moved) {
                const shift =
                  (-distance / width) *
                  (drag.current.start.end - drag.current.start.start);
                props.setViewport(
                  clampView(
                    {
                      start: drag.current.start.start + shift,
                      end: drag.current.start.end + shift,
                    },
                    trace,
                  ),
                );
              }
            }}
            onPointerLeave={() => setHover(null)}
            onPointerUp={(event) => {
              if (drag.current && !drag.current.moved) {
                props.setMarker(
                  tickAtPointer(event),
                  event.altKey || drag.current.button === 1,
                );
                const y =
                  event.clientY - wave.current!.getBoundingClientRect().top;
                const row = positions.find(
                  (item) => y >= item.y && y < item.y + item.row.height,
                )?.row;
                if (row)
                  props.select(
                    row.id,
                    event.ctrlKey || event.metaKey,
                    event.shiftKey,
                  );
              }
              drag.current = null;
            }}
            onPointerCancel={() => {
              drag.current = null;
            }}
          />
          <div
            className="empty-wave-space"
            style={{ top: height, left: labelWidth }}
            onContextMenu={(event) => props.context(event)}
          />
        </div>
        {props.emptyState && <Welcome state={props.emptyState} />}
      </div>
      {hover && hoverRow?.bus && props.buses.has(hoverRow.id) && (
        <div
          role="tooltip"
          ref={tooltip}
          className="bus-tooltip"
          style={{ left: hover.left, top: hover.top }}
        >
          <strong>
            {hoverRow.label} · Tick{" "}
            {tickLabel(trace, hover.tick, props.zeroTick)}
          </strong>
          <div className="bus-preview-value">
            {busPreview(
              props.buses.get(hoverRow.id)!,
              hoverRow.bus,
              hover.tick,
            )}
          </div>
          <small>
            {props.buses
              .get(hoverRow.id)!
              .members.map(
                (signal, bit) => `${bit}: ${signal?.name ?? "missing (X)"}`,
              )
              .join(" · ")}
          </small>
        </div>
      )}
      <CommandEvents
        zeroTick={props.zeroTick}
        trace={trace}
        viewport={viewport}
        labelWidth={labelWidth}
        width={width}
        setMarker={props.setMarker}
        expanded={props.eventsExpanded}
        setExpanded={props.setEventsExpanded}
      />
    </div>
  );
}
