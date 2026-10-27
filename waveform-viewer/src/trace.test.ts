import assert from "node:assert/strict";
import { test } from "node:test";
import {
  valueAt,
  zoomAt,
  clampView,
  nextEdge,
  parseZeroTick,
  tickLabel,
} from "./trace.ts";

const signal = {
  id: "a",
  name: "a",
  scope: "test",
  position: [0, 0, 0] as [number, number, number],
  initial: false,
  changes: [
    { tick: 4, seq: 1, value: true },
    { tick: 4, seq: 2, value: false },
    { tick: 8, seq: 3, value: true },
  ],
};
test("settled sampling preserves order without deleting same-tick pulse records", () => {
  assert.equal(valueAt(signal, 3), false);
  assert.equal(valueAt(signal, 4), false);
  assert.equal(valueAt(signal, 8), true);
  assert.equal(signal.changes.filter((c) => c.tick === 4).length, 2);
  assert.equal(nextEdge(signal, 4, 1), 8);
});
test("pointer zoom preserves tick under pointer and pan clamps to retained range", () => {
  const view = { start: 100, end: 300 };
  const zoomed = zoomAt(view, 0.5, 0.25, { start: 0, end: 1000 });
  assert.equal(zoomed.start + (zoomed.end - zoomed.start) * 0.25, 150);
  assert.deepEqual(
    clampView({ start: -20, end: 80 }, { start: 0, end: 1000 }),
    { start: 0, end: 100 },
  );
  assert.deepEqual(
    clampView({ start: 950, end: 1050 }, { start: 0, end: 1000 }),
    { start: 900, end: 1000 },
  );
});

test("zero reference shifts tick labels exactly, including negative and u64 ticks", () => {
  const trace = { tickOrigin: "18446744073709551500" } as Parameters<
    typeof tickLabel
  >[0];
  assert.equal(tickLabel(trace, 10, "18446744073709551510"), "0");
  assert.equal(tickLabel(trace, 0, "18446744073709551510"), "-10");
  assert.equal(tickLabel(trace, 20, "18446744073709551510"), "10");
  assert.equal(
    tickLabel(
      { ...trace, tickOrigin: "18446744073709551505" },
      5,
      "18446744073709551510",
    ),
    "0",
  );
  assert.equal(parseZeroTick(" 0010 "), "10");
  assert.equal(parseZeroTick(""), "0");
  assert.equal(parseZeroTick("   "), "0");
  assert.equal(parseZeroTick("18446744073709551615"), "18446744073709551615");
  for (const value of [
    "-1",
    "1.5",
    "1e3",
    "18446744073709551616",
    "1".repeat(100),
    null,
    10,
  ])
    assert.equal(parseZeroTick(value), null);
});
