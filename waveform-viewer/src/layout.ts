import { initialSession, parseSession } from "./state.ts";
import type { Row } from "./state.ts";
import type { Signal, Trace } from "./trace.ts";
import { parseZeroTick, signalKey } from "./trace.ts";
import type { ScrollMode } from "./scroll.ts";

export interface LayoutOptions {
  clock: string | null;
  zeroTick: string;
  netlistWidth: number;
  labelWidth: number;
  eventsExpanded: boolean;
  snap: boolean;
  scrollMode: ScrollMode;
  closedScopes: string[];
}
export interface Layout extends LayoutOptions {
  rows: Row[];
}
export const defaultOptions: LayoutOptions = {
  clock: null,
  zeroTick: "0",
  netlistWidth: 280,
  labelWidth: 292,
  eventsExpanded: false,
  snap: true,
  scrollMode: "auto",
  closedScopes: [],
};
export { signalKey } from "./trace.ts";
export const layoutKey = (source: string, plot: [number, number]): string =>
  `mchprs.waveform.layout.v1:${source}:${plot.join(":")}`;

export function encodeLayout(layout: Layout, trace: Trace): string {
  const signals = new Map(
    trace.signals.map((signal) => [signal.id, signalKey(signal)]),
  );
  return JSON.stringify({
    version: 1,
    ...defaultOptions,
    clock: layout.clock,
    zeroTick: layout.zeroTick,
    netlistWidth: layout.netlistWidth,
    labelWidth: layout.labelWidth,
    eventsExpanded: layout.eventsExpanded,
    snap: layout.snap,
    scrollMode: layout.scrollMode,
    closedScopes: layout.closedScopes,
    signals: [...signals.values()],
    rows: layout.rows.map((row) => ({
      id:
        row.kind === "signal" ? `signal:${signals.get(row.signalId!)}` : row.id,
      signalKey: row.kind === "signal" ? signals.get(row.signalId!) : undefined,
      kind: row.kind,
      label: row.label,
      enabled: row.enabled,
      color: row.color,
      height: row.height,
      groupId: row.groupId,
      collapsed: row.collapsed,
      guides: row.guides,
      shortName: row.shortName,
      bus: row.bus,
      clockEdge: row.clockEdge,
    })),
  });
}

export function decodeLayout(text: string, trace: Trace): Layout {
  if (text.length > 1_048_576) throw Error("Layout is too large");
  const saved = JSON.parse(text);
  const zeroTick = parseZeroTick(saved?.zeroTick ?? "0");
  const clock = saved?.clock ?? null;
  if (
    saved?.version !== 1 ||
    zeroTick === null ||
    (clock !== null && (typeof clock !== "string" || clock.length > 512)) ||
    !Array.isArray(saved.rows) ||
    saved.rows.length > 4096 ||
    !Array.isArray(saved.signals) ||
    saved.signals.length > 4096 ||
    saved.signals.some(
      (key: unknown) => typeof key !== "string" || key.length > 512,
    ) ||
    !Number.isFinite(saved.netlistWidth) ||
    saved.netlistWidth < 180 ||
    saved.netlistWidth > 440 ||
    !Number.isFinite(saved.labelWidth) ||
    saved.labelWidth < 120 ||
    saved.labelWidth > 600 ||
    typeof saved.eventsExpanded !== "boolean" ||
    typeof saved.snap !== "boolean" ||
    !["auto", "mouse", "touchpad"].includes(saved.scrollMode) ||
    !Array.isArray(saved.closedScopes) ||
    saved.closedScopes.length > 1024 ||
    saved.closedScopes.some(
      (scope: unknown) => typeof scope !== "string" || scope.length > 128,
    )
  )
    throw Error("Invalid stored layout");
  const signals = new Map(
    trace.signals.map((signal) => [signalKey(signal), signal]),
  );
  const rows: Row[] = saved.rows.flatMap(
    (row: Row & { signalKey?: string }) => {
      if (!row || typeof row !== "object") throw Error("Invalid stored row");
      if (row.kind !== "signal") return [row];
      const signal = signals.get(row.signalKey ?? "");
      return signal ? [{ ...row, signalId: signal.id }] : [];
    },
  );
  const known = new Set(saved.signals);
  for (const signal of trace.signals) {
    if (!known.has(signalKey(signal))) {
      const group = rows.find((row) =>
        row.bus?.members.includes(signalKey(signal)),
      );
      const row: Row = {
        ...initialSession({ ...trace, signals: [signal] }).rows[0],
        id: `signal:${signalKey(signal)}`,
        groupId: group?.id,
      };
      if (!group) rows.push(row);
      else {
        const bit = group.bus!.members.indexOf(signalKey(signal));
        let at = rows.findIndex((item) => item.id === group.id) + 1;
        while (rows[at]?.groupId === group.id) {
          const other = trace.signals.find(
            (signal) => signal.id === rows[at].signalId,
          );
          if (other && group.bus!.members.indexOf(signalKey(other)) > bit)
            break;
          at++;
        }
        rows.splice(at, 0, row);
      }
    }
  }
  // Reuse the session validator for row fields, unique IDs and group references.
  const session = initialSession(trace);
  const validated = parseSession(
    JSON.stringify({ version: 1, traceId: trace.id, ...session, rows }),
    trace,
  );
  return {
    clock,
    zeroTick,
    rows: validated.rows,
    netlistWidth: saved.netlistWidth,
    labelWidth: saved.labelWidth,
    eventsExpanded: saved.eventsExpanded,
    snap: saved.snap,
    scrollMode: saved.scrollMode,
    closedScopes: saved.closedScopes,
  };
}

type Storage = Pick<globalThis.Storage, "getItem" | "setItem">;
export function readLayout(
  key: string,
  trace: Trace,
  storage?: Storage,
): Layout {
  try {
    const text = (storage ?? globalThis.localStorage).getItem(key);
    if (text) return decodeLayout(text, trace);
  } catch {
    /* Browser storage can be blocked, full, or contain old malformed data. */
  }
  return { ...defaultOptions, rows: initialSession(trace).rows };
}
export function writeLayout(
  key: string,
  layout: Layout,
  trace: Trace,
  storage?: Storage,
): void {
  try {
    (storage ?? globalThis.localStorage).setItem(
      key,
      encodeLayout(layout, trace),
    );
  } catch {
    /* Layout persistence must never prevent viewing a capture. */
  }
}
