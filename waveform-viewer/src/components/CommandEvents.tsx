import { MaterialIcon } from "./MaterialIcon";
import { Fragment, useState } from "react";
import type { Trace, Viewport } from "../trace";
import { tickLabel } from "../trace";

interface Props {
  zeroTick: string;
  trace: Trace;
  viewport: Viewport;
  labelWidth: number;
  width: number;
  setMarker: (tick: number, alternate: boolean) => void;
  expanded: boolean;
  setExpanded: (expanded: boolean) => void;
}

export function CommandEvents({
  zeroTick,
  trace,
  viewport,
  labelWidth,
  width,
  setMarker,
  expanded,
  setExpanded,
}: Props) {
  const [selectedEvent, setSelectedEvent] = useState<string | null>(null);
  const [hover, setHover] = useState<{
    tick: number;
    left: number;
    bottom: number;
  } | null>(null);
  let firstTick = viewport.start;
  // Expanded labels last until the next output, even if their start is off-screen.
  if (expanded) {
    for (const event of trace.events ?? []) {
      if (event.tick > viewport.start) break;
      firstTick = event.tick;
    }
  }
  const visible = (trace.events ?? []).filter(
    (event) => event.tick >= firstTick && event.tick <= viewport.end,
  );
  const grouped = new Map<number, typeof visible>();
  for (const event of visible) {
    const group = grouped.get(event.tick);
    if (group) group.push(event);
    else grouped.set(event.tick, [event]);
  }
  const selectedTick = visible.find(
    (event) => `${trace.id}:${event.seq}` === selectedEvent,
  )?.tick;
  const preview = visible.filter((event) => event.tick === selectedTick);
  const hovered = hover ? grouped.get(hover.tick) : undefined;
  const showTooltip = (tick: number, target: HTMLButtonElement) => {
    const rect = target.getBoundingClientRect();
    setHover({
      tick,
      left: Math.max(8, Math.min(rect.left, window.innerWidth - 368)),
      bottom: window.innerHeight - rect.top + 6,
    });
  };
  return (
    <section className="command-events" aria-label="Command-block output">
      <div className={`event-lane${expanded ? " expanded" : ""}`}>
        <button
          className="event-heading"
          style={{ width: labelWidth }}
          aria-expanded={expanded}
          onClick={() => setExpanded(!expanded)}
        >
          <MaterialIcon name={expanded ? "expand" : "chevron"} size={16} />{" "}
          Command output{" "}
          <small>
            {visible.length}/{trace.events?.length ?? 0}
          </small>
        </button>
        <div className="event-track" style={{ width }}>
          {[...grouped].map(([tick, outputs], index, groups) => {
            const start = Math.max(tick, viewport.start);
            const end = groups[index + 1]?.[0] ?? viewport.end;
            const details = {
              "aria-label": `Tick ${tickLabel(trace, tick, zeroTick)}: ${outputs.map((event) => event.text).join("; ")}`,
              "aria-describedby":
                hover?.tick === tick ? "command-output-tooltip" : undefined,
              onPointerEnter: (event: React.PointerEvent<HTMLButtonElement>) =>
                showTooltip(tick, event.currentTarget),
              onPointerLeave: () => setHover(null),
              onFocus: (event: React.FocusEvent<HTMLButtonElement>) =>
                showTooltip(tick, event.currentTarget),
              onBlur: () => setHover(null),
              onClick: () => {
                setSelectedEvent(
                  selectedTick === tick
                    ? null
                    : `${trace.id}:${outputs[0].seq}`,
                );
                setMarker(Math.max(tick, trace.start), false);
                setHover(null);
              },
            };
            return (
              <Fragment key={tick}>
                {tick >= viewport.start && (
                  <button
                    {...details}
                    className="event-dot"
                    style={{
                      left: `${(100 * (tick - viewport.start)) / (viewport.end - viewport.start)}%`,
                    }}
                  >
                    ●
                  </button>
                )}
                {expanded && end > start && (
                  <button
                    {...details}
                    className="event-command"
                    style={{
                      left: `${(100 * (start - viewport.start)) / (viewport.end - viewport.start)}%`,
                      width: `${(100 * (end - start)) / (viewport.end - viewport.start)}%`,
                    }}
                  >
                    {outputs.map((event) => event.text).join(" · ")}
                  </button>
                )}
              </Fragment>
            );
          })}
        </div>
      </div>
      {hover && hovered && (
        <div
          id="command-output-tooltip"
          role="tooltip"
          className="event-tooltip"
          style={{ left: hover.left, bottom: hover.bottom }}
        >
          <strong>Tick {tickLabel(trace, hover.tick, zeroTick)}</strong>
          {hovered.map((event) => (
            <div className="event-tooltip-message" key={event.seq}>
              <small>{event.position.join(", ")}</small>
              <div className="event-text">
                {event.text}
                {event.originalBytes > 0
                  ? ` [truncated; ${event.originalBytes} bytes originally]`
                  : ""}
              </div>
            </div>
          ))}
        </div>
      )}
      {preview.length > 0 && (
        <div
          className="event-output"
          role="region"
          aria-label="Selected command output"
        >
          <button
            className="event-close"
            aria-label="Close command preview"
            onClick={() => setSelectedEvent(null)}
          >
            <MaterialIcon name="close" size={16} />
          </button>
          <div className="event-text">
            {preview
              .map(
                (event) =>
                  event.text +
                  (event.originalBytes > 0
                    ? ` [truncated; ${event.originalBytes} bytes originally]`
                    : ""),
              )
              .join("\n")}
          </div>
        </div>
      )}
    </section>
  );
}
