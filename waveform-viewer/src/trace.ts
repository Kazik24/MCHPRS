export type Value = boolean | null;
export interface Change {
  tick: number;
  seq: number | string;
  value: Value;
}
export interface Signal {
  id: string;
  name: string;
  scope: string;
  position: [number, number, number];
  initial: Value;
  changes: Change[];
}
export const signalKey = (signal: Signal): string =>
  JSON.stringify([signal.name, signal.position]);
export interface Trace {
  id: string;
  plot: [number, number];
  start: number;
  end: number;
  signals: Signal[];
  tickOrigin?: string;
  events?: CommandEvent[];
}
export interface CommandEvent {
  tick: number;
  seq: string;
  position: [number, number, number];
  text: string;
  originalBytes: number;
}
export function parseZeroTick(text: unknown): string | null {
  if (typeof text !== "string") return null;
  const value = text.trim() || "0";
  if (!/^\d{1,20}$/.test(value)) return null;
  const tick = BigInt(value);
  return tick <= 18446744073709551615n ? tick.toString() : null;
}

export function tickLabel(trace: Trace, tick: number, zeroTick = "0"): string {
  return (
    BigInt(trace.tickOrigin ?? "0") +
    BigInt(Math.round(tick)) -
    BigInt(zeroTick)
  ).toString();
}
export function compareSeq(a: number | string, b: number | string): number {
  return BigInt(a) < BigInt(b) ? -1 : BigInt(a) > BigInt(b) ? 1 : 0;
}
export interface Viewport {
  start: number;
  end: number;
}

export function valueAt<T>(
  signal: { initial: T; changes: { tick: number; value: T }[] },
  tick: number,
): T {
  let low = 0,
    high = signal.changes.length;
  while (low < high) {
    const mid = (low + high) >>> 1;
    if (signal.changes[mid].tick <= tick) low = mid + 1;
    else high = mid;
  }
  return low ? signal.changes[low - 1].value : signal.initial;
}

export function clampView(view: Viewport, bounds: Viewport): Viewport {
  const width = Math.min(
    Math.max(view.end - view.start, 1),
    bounds.end - bounds.start,
  );
  const start = Math.max(
    bounds.start,
    Math.min(bounds.end - width, view.start),
  );
  return { start, end: start + width };
}

export function zoomAt(
  view: Viewport,
  factor: number,
  anchor: number,
  bounds: Viewport,
): Viewport {
  const ratio = Math.max(0, Math.min(1, anchor));
  const width = Math.max(
    1,
    Math.min(bounds.end - bounds.start, (view.end - view.start) * factor),
  );
  const point = view.start + (view.end - view.start) * ratio;
  return clampView(
    { start: point - width * ratio, end: point + width * (1 - ratio) },
    bounds,
  );
}

export function nextEdge(
  signal: Signal,
  tick: number,
  direction: -1 | 1,
  polarity: "any" | "rising" | "falling" = "any",
): number | undefined {
  const matches = signal.changes.filter(
    (change) => polarity === "any" || change.value === (polarity === "rising"),
  );
  return direction === 1
    ? matches.find((change) => change.tick > tick)?.tick
    : matches
        .slice()
        .reverse()
        .find((change) => change.tick < tick)?.tick;
}

export function tickStep(width: number, pixels: number): number {
  const raw = width / Math.max(1, pixels / 110);
  const scale = 10 ** Math.floor(Math.log10(Math.max(raw, 1)));
  return (
    [1, 2, 5, 10].map((n) => n * scale).find((n) => n >= raw) ?? scale * 10
  );
}
