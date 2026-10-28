import { MaterialIcon } from "./components/MaterialIcon";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  buildBuses,
  busFormats,
  formatBus,
  memberKeys,
  nextBusEdge,
  syncBusMembership,
} from "./bus";
import type { BusSettings } from "./bus";
import { sampleOnClock } from "./clock";
import {
  clampView,
  nextEdge,
  parseZeroTick,
  tickLabel,
  signalKey,
  valueAt,
  zoomAt,
} from "./trace";
import type { Signal, Trace, Viewport } from "./trace";
import {
  colors,
  initialSession,
  moveRow,
  ungroupRows,
  parseSession,
  visibleRows,
  selectRows,
} from "./state";
import type { Row, Session } from "./state";
import { Netlist } from "./components/Netlist";
import { Toolbar } from "./components/Toolbar";
import type { ScrollMode } from "./scroll";
import { Waveform } from "./components/Waveform";
import { ContextMenu } from "./components/ContextMenu";
import type { MenuItem } from "./components/ContextMenu";
import { ResizeHandle } from "./components/ResizeHandle";
import { toVcd } from "./vcd";
import { connectWaveform } from "./api";
import type { ConnectionState } from "./api";
import { emptyState, ownerLabel } from "./empty";
import { parsePlot } from "./plot";
import { layoutKey, readLayout, writeLayout } from "./layout";
import { BusPlot } from "./components/BusPlot";

function download(text: string, name: string, type = "application/json") {
  const url = URL.createObjectURL(new Blob([text], { type }));
  const link = document.createElement("a");
  link.href = url;
  link.download = name;
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

function emptyTrace(plot: [number, number]): Trace {
  return {
    id: "loading",
    plot,
    start: 0,
    end: 1,
    signals: [],
    events: [],
    tickOrigin: "0",
  };
}

export default function App() {
  const params = new URLSearchParams(window.location.search);
  const initialPlot =
    parsePlot(`${params.get("x") ?? 0} ${params.get("y") ?? 0}`) ??
    ([0, 0] as [number, number]);
  const [trace, setTrace] = useState(() => emptyTrace(initialPlot));
  const [plotInput, setPlotInput] = useState(initialPlot.join(", "));
  const [connection, setConnection] = useState("Loading…");
  const [connectionState, setConnectionState] =
    useState<ConnectionState>("loading");
  const [recorderState, setRecorderState] = useState("loading");
  const [owner, setOwner] = useState<string | null | undefined>(undefined);
  const [loadedPlot, setLoadedPlot] = useState<[number, number]>(initialPlot);
  const currentTrace = useRef(trace);
  useEffect(() => {
    const url = new URL(window.location.href);
    url.searchParams.set("x", String(loadedPlot[0]));
    url.searchParams.set("y", String(loadedPlot[1]));
    window.history.replaceState(window.history.state, "", url);
    const empty = emptyTrace(loadedPlot);
    currentTrace.current = empty;
    setTrace(empty);
    setRecorderState("loading");
    setOwner(undefined);
    setSession(initialSession(empty));
    return connectWaveform(
      loadedPlot,
      (capture) => {
        setRecorderState(capture.state);
        setOwner(capture.owner);
        const previous = currentTrace.current;
        const next = capture.trace;
        const shift = Number(
          BigInt(next.tickOrigin ?? "0") - BigInt(previous.tickOrigin ?? "0"),
        );
        const schemaChanged =
          previous.signals.length !== next.signals.length ||
          previous.signals.some((signal, index) => {
            const other = next.signals[index];
            return (
              signal.name !== other.name ||
              signal.position.some(
                (value, axis) => value !== other.position[axis],
              )
            );
          });
        if (previous.id !== next.id || schemaChanged) {
          restoreLayout(next);
          setSelected([]);
          selectionAnchor.current = null;
        } else {
          setSession((session) => {
            const follow = session.viewport.end >= previous.end;
            const width = session.viewport.end - session.viewport.start;
            return {
              ...session,
              viewport: clampView(
                follow
                  ? { start: next.end - width, end: next.end }
                  : {
                      start: session.viewport.start - shift,
                      end: session.viewport.end - shift,
                    },
                next,
              ),
              marker: Math.max(
                next.start,
                Math.min(next.end, session.marker - shift),
              ),
              altMarker:
                session.altMarker === null
                  ? null
                  : Math.max(
                      next.start,
                      Math.min(next.end, session.altMarker - shift),
                    ),
            };
          });
        }
        currentTrace.current = next;
        setTrace(next);
      },
      (message, state) => {
        setConnection(message);
        setConnectionState(state);
      },
    );
  }, [loadedPlot]);
  const [savedLayout] = useState(() =>
    readLayout(layoutKey("live", loadedPlot), trace),
  );
  const [layoutOwner, setLayoutOwner] = useState(layoutKey("live", loadedPlot));
  const [netlistWidth, setNetlistWidth] = useState(savedLayout.netlistWidth);
  const [labelWidth, setLabelWidth] = useState(savedLayout.labelWidth);
  const [eventsExpanded, setEventsExpanded] = useState(
    savedLayout.eventsExpanded,
  );
  const [closedScopes, setClosedScopes] = useState(savedLayout.closedScopes);
  const bounds = { start: trace.start, end: trace.end };
  const [session, setSession] = useState<Session>(() => ({
    ...initialSession(trace),
    rows: savedLayout.rows,
    zeroTick: savedLayout.zeroTick,
    clock: savedLayout.clock,
  }));
  const [zeroInput, setZeroInput] = useState(savedLayout.zeroTick);
  const [selected, setSelected] = useState<string[]>([]);
  const selectionAnchor = useRef<string | null>(null);
  const [plotRow, setPlotRow] = useState<string | null>(null);
  const busLayout = JSON.stringify(
    session.rows
      .filter((row) => row.bus && row.enabled)
      .map((row) => [row.id, row.bus!.members]),
  );
  const rawBuses = useMemo(
    () => buildBuses(session.rows, trace),
    [busLayout, trace],
  );
  const clock = trace.signals.find(
    (signal) => signalKey(signal) === session.clock,
  );
  const { signals, buses } = useMemo(() => {
    const signals = new Map<string, Signal>();
    const buses = new Map(rawBuses);
    for (const row of session.rows) {
      const raw = trace.signals.find((signal) => signal.id === row.signalId);
      if (raw) {
        signals.set(
          row.id,
          row.clockEdge && signalKey(raw) !== session.clock
            ? {
                ...raw,
                ...sampleOnClock(
                  clock,
                  raw,
                  row.clockEdge,
                  null,
                  (a, b) => a === b,
                ),
              }
            : raw,
        );
      }
      const bus = buses.get(row.id);
      if (bus && row.clockEdge)
        buses.set(row.id, {
          ...bus,
          ...sampleOnClock(
            clock,
            bus,
            row.clockEdge,
            { bits: 0n, known: 0n },
            (a, b) => a.bits === b.bits && a.known === b.known,
          ),
        });
    }
    return { signals, buses };
  }, [session.rows, session.clock, trace, rawBuses]);
  useEffect(() => {
    if (plotRow && !buses.has(plotRow)) setPlotRow(null);
  }, [plotRow, buses]);
  const [search, setSearch] = useState("");
  const [snap, setSnap] = useState(savedLayout.snap);
  const [scrollMode, setScrollMode] = useState<ScrollMode>(
    savedLayout.scrollMode,
  );
  function restoreLayout(next: Trace) {
    const key = layoutKey("live", next.plot);
    const saved = readLayout(key, next);
    setSession({
      ...initialSession(next),
      rows: saved.rows,
      zeroTick: saved.zeroTick,
      clock: saved.clock,
    });
    setZeroInput(saved.zeroTick);
    setNetlistWidth(saved.netlistWidth);
    setLabelWidth(saved.labelWidth);
    setEventsExpanded(saved.eventsExpanded);
    setClosedScopes(saved.closedScopes);
    setSnap(saved.snap);
    setScrollMode(saved.scrollMode);
    setLayoutOwner(key);
  }
  const storageKey = layoutKey("live", loadedPlot);
  useEffect(() => {
    if (
      layoutOwner !== storageKey ||
      trace.id === "loading" ||
      trace.plot[0] !== loadedPlot[0] ||
      trace.plot[1] !== loadedPlot[1]
    )
      return;
    writeLayout(
      storageKey,
      {
        rows: session.rows,
        zeroTick: session.zeroTick,
        clock: session.clock,
        netlistWidth,
        labelWidth,
        eventsExpanded,
        snap,
        scrollMode,
        closedScopes,
      },
      trace,
    );
  }, [
    storageKey,
    layoutOwner,
    session.rows,
    session.zeroTick,
    session.clock,
    netlistWidth,
    labelWidth,
    eventsExpanded,
    snap,
    scrollMode,
    closedScopes,
    // A capture change needs new probe identities; sample batches need no storage writes.
    trace.id,
  ]);
  const [menu, setMenu] = useState<{
    x: number;
    y: number;
    rowId?: string;
  } | null>(null);
  const [notice, setNotice] = useState("");
  useEffect(() => {
    if (!notice) return;
    const timer = setTimeout(() => setNotice(""), 4000);
    return () => clearTimeout(timer);
  }, [notice]);
  const [help, setHelp] = useState(false);
  const loadInput = useRef<HTMLInputElement>(null);
  const setViewport = (viewport: Viewport) =>
    setSession((previous) => ({
      ...previous,
      viewport: clampView(viewport, bounds),
    }));
  const setMarker = (tick: number, alternate: boolean) =>
    setSession((previous) => ({
      ...previous,
      [alternate ? "altMarker" : "marker"]: tick,
    }));
  function applyZeroTick() {
    const tick = parseZeroTick(zeroInput);
    if (tick === null) {
      setNotice("Enter a non-negative whole tick (max 18446744073709551615).");
      setZeroInput(session.zeroTick);
      return;
    }
    setZeroInput(tick);
    setSession((session) => ({ ...session, zeroTick: tick }));
  }
  const closeMenu = useCallback(() => setMenu(null), []);
  const select = (id: string, multiple: boolean, range: boolean) => {
    setSelected((previous) =>
      selectRows(
        session.rows,
        previous,
        selectionAnchor.current,
        id,
        multiple,
        range,
      ),
    );
    if (!range) selectionAnchor.current = id;
  };
  const updateRows = (update: (rows: Row[]) => Row[]) =>
    setSession((previous) => ({
      ...previous,
      rows: syncBusMembership(previous.rows, update(previous.rows), trace),
    }));
  const patchBuses = (patch: Partial<BusSettings>) =>
    updateRows((rows) =>
      rows.map((row) =>
        selected.includes(row.id) && row.bus
          ? { ...row, bus: { ...row.bus, ...patch } }
          : row,
      ),
    );
  const patchRows = (patch: Partial<Row>) =>
    updateRows((rows) =>
      rows.map((row) =>
        (
          menu?.rowId && !selected.includes(menu.rowId)
            ? row.id === menu.rowId
            : selected.includes(row.id)
        )
          ? { ...row, ...patch }
          : row,
      ),
    );
  const reorder = (from: string, to: string) =>
    updateRows((rows) => moveRow(rows, from, to));
  const toggleRow = (id: string) =>
    updateRows((rows) =>
      rows.map((row) =>
        row.id === id ? { ...row, enabled: !row.enabled } : row,
      ),
    );

  function toggleSignal(signal: Signal) {
    updateRows((rows) => {
      const existing = rows.find((row) => row.signalId === signal.id);
      return existing
        ? rows.map((row) =>
            row.id === existing.id ? { ...row, enabled: !row.enabled } : row,
          )
        : [
            ...rows,
            {
              id: signal.id,
              signalId: signal.id,
              kind: "signal",
              label: signal.name,
              enabled: true,
              color: colors[0],
              height: 38,
              guides: "none",
              shortName: false,
            },
          ];
    });
  }
  function context(event: React.MouseEvent, rowId?: string) {
    event.preventDefault();
    if (rowId && !selected.includes(rowId)) {
      setSelected([rowId]);
      selectionAnchor.current = rowId;
    }
    setMenu({ x: event.clientX, y: event.clientY, rowId });
  }
  function addLayout(kind: "group" | "separator", fromSelection = false) {
    const id = crypto.randomUUID();
    updateRows((rows) => {
      const chosen = rows.filter(
        (row) => selected.includes(row.id) && row.kind === "signal",
      );
      const group: Row = {
        id,
        kind,
        label:
          kind === "group"
            ? `Group ${rows.filter((row) => row.kind === "group").length + 1}`
            : "───",
        enabled: true,
        color: colors[0],
        height: 38,
        guides: "none",
        shortName: true,
      };
      if (!fromSelection || !chosen.length) return [...rows, group];
      const first = rows.findIndex(
        (row) => row.id === (chosen[0].groupId ?? chosen[0].id),
      );
      const result = rows.filter(
        (row) => !chosen.some((item) => item.id === row.id),
      );
      result.splice(
        first,
        0,
        group,
        ...chosen.map((row) => ({ ...row, groupId: id })),
      );
      return result;
    });
    setSelected([id]);
    selectionAnchor.current = id;
  }
  function removeSelected() {
    updateRows((rows) =>
      rows
        .filter((row) => !selected.includes(row.id))
        .map((row) =>
          row.groupId && selected.includes(row.groupId)
            ? { ...row, groupId: undefined }
            : row,
        ),
    );
    setSelected([]);
    selectionAnchor.current = null;
  }
  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      setNotice("Copied to clipboard");
    } catch {
      setNotice("Clipboard unavailable. Use the session download instead.");
    }
  }
  function saveSession() {
    download(
      JSON.stringify({ version: 1, traceId: trace.id, ...session }, null, 2),
      "waveform-session.json",
    );
  }
  function exportVcd() {
    try {
      download(toVcd(trace), `plot_${trace.plot.join("_")}.vcd`, "text/plain");
    } catch (error) {
      setNotice(
        error instanceof Error ? error.message : "Could not export waveform.",
      );
    }
  }
  function waveDrom() {
    const start = Math.floor(
      Math.min(session.marker, session.altMarker ?? session.marker + 16),
    );
    const end = Math.min(
      trace.end,
      start + 31,
      Math.ceil(
        Math.max(session.marker, session.altMarker ?? session.marker + 16),
      ),
    );
    const output = visibleRows(session.rows)
      .filter((row) => row.signalId || buses.has(row.id))
      .map((row) => {
        const bus = buses.get(row.id);
        if (bus && row.bus) {
          let last = "";
          const data: string[] = [];
          const wave = Array.from(
            { length: Math.max(1, end - start + 1) },
            (_, index) => {
              const value = formatBus(valueAt(bus, start + index), row.bus!);
              if (index && value === last) return ".";
              last = value;
              data.push(value);
              return "=";
            },
          ).join("");
          return { name: row.label, wave, data };
        }
        const signal = signals.get(row.id)!;
        let last = "";
        const wave = Array.from(
          { length: Math.max(1, end - start + 1) },
          (_, index) => {
            const value = valueAt(signal, start + index);
            const char = value === null ? "x" : value ? "1" : "0";
            const result = char === last ? "." : char;
            last = char;
            return result;
          },
        ).join("");
        return { name: row.label, wave };
      });
    void copy(
      JSON.stringify(
        {
          signal: output,
          head: {
            text: `Settled ticks ${tickLabel(trace, start, session.zeroTick)}–${tickLabel(trace, end, session.zeroTick)}; intra-tick events omitted`,
          },
        },
        null,
        2,
      ),
    );
  }
  function action(name: string) {
    const { viewport, marker } = session;
    const width = viewport.end - viewport.start;
    const bus = buses.get(selected[0]);
    const signal = signals.get(selected[0]);
    if (name === "fit") setViewport(bounds);
    else if (name === "in" || name === "out")
      setViewport(zoomAt(viewport, name === "in" ? 0.5 : 2, 0.5, bounds));
    else if (name === "first" || name === "last")
      setViewport({
        start: name === "first" ? trace.start : trace.end - width,
        end: name === "first" ? trace.start + width : trace.end,
      });
    else if (name === "back" || name === "forward") {
      const tick = Math.max(
        trace.start,
        Math.min(trace.end, marker + (name === "back" ? -1 : 1)),
      );
      setMarker(tick, false);
      if (tick < viewport.start || tick > viewport.end)
        setViewport({ start: tick - width / 2, end: tick + width / 2 });
    } else if (["previous", "next", "rising", "falling"].includes(name)) {
      const edge = bus
        ? ["previous", "next"].includes(name)
          ? nextBusEdge(bus, marker, name === "previous" ? -1 : 1)
          : undefined
        : signal
          ? nextEdge(
              signal,
              marker,
              name === "previous" ? -1 : 1,
              name === "rising"
                ? "rising"
                : name === "falling"
                  ? "falling"
                  : "any",
            )
          : marker + (name === "previous" ? -1 : 1);
      if (edge !== undefined) {
        setMarker(Math.max(trace.start, Math.min(trace.end, edge)), false);
        if (edge < viewport.start || edge > viewport.end)
          setViewport({ start: edge - width / 2, end: edge + width / 2 });
      }
    } else if (name === "search") {
      let tick = NaN;
      if (/^[+-]?\d+$/.test(search.trim())) {
        const relative =
          BigInt(search.trim()) +
          BigInt(session.zeroTick) -
          BigInt(trace.tickOrigin ?? "0");
        if (relative >= 0n && relative <= BigInt(Number.MAX_SAFE_INTEGER))
          tick = Number(relative);
      }
      if (
        search.trim() &&
        Number.isFinite(tick) &&
        tick >= trace.start &&
        tick <= trace.end
      ) {
        setMarker(Math.round(tick), false);
        setViewport({ start: tick - width / 2, end: tick + width / 2 });
      } else {
        const found = session.rows.find((row) => row.label.includes(search));
        if (found) {
          setSelected([found.id]);
          selectionAnchor.current = found.id;
        }
      }
    } else if (name === "help") setHelp(true);
    else if (name === "menu") setMenu({ x: window.innerWidth - 350, y: 100 });
  }
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (
        (event.target as HTMLElement).matches("input,select,textarea") ||
        menu ||
        help ||
        plotRow
      )
        return;
      if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
        event.preventDefault();
        action(event.key === "ArrowLeft" ? "back" : "forward");
      }
      if (event.key === "Home" || event.key === "End") {
        event.preventDefault();
        action(event.key === "Home" ? "first" : "last");
      }
      if (event.key === "0" && (event.ctrlKey || event.metaKey)) {
        event.preventDefault();
        action("fit");
      }
      if (event.key === "s" && (event.ctrlKey || event.metaKey)) {
        event.preventDefault();
        saveSession();
      }
      if (event.key === "Delete" || event.key === "Backspace") {
        event.preventDefault();
        removeSelected();
      }
      if (event.key.toLowerCase() === "g") addLayout("group", true);
      if (/^[1-8]$/.test(event.key))
        patchRows({ color: colors[Number(event.key) - 1] });
      if (
        event.altKey &&
        (event.key === "ArrowUp" || event.key === "ArrowDown")
      ) {
        event.preventDefault();
        const rows = session.rows;
        const index = rows.findIndex((row) => row.id === selected[0]);
        const moving = rows[index];
        let target = index + (event.key === "ArrowUp" ? -1 : 1);
        if (event.key === "ArrowDown" && moving?.kind === "group") {
          while (rows[target]?.groupId === moving.id) target++;
        }
        if (index >= 0 && rows[target])
          updateRows((items) =>
            moveRow(
              items,
              rows[index].id,
              rows[target].id,
              event.key === "ArrowDown",
            ),
          );
      }
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  });

  const activeRow = session.rows.find(
    (row) => row.id === (menu?.rowId ?? selected[0]),
  );
  const activeSignal = trace.signals.find(
    (signal) => signal.id === activeRow?.signalId,
  );
  const separator: MenuItem = { label: "", divider: true };
  const commonMenu: MenuItem[] = [
    { label: "New Group", action: () => addLayout("group") },
    {
      label: "New Group from Selection",
      shortcut: "G",
      action: () => addLayout("group", true),
    },
    { label: "New Separator", action: () => addLayout("separator") },
    separator,
    { label: "Save Viewer Session…", action: saveSession },
    { label: "Load Viewer Session…", action: () => loadInput.current?.click() },
    separator,
    {
      label: "WaveDrom",
      children: [
        { label: "Copy settled ticks between markers", action: waveDrom },
      ],
    },
  ];
  const rowMenu: MenuItem[] = activeRow
    ? [
        ...(activeSignal
          ? [
              {
                label: "Set as Clock",
                checked: signalKey(activeSignal) === session.clock,
                action: () => {
                  const key = signalKey(activeSignal);
                  setSession((previous) => ({
                    ...previous,
                    clock: previous.clock === key ? null : key,
                    rows: previous.rows.map((row) =>
                      previous.clock === key || row.signalId === activeSignal.id
                        ? { ...row, clockEdge: undefined }
                        : row,
                    ),
                  }));
                },
              },
            ]
          : []),
        ...(activeRow.signalId || activeRow.bus
          ? [
              {
                label: "Clock Lock",
                children: (["none", "rising", "falling"] as const).map(
                  (edge) => ({
                    label:
                      edge === "none"
                        ? "None"
                        : edge === "rising"
                          ? "Rising edge"
                          : "Falling edge",
                    checked: (activeRow.clockEdge ?? "none") === edge,
                    disabled:
                      edge !== "none" &&
                      (!session.clock ||
                        (activeSignal &&
                          signalKey(activeSignal) === session.clock)),
                    action: () =>
                      updateRows((rows) =>
                        rows.map((row) =>
                          selected.includes(row.id) &&
                          (row.signalId || row.bus) &&
                          (!row.signalId ||
                            trace.signals.find(
                              (signal) =>
                                signal.id === row.signalId &&
                                signalKey(signal) === session.clock,
                            ) === undefined)
                            ? {
                                ...row,
                                clockEdge: edge === "none" ? undefined : edge,
                              }
                            : row,
                        ),
                      ),
                  }),
                ),
              },
            ]
          : []),
        ...(activeRow.bus
          ? [{ label: "Plot Bus", action: () => setPlotRow(activeRow.id) }]
          : []),
        ...(activeRow.kind === "group"
          ? [
              {
                label: "Show as Bus",
                checked: !!activeRow.bus,
                disabled:
                  !activeRow.bus &&
                  !memberKeys(session.rows, activeRow.id, trace).length,
                action: () =>
                  updateRows((rows) =>
                    rows.map((row) => {
                      if (row.id !== activeRow.id) return row;
                      const members = memberKeys(rows, row.id, trace);
                      return {
                        ...row,
                        clockEdge: row.bus ? undefined : row.clockEdge,
                        bus:
                          row.bus || !members.length
                            ? undefined
                            : { radix: 16, inverted: false, members },
                      };
                    }),
                  ),
              },
            ]
          : []),
        ...(activeRow.bus
          ? [
              {
                label: "Format Values",
                children: [false, true].flatMap((inverted) =>
                  busFormats.map(({ radix, label, signed }) => ({
                    label: (inverted ? "Inverted " : "") + label,
                    checked:
                      activeRow.bus!.radix === radix &&
                      (activeRow.bus!.signed ?? false) === signed &&
                      activeRow.bus!.inverted === inverted,
                    action: () => patchBuses({ radix, inverted, signed }),
                  })),
                ),
              },
            ]
          : []),
        ...(!activeRow.bus
          ? [
              {
                label: "Edge Guides",
                children: (["none", "rising", "falling", "both"] as const).map(
                  (value) => ({
                    label: value[0].toUpperCase() + value.slice(1),
                    checked: activeRow.guides === value,
                    action: () => patchRows({ guides: value }),
                  }),
                ),
              },
            ]
          : []),
        {
          label: "Color",
          children: colors.map((color, index) => ({
            label: `${["Red", "Green", "Blue", "Amber", "Purple", "Orange", "Yellow", "Cyan"][index]}`,
            shortcut: String(index + 1),
            checked: activeRow.color === color,
            action: () => patchRows({ color }),
          })),
        },
        {
          label: "Row Height",
          children: [24, 38, 56, 76].map((height) => ({
            label: `${height} px`,
            checked: activeRow.height === height,
            action: () => patchRows({ height }),
          })),
        },
        {
          label: "Name",
          children: [
            {
              label: "Full name",
              checked: !activeRow.shortName,
              action: () => patchRows({ shortName: false }),
            },
            {
              label: "Short name",
              checked: activeRow.shortName,
              action: () => patchRows({ shortName: true }),
            },
            {
              label: "Rename…",
              action: () => {
                const label = prompt("Display name", activeRow.label);
                if (label?.trim() && label.length <= 128)
                  patchRows({ label: label.trim() });
              },
            },
          ],
        },
        separator,
        ...(session.rows.some((row) => row.groupId && selected.includes(row.id))
          ? [
              {
                label: "Remove from Group",
                action: () => updateRows((rows) => ungroupRows(rows, selected)),
              },
            ]
          : []),
        {
          label: "Remove Variable",
          shortcut: "Delete",
          action: removeSelected,
        },
        {
          label: "Show in Netlist View",
          shortcut: "Ctrl+R",
          action: () => {
            setSearch(activeRow.label);
            setNotice("Netlist filtered to the selected signal");
          },
        },
        {
          label: "Copy Variable Name",
          action: () => {
            const signal = signals.get(activeRow.id);
            void copy(
              signal ? `${signal.scope}.${signal.name}` : activeRow.label,
            );
          },
        },
        {
          label: "Copy Value at Marker",
          action: () => {
            const bus = buses.get(activeRow.id);
            if (bus && activeRow.bus) {
              void copy(formatBus(valueAt(bus, session.marker), activeRow.bus));
              return;
            }
            const signal = signals.get(activeRow.id);
            if (signal) {
              const value = valueAt(signal, session.marker);
              void copy(value === null ? "X" : value ? "1" : "0");
            }
          },
        },
        separator,
        ...commonMenu,
      ]
    : commonMenu;
  return (
    <div className="app">
      <form
        className="topbar"
        onSubmit={(event) => {
          event.preventDefault();
          const plot = parsePlot(plotInput);
          if (!plot) {
            setNotice("Enter plot coordinates as X Y or X, Y.");
            return;
          }
          setLoadedPlot(plot);
          setPlotInput(plot.join(", "));
          setSearch("");
          setMenu(null);
        }}
      >
        <label>
          Plot XY:{" "}
          <input
            aria-label="Plot XY"
            type="text"
            placeholder="X, Y"
            required
            value={plotInput}
            onChange={(event) => setPlotInput(event.target.value)}
          />
        </label>
        <button type="submit">Load</button>
        <span
          className="plot-owner"
          title={`Plot ${loadedPlot.join(", ")} · ${ownerLabel(owner)}`}
          aria-label="Plot info"
        >
          Plot {loadedPlot.join(", ")} · {ownerLabel(owner)}
        </span>
        <button type="button" onClick={exportVcd}>
          Export VCD
        </button>
        <span className="connection-status" role="status">
          {connection}
        </span>
      </form>
      <div className="workspace">
        <Netlist
          trace={trace}
          rows={session.rows}
          query={Number.isFinite(Number(search)) ? "" : search}
          width={netlistWidth}
          toggleSignal={toggleSignal}
          closed={closedScopes}
          setClosed={setClosedScopes}
        />
        <ResizeHandle
          label="Resize netlist"
          value={netlistWidth}
          min={180}
          max={440}
          onChange={setNetlistWidth}
        />
        <main className="editor">
          <Toolbar
            busSelected={buses.has(selected[0])}
            action={action}
            search={search}
            setSearch={setSearch}
            snap={snap}
            setSnap={setSnap}
            scrollMode={scrollMode}
            setScrollMode={setScrollMode}
          />
          <Waveform
            zeroTick={session.zeroTick}
            trace={trace}
            buses={buses}
            signals={signals}
            clock={session.clock}
            rows={visibleRows(session.rows)}
            selected={selected}
            viewport={session.viewport}
            marker={session.marker}
            altMarker={session.altMarker}
            setViewport={setViewport}
            setMarker={setMarker}
            select={select}
            reorder={reorder}
            context={context}
            toggleGroup={(id) =>
              updateRows((rows) =>
                rows.map((row) =>
                  row.id === id ? { ...row, collapsed: !row.collapsed } : row,
                ),
              )
            }
            snap={snap}
            scrollMode={scrollMode}
            labelWidth={labelWidth}
            setLabelWidth={setLabelWidth}
            eventsExpanded={eventsExpanded}
            setEventsExpanded={setEventsExpanded}
            emptyState={emptyState(
              trace,
              recorderState,
              connectionState,
              connection,
            )}
          />
        </main>
      </div>
      <footer className="statusbar">
        <form
          className="zero-tick"
          onSubmit={(event) => {
            event.preventDefault();
            applyZeroTick();
          }}
        >
          <label>
            Zero tick:{" "}
            <input
              aria-label="Zero tick"
              title="Absolute tick shown as zero; press Enter or leave the field to apply"
              type="text"
              inputMode="numeric"
              value={zeroInput}
              onChange={(event) => setZeroInput(event.target.value)}
              onBlur={applyZeroTick}
            />
          </label>
          <button
            type="button"
            aria-label="Reset zero tick to marker A"
            title="Set zero tick to marker A"
            onClick={() => {
              const tick = tickLabel(trace, session.marker);
              setZeroInput(tick);
              setSession((session) => ({ ...session, zeroTick: tick }));
              setNotice("");
            }}
          >
            Reset
          </button>
        </form>
        <span>
          Δt:{" "}
          {session.altMarker === null
            ? "—"
            : session.altMarker - session.marker + " ticks"}
        </span>
        <span>A: {tickLabel(trace, session.marker, session.zeroTick)}</span>
        <span>
          B:{" "}
          {session.altMarker === null
            ? "—"
            : tickLabel(trace, session.altMarker, session.zeroTick)}
        </span>
      </footer>
      {notice && (
        <div className="notice-toast" role="status">
          {notice}
        </div>
      )}
      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          items={menu.rowId ? rowMenu : commonMenu}
          close={closeMenu}
        />
      )}
      {plotRow &&
        (() => {
          const row = session.rows.find((row) => row.id === plotRow);
          const bus = buses.get(plotRow);
          return row?.bus && bus ? (
            <BusPlot
              bus={bus}
              settings={row.bus}
              label={row.label}
              color={row.color}
              trace={trace}
              viewport={session.viewport}
              marker={session.marker}
              zeroTick={session.zeroTick}
              setMarker={setMarker}
              close={() => setPlotRow(null)}
            />
          ) : null;
        })()}
      <input
        type="file"
        accept=".json,application/json"
        hidden
        ref={loadInput}
        onChange={async (event) => {
          const file = event.target.files?.[0];
          if (!file) return;
          try {
            if (file.size > 1_048_576)
              throw Error("Session file is too large.");
            const saved = parseSession(await file.text(), trace);
            setSession(saved);
            setZeroInput(saved.zeroTick);
            setSelected([]);
            selectionAnchor.current = null;
            setNotice("Viewer session restored");
          } catch (error) {
            setNotice(
              error instanceof Error ? error.message : "Invalid session",
            );
          }
          event.target.value = "";
        }}
      />
      {help && (
        <div className="modal-backdrop" onClick={() => setHelp(false)}>
          <section
            className="help-dialog"
            role="dialog"
            aria-modal="true"
            aria-label="Waveform controls"
            onClick={(event) => event.stopPropagation()}
          >
            <button
              className="dialog-close"
              aria-label="Close controls"
              autoFocus
              onClick={() => setHelp(false)}
            >
              <MaterialIcon name="close" />
            </button>
            <h2>Waveform controls</h2>
            <dl>
              <dt>Mouse wheel</dt>
              <dd>Pan through time in Mouse mode</dd>
              <dt>Trackpad</dt>
              <dd>Two axes: time horizontally, signals vertically</dd>
              <dt>Shift + wheel / labels + wheel</dt>
              <dd>Scroll signals vertically</dd>
              <dt>Ctrl / ⌘ + wheel, pinch</dt>
              <dd>Zoom around the pointer</dd>
              <dt>Click / Alt-click / middle-click</dt>
              <dd>Place cursor A / cursor B</dd>
              <dt>Drag waveforms</dt>
              <dd>Pan through time</dd>
              <dt>Drag signal labels</dt>
              <dd>Reorder rows; drop on a group to add to it</dd>
              <dt>Shift-click rows</dt>
              <dd>Select a range of visible rows</dd>
              <dt>Clock and bus plot</dt>
              <dd>
                Right-click a signal to set the clock; right-click rows to lock
                or plot
              </dd>
              <dt>Alt + ↑ / ↓</dt>
              <dd>Reorder selected row with the keyboard</dd>
              <dt>← / →, Home / End, Ctrl+0</dt>
              <dd>Previous/next tick, align to limits, fit capture</dd>
              <dt>Right-click / Delete / G / 1–8</dt>
              <dd>Context menu, remove row, group selection, color</dd>
            </dl>
          </section>
        </div>
      )}
    </div>
  );
}
