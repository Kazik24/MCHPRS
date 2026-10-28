import assert from "node:assert/strict";
import { test } from "node:test";
import { busPlot } from "./busPlot.ts";
import type { Bus, BusSettings } from "./bus.ts";

test("bus plot handles signed, inverted, large and unknown values without rounding its range", () => {
  const settings: BusSettings = {
    radix: 10,
    signed: true,
    inverted: false,
    members: Array.from({ length: 64 }, (_, i) => String(i)),
  };
  const known = (1n << 64n) - 1n;
  const bus: Bus = {
    members: [],
    initial: { bits: 0n, known },
    changes: [
      { tick: 2, seq: 1, value: { bits: 1n << 63n, known } },
      { tick: 4, seq: 2, value: { bits: 1n, known: 0n } },
      { tick: 6, seq: 3, value: { bits: 7n, known } },
    ],
  };
  const plot = busPlot(bus, settings, { start: 0, end: 8 });
  assert.equal(plot.min, -(1n << 63n));
  assert.equal(plot.max, 7n);
  assert.equal((plot.path.match(/M/g) ?? []).length, 3);
  assert.ok(!plot.path.includes("NaN"));
  const inverted = busPlot(
    bus,
    { ...settings, inverted: true },
    { start: 0, end: 8 },
  );
  assert.equal(inverted.max, (1n << 63n) - 1n);
  const unknown = busPlot(
    { ...bus, initial: { bits: 0n, known: 0n }, changes: [] },
    settings,
    { start: 0, end: 8 },
  );
  assert.deepEqual(unknown, { path: "", min: null, max: null });
  const width = 1024;
  const wideKnown = (1n << BigInt(width)) - 1n;
  const wide = busPlot(
    {
      ...bus,
      initial: { bits: 0n, known: wideKnown },
      changes: [
        { tick: 2, seq: 1, value: { bits: 1n << 1023n, known: wideKnown } },
      ],
    },
    {
      ...settings,
      members: Array.from({ length: width }, (_, i) => String(i)),
    },
    { start: 0, end: 8 },
  );
  assert.equal(wide.min, -(1n << 1023n));
  assert.ok(!wide.path.includes("Infinity"));
});
