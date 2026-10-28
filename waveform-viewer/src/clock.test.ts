import assert from "node:assert/strict";
import { test } from "node:test";
import { sampleOnClock } from "./clock.ts";
import { buildBus } from "./bus.ts";
import { signalKey, valueAt } from "./trace.ts";
import type { Signal } from "./trace.ts";

const clock: Signal = {
  id: "clock",
  name: "clock",
  scope: "plot",
  position: [0, 0, 0],
  initial: false,
  changes: [
    { tick: 5, seq: "9007199254740994", value: true },
    { tick: 8, seq: "9007199254740996", value: false },
    { tick: 12, seq: "9007199254740998", value: true },
    { tick: 15, seq: "9007199254741000", value: null },
    { tick: 16, seq: "9007199254741001", value: true },
    { tick: 17, seq: "9007199254741002", value: false },
  ],
};
const data: Signal = {
  id: "data",
  name: "data",
  scope: "plot",
  position: [1, 0, 0],
  initial: false,
  changes: [
    { tick: 5, seq: "9007199254740993", value: true },
    { tick: 5, seq: "9007199254740995", value: false },
    { tick: 9, seq: "9007199254740997", value: true },
    { tick: 12, seq: "9007199254740999", value: false },
  ],
};

test("clock-locked signals sample on the selected edges using same-tick sequence order", () => {
  const rising = sampleOnClock(clock, data, "rising", null, (a, b) => a === b);
  assert.equal(valueAt(rising, 4), null);
  assert.equal(valueAt(rising, 5), true);
  assert.equal(valueAt(rising, 8), true);
  assert.equal(valueAt(rising, 12), true);
  assert.deepEqual(
    rising.changes.map((change) => change.tick),
    [5],
  );
  const falling = sampleOnClock(
    clock,
    data,
    "falling",
    null,
    (a, b) => a === b,
  );
  assert.equal(valueAt(falling, 7), null);
  assert.equal(valueAt(falling, 8), false);
  assert.equal(valueAt(falling, 16), false);
  assert.deepEqual(
    falling.changes.map((change) => change.tick),
    [8],
  );
  assert.deepEqual(
    sampleOnClock(undefined, data, "rising", null, (a, b) => a === b),
    { initial: null, changes: [] },
  );
  const changed = sampleOnClock(
    clock,
    { ...data, changes: data.changes.filter((change) => change.tick !== 9) },
    "rising",
    null,
    (a, b) => a === b,
  );
  assert.deepEqual(
    changed.changes.map((change) => [change.tick, change.value]),
    [
      [5, true],
      [12, false],
    ],
  );
  assert.equal(data.changes.length, 4);
});

test("bus sampling holds the combined value and keeps unknown clock transitions from creating edges", () => {
  const second = {
    ...data,
    id: "second",
    name: "second",
    position: [2, 0, 0] as [number, number, number],
    initial: true,
    changes: [],
  };
  const settings = {
    radix: 16 as const,
    inverted: false,
    members: [signalKey(data), signalKey(second)],
  };
  const raw = buildBus(
    settings,
    new Map([
      [signalKey(data), data],
      [signalKey(second), second],
    ]),
  );
  const sampled = sampleOnClock(
    clock,
    raw,
    "rising",
    { bits: 0n, known: 0n },
    (a, b) => a.bits === b.bits && a.known === b.known,
  );
  assert.equal(valueAt(sampled, 4).known, 0n);
  assert.equal(valueAt(sampled, 5).bits, 3n);
  assert.equal(valueAt(sampled, 8).bits, 3n);
  assert.deepEqual(
    sampled.changes.map((change) => change.tick),
    [5],
  );
});
