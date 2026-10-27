import assert from "node:assert/strict";
import { test } from "node:test";
import { applyPage } from "./api.ts";
import type { Page } from "./api.ts";
import { tickLabel } from "./trace.ts";
import { toVcd } from "./vcd.ts";

const origin = 18446744073709541615n;
const base = 9007199254740993n;
const first: Page = {
  source: "abc",
  capture: "def",
  state: "recording",
  window: {
    first_tick: origin.toString(),
    observed_tick: (origin + 5n).toString(),
    completed_tick: (origin + 5n).toString(),
    partial_tick: false,
  },
  snapshot: {
    signals: [["clock", [1, 64, 2]]],
    baseline: [0],
    baseline_tick: origin.toString(),
    baseline_seq: base.toString(),
  },
  seq_base: base.toString(),
  ticks: [
    [
      (origin + 1n).toString(),
      [
        [1, 0, 0, 1],
        [2, 1, 4, 65, 6, "<b>hello</b>\nworld", 0],
        [3, 0, 0, 0],
      ],
    ],
  ],
  more: true,
  cursor: "page2",
};

test("snapshot pagination preserves u64 ticks, same-tick order, text, and exact sequence IDs", () => {
  const capture = applyPage(undefined, first, [3, -2]);
  assert.equal(capture.trace.signals[0].changes[0].seq, (base + 1n).toString());
  assert.equal(capture.trace.events![0].text, "<b>hello</b>\nworld");
  assert.equal(tickLabel(capture.trace, 1), (origin + 1n).toString());
  const next: Page = {
    ...first,
    snapshot: null,
    seq_base: (base + 3n).toString(),
    ticks: [[(origin + 2n).toString(), [[1, 0, 0, null]]]],
    more: false,
    cursor: "tail",
  };
  const finished = applyPage(capture, next, [3, -2]);
  assert.equal(finished.lastSeq, (base + 4n).toString());
  assert.equal(finished.trace.signals[0].changes.at(-1)!.value, null);
  const repeated = applyPage(finished, next, [3, -2]);
  assert.deepEqual(repeated.trace, finished.trace);
  const vcd = toVcd(finished.trace);
  assert.ok(
    vcd.includes(String.raw`#50
1s0
s(4,\04065,\0406)\040<b>hello</b>\012world s1
0s0`),
  );
  assert.ok(vcd.includes("#100\nxs0"));
});

test("quiet rollover prunes old records and retains unknown baseline without losing sequence continuity", () => {
  let capture = applyPage(undefined, first, [3, -2]);
  capture = applyPage(
    capture,
    {
      ...first,
      snapshot: null,
      seq_base: (base + 3n).toString(),
      ticks: [[(origin + 2n).toString(), [[1, 0, 0, null]]]],
    },
    [3, -2],
  );
  const rolled = applyPage(
    capture,
    {
      ...first,
      snapshot: null,
      seq_base: capture.lastSeq,
      ticks: [],
      window: {
        ...first.window!,
        first_tick: (origin + 3n).toString(),
        observed_tick: (origin + 8n).toString(),
      },
    },
    [3, -2],
  );
  assert.equal(rolled.trace.signals[0].initial, null);
  assert.deepEqual(rolled.trace.signals[0].changes, []);
  assert.deepEqual(rolled.trace.events, []);
  assert.equal(rolled.trace.end, 5);
  assert.equal(rolled.lastSeq, capture.lastSeq);
});

test("missing records and changed capture require a fresh snapshot", () => {
  const capture = applyPage(undefined, first, [3, -2]);
  assert.throws(
    () =>
      applyPage(
        capture,
        {
          ...first,
          snapshot: null,
          seq_base: (base + 4n).toString(),
          ticks: [[origin.toString(), [[1, 0, 0, 1]]]],
        },
        [3, -2],
      ),
    /Missing/,
  );
  assert.throws(
    () =>
      applyPage(capture, { ...first, capture: "new", snapshot: null }, [3, -2]),
    /snapshot/,
  );
  assert.throws(
    () =>
      applyPage(
        undefined,
        { ...first, ticks: [[origin.toString(), [[1, 0, 10, 1]]]] },
        [3, -2],
      ),
    /Unknown/,
  );
});

test("disabled recording retains the registered netlist with unknown values", () => {
  const disabled = applyPage(
    undefined,
    {
      source: "abc",
      capture: null,
      state: "disabled",
      window: null,
      signals: [["registered", [1, 64, 2]]],
      ticks: [],
    },
    [3, -2],
  );
  assert.equal(disabled.trace.signals[0].name, "registered");
  assert.equal(disabled.trace.signals[0].initial, null);
  assert.equal(disabled.cursor, undefined);
});

test("plot owner propagates from metadata and stream batches including unclaimed plots", () => {
  const initial = applyPage(undefined, { ...first, owner: "Maciej" }, [3, -2]);
  assert.equal(initial.owner, "Maciej");
  const next = applyPage(
    initial,
    { ...first, snapshot: null, ticks: [], owner: null },
    [3, -2],
  );
  assert.equal(next.owner, null);
  const metadata = applyPage(
    undefined,
    {
      source: "abc",
      capture: null,
      state: "disabled",
      owner: "Owner",
      window: null,
      ticks: [],
    },
    [3, -2],
  );
  assert.equal(metadata.owner, "Owner");
});

test("background retries keep unloaded and recorder-off status without flashing loading", async (t) => {
  const { connectWaveform } = await import("./api.ts");
  const timers: (() => void)[] = [];
  const states: string[] = [];
  let unloaded = true;
  t.mock.method(globalThis, "fetch", async () =>
    unloaded
      ? Response.json({ error: "plot_not_loaded" }, { status: 404 })
      : Response.json({
          source: "abc",
          capture: null,
          state: "empty",
          owner: null,
          window: null,
          ticks: [],
        }),
  );
  t.mock.method(globalThis, "setTimeout", (callback: () => void) => {
    timers.push(callback);
    return 1;
  });
  t.mock.method(globalThis, "clearTimeout", () => {});
  const close = connectWaveform(
    [3, -2],
    () => {},
    (_message, state) => states.push(state),
  );
  t.after(close);
  const settle = () => new Promise<void>((resolve) => setImmediate(resolve));
  await settle();
  assert.deepEqual(states, ["loading", "unloaded"]);
  timers.shift()!();
  await settle();
  assert.deepEqual(states, ["loading", "unloaded", "unloaded"]);
  unloaded = false;
  timers.shift()!();
  await settle();
  assert.equal(states.at(-1), "ready");
  timers.shift()!();
  await settle();
  assert.equal(states.filter((state) => state === "loading").length, 1);
  assert.equal(states.at(-1), "ready");
});
