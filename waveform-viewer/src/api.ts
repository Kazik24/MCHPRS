import type { Trace, Value } from "./trace.ts";

type Bit = 0 | 1 | null;
type RecordTuple =
  | [number, 0, number, Bit]
  | [number, 1, number, number, number, string, number];
export interface Page {
  source: string;
  capture: string | null;
  state: string;
  reason?: string | null;
  owner?: string | null;
  signals?: [string, [number, number, number]][];
  window: {
    first_tick: string;
    observed_tick: string;
    completed_tick: string;
    partial_tick: boolean;
  } | null;
  snapshot?: {
    signals: [string, [number, number, number]][];
    baseline: Bit[];
    baseline_tick: string;
    baseline_seq: string;
  } | null;
  seq_base?: string;
  ticks: [string, RecordTuple[]][];
  more?: boolean;
  cursor?: string;
}
export interface Capture {
  trace: Trace;
  lastSeq: string;
  cursor?: string;
  state: string;
  reason?: string | null;
  owner?: string | null;
}
export type ConnectionState = "loading" | "ready" | "unloaded" | "error";
const bit = (value: Bit): Value => {
  if (value !== 0 && value !== 1 && value !== null)
    throw Error("Invalid signal value");
  return value === null ? null : value === 1;
};
const relative = (tick: string, origin: bigint): number => {
  const delta = BigInt(tick) - origin;
  if (delta < 0n || delta > BigInt(Number.MAX_SAFE_INTEGER))
    throw Error("Invalid capture tick range");
  return Number(delta);
};

// Only bounded offsets enter the canvas; absolute ticks and record IDs stay exact.
export function applyPage(
  previous: Capture | undefined,
  page: Page,
  plot: [number, number],
): Capture {
  const id = `${page.source}:${page.capture}`;
  if (!page.capture || !page.window) {
    return {
      trace: {
        id,
        plot,
        tickOrigin: "0",
        start: 0,
        end: 1,
        signals: (page.signals ?? []).map(([name, position], index) => ({
          id: `probe-${index}`,
          name,
          position,
          scope: "plot",
          initial: null,
          changes: [],
        })),
        events: [],
      },
      lastSeq: "0",
      state: page.state,
      reason: page.reason,
      owner: page.owner,
    };
  }
  const origin = BigInt(page.window.first_tick);
  let trace: Trace;
  let lastSeq: bigint;
  if (page.snapshot) {
    const snapshot = page.snapshot;
    if (snapshot.signals.length !== snapshot.baseline.length)
      throw Error("Invalid snapshot baseline");
    trace = {
      id,
      plot,
      tickOrigin: origin.toString(),
      start: 0,
      end: 1,
      events: [],
      signals: snapshot.signals.map(([name, position], index) => ({
        id: `probe-${index}`,
        name,
        position,
        scope: "plot",
        initial: bit(snapshot.baseline[index]),
        changes: [],
      })),
    };
    lastSeq = BigInt(snapshot.baseline_seq);
  } else {
    if (!previous || previous.trace.id !== id)
      throw Error("Capture changed; snapshot required");
    const shift = Number(origin - BigInt(previous.trace.tickOrigin ?? "0"));
    if (!Number.isSafeInteger(shift) || shift < 0)
      throw Error("Invalid capture window");
    trace = {
      ...previous.trace,
      tickOrigin: origin.toString(),
      signals: previous.trace.signals.map((signal) => {
        const removed = signal.changes.filter((change) => change.tick < shift);
        return {
          ...signal,
          initial: removed.length ? removed.at(-1)!.value : signal.initial,
          changes: signal.changes
            .filter((change) => change.tick >= shift)
            .map((change) => ({ ...change, tick: change.tick - shift })),
        };
      }),
      events:
        previous.trace.events
          ?.filter((event) => event.tick >= shift)
          .map((event) => ({ ...event, tick: event.tick - shift })) ?? [],
    };
    lastSeq = BigInt(previous.lastSeq);
  }
  const base = BigInt(page.seq_base ?? "0");
  for (const [tick, records] of page.ticks) {
    const offset = relative(tick, origin);
    for (const record of records) {
      if (!Number.isSafeInteger(record[0]) || record[0] < 1)
        throw Error("Invalid record offset");
      const seq = base + BigInt(record[0]);
      if (seq <= lastSeq) continue;
      if (seq !== lastSeq + 1n)
        throw Error("Missing waveform records; snapshot required");
      if (record[1] === 0) {
        const signal = trace.signals[record[2]];
        if (!signal) throw Error("Unknown waveform signal");
        signal.changes.push({
          tick: offset,
          seq: seq.toString(),
          value: bit(record[3]),
        });
      } else if (record[1] === 1) {
        trace.events!.push({
          tick: offset,
          seq: seq.toString(),
          position: [record[2], record[3], record[4]],
          text: record[5],
          originalBytes: record[6],
        });
      } else throw Error("Unknown waveform record");
      lastSeq = seq;
    }
  }
  trace.end = Math.max(1, relative(page.window.observed_tick, origin));
  return {
    trace,
    lastSeq: lastSeq.toString(),
    cursor: page.cursor,
    state: page.state,
    reason: page.reason,
    owner: page.owner === undefined ? previous?.owner : page.owner,
  };
}

export function connectWaveform(
  plot: [number, number],
  update: (capture: Capture) => void,
  status: (message: string, state: ConnectionState) => void,
): () => void {
  const controller = new AbortController();
  let stream: EventSource | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let capture: Capture | undefined;
  const path = `/api/v1/plots/${plot[0]}/${plot[1]}/waveform`;
  const retry = (message: string, state: ConnectionState = "error") => {
    stream?.close();
    if (controller.signal.aborted) return;
    status(message, state);
    clearTimeout(timer);
    timer = setTimeout(() => void snapshot(), 1000);
  };
  const accept = (page: Page) => {
    if (controller.signal.aborted) return;
    capture = applyPage(capture, page, plot);
    update(capture);
    status(page.reason ? `${page.state}: ${page.reason}` : page.state, "ready");
  };
  async function snapshot() {
    stream?.close();
    capture = undefined;
    try {
      let cursor: string | undefined;
      let page: Page;
      do {
        const response = await fetch(
          `${path}/data${cursor ? `?cursor=${encodeURIComponent(cursor)}` : ""}`,
          { signal: controller.signal, cache: "no-store" },
        );
        if (!response.ok) {
          const problem = await response.json();
          if (problem.error === "plot_not_loaded") {
            retry("Plot is not loaded", "unloaded");
            return;
          }
          throw Error(
            problem.error === "plot_not_loaded"
              ? "Plot is not loaded"
              : (problem.error ?? `HTTP ${response.status}`),
          );
        }
        page = await response.json();
        capture = applyPage(capture, page, plot);
        cursor = page.cursor;
        if (page.more && !cursor) throw Error("Missing snapshot cursor");
      } while (page.more && !controller.signal.aborted);
      if (controller.signal.aborted) return;
      // Keep the displayed capture intact until every replacement snapshot page arrives.
      update(capture!);
      status(
        page.reason ? `${page.state}: ${page.reason}` : page.state,
        "ready",
      );
      if (!cursor) {
        retry(
          page.state === "unsupported_backend"
            ? "Interpreter required"
            : "Recording is off",
          "ready",
        );
        return;
      }
      stream = new EventSource(
        `${path}/stream?cursor=${encodeURIComponent(cursor)}`,
      );
      stream.addEventListener("batch", (event) => {
        try {
          accept(JSON.parse((event as MessageEvent).data));
        } catch (error) {
          retry(error instanceof Error ? error.message : "Invalid stream data");
        }
      });
      stream.addEventListener("reset", () =>
        retry("Capture changed; loading…", "loading"),
      );
      // Re-bootstrap from the last complete snapshot on failure. No speculative cursor advancement.
      stream.onerror = () => retry("Connection lost; reconnecting…");
    } catch (error) {
      if (!controller.signal.aborted)
        retry(error instanceof Error ? error.message : "Could not load plot");
    }
  }
  status("Loading…", "loading");
  void snapshot();
  return () => {
    controller.abort();
    stream?.close();
    clearTimeout(timer);
  };
}
