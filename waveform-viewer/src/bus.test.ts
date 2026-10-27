import assert from "node:assert/strict";
import { test } from "node:test";
import {
  buildBus,
  buildBuses,
  busPreview,
  formatBus,
  memberKeys,
  nextBusEdge,
  syncBusMembership,
} from "./bus.ts";
import type { BusSettings } from "./bus.ts";
import {
  initialSession,
  moveRow,
  parseSession,
  ungroupRows,
  visibleRows,
} from "./state.ts";
import { signalKey, valueAt } from "./trace.ts";
import type { Signal, Trace } from "./trace.ts";
import { defaultOptions, decodeLayout, encodeLayout } from "./layout.ts";

const signals: Signal[] = Array.from({ length: 4 }, (_, bit) => ({
  id: `bit-${bit}`,
  name: `data_${bit}`,
  scope: "plot",
  position: [bit, 64, 0],
  initial: bit === 0,
  changes: [],
}));
const trace: Trace = {
  id: "capture",
  plot: [0, 0],
  tickOrigin: "0",
  start: 0,
  end: 100,
  signals,
};
const settings: BusSettings = {
  radix: 16,
  inverted: false,
  members: signals.map(signalKey),
};
const lookup = (signals: Signal[]) =>
  new Map(signals.map((signal) => [signalKey(signal), signal]));
const makeRows = () => {
  const rows = initialSession(trace).rows;
  return [
    {
      ...rows[0],
      id: "group",
      kind: "group" as const,
      signalId: undefined,
      bus: settings,
    },
    ...rows.map((row) => ({ ...row, groupId: "group" })),
  ];
};

test("bus formats pack low-first bits, pad digits, and invert only declared real bits", () => {
  const bus = buildBus(settings, lookup(signals));
  const expected = new Map([
    [2, "0b0001"],
    [8, "0o01"],
    [10, "1"],
    [16, "0x1"],
  ]);
  const inverse = new Map([
    [2, "0b1110"],
    [8, "0o16"],
    [10, "14"],
    [16, "0xE"],
  ]);
  for (const radix of [2, 8, 10, 16] as const) {
    assert.equal(
      formatBus(bus.initial, { ...settings, radix }),
      expected.get(radix),
    );
    assert.equal(
      formatBus(bus.initial, { ...settings, radix, inverted: true }),
      inverse.get(radix),
    );
  }
  const five = { ...settings, members: [...settings.members, "fifth"] };
  assert.equal(
    formatBus({ bits: 0n, known: 31n }, { ...five, inverted: true }),
    "0x1F",
  );
  assert.equal(
    formatBus({ bits: 0n, known: 31n }, { ...five, radix: 8, inverted: true }),
    "0o37",
  );
  assert.equal(
    formatBus({ bits: 0n, known: 15n }, { ...settings, inverted: true }),
    "0xF",
  );
  assert.equal(formatBus({ bits: 8n, known: 15n }, settings), "0x8");
});

test("unknown bits remain unknown during inversion and decimal falls back to explicit binary", () => {
  const value = { bits: 1n, known: 13n };
  assert.equal(formatBus(value, { ...settings, radix: 2 }), "0b00x1");
  assert.equal(
    formatBus(value, { ...settings, radix: 2, inverted: true }),
    "0b11x0",
  );
  assert.equal(formatBus(value, { ...settings, radix: 10 }), "0b00x1");
  assert.equal(formatBus(value, settings), "0xx");
  const five = { ...settings, members: [...settings.members, "fifth"] };
  assert.equal(formatBus({ bits: 16n, known: 16n }, five), "0x1x");
  assert.equal(
    formatBus({ bits: 16n, known: 16n }, { ...five, radix: 8 }),
    "0oxx",
  );
});

test("64-bit and 1024-bit unsigned values stay exact", () => {
  for (const width of [64, 1024]) {
    const options = {
      ...settings,
      members: Array.from({ length: width }, (_, bit) => String(bit)),
    };
    const maximum = (1n << BigInt(width)) - 1n;
    assert.equal(
      formatBus({ bits: maximum, known: maximum }, { ...options, radix: 10 }),
      maximum.toString(),
    );
    assert.equal(
      formatBus({ bits: 0n, known: maximum }, { ...options, inverted: true }),
      "0x" + "F".repeat(width / 4),
    );
  }
});

test("merged transitions preserve exact sequence order, settled sampling and returning glitches", () => {
  const first = {
    ...signals[0],
    initial: false,
    changes: [
      { tick: 10, seq: "9007199254740994", value: true },
      { tick: 10, seq: "9007199254740996", value: false },
      { tick: 20, seq: "9007199254740998", value: true },
    ],
  };
  const second = {
    ...signals[1],
    changes: [
      { tick: 10, seq: "9007199254740993", value: true },
      { tick: 10, seq: "9007199254740995", value: false },
      { tick: 15, seq: "9007199254740997", value: false },
    ],
  };
  const bus = buildBus(settings, lookup([first, second, ...signals.slice(2)]));
  assert.deepEqual(
    bus.changes.map((change) => change.value.bits),
    [2n, 3n, 1n, 0n, 1n],
  );
  assert.equal(valueAt(bus, 9).bits, 0n);
  assert.equal(valueAt(bus, 10).bits, 0n);
  assert.equal(valueAt(bus, 20).bits, 1n);
  assert.equal(nextBusEdge(bus, 10, 1), 20);
  assert.equal(nextBusEdge(bus, 20, -1), 10);
  assert.equal(nextBusEdge(bus, 20, 1), undefined);
  assert.match(busPreview(bus, settings, 10), /Changes: 0x2 → 0x3 → 0x1 → 0x0/);
  assert.equal(first.changes.length, 3);
});

test("hide and collapse preserve bus membership; reorder and ungroup explicitly change it", () => {
  const rows = makeRows();
  const hidden = syncBusMembership(
    rows,
    rows.map((row) =>
      row.id === signals[0].id ? { ...row, enabled: false } : row,
    ),
    trace,
  );
  assert.equal(buildBuses(hidden, trace).get("group")!.initial.bits, 1n);
  assert.equal(hidden[0].bus, settings);
  const collapsed = hidden.map((row) =>
    row.id === "group" ? { ...row, collapsed: true } : row,
  );
  assert.equal(visibleRows(collapsed).length, 1);
  assert.equal(buildBuses(collapsed, trace).get("group")!.initial.bits, 1n);
  const moved = syncBusMembership(
    rows,
    moveRow(rows, signals[1].id, signals[0].id),
    trace,
  );
  assert.equal(buildBuses(moved, trace).get("group")!.initial.bits, 2n);
  const detached = syncBusMembership(
    rows,
    ungroupRows(rows, [signals[0].id]),
    trace,
  );
  assert.equal(detached[0].bus!.members.length, 3);
  assert.equal(buildBuses(detached, trace).get("group")!.initial.bits, 0n);
  const empty = syncBusMembership(
    rows,
    ungroupRows(
      rows,
      signals.map((signal) => signal.id),
    ),
    trace,
  );
  assert.equal(empty[0].bus, undefined);
  assert.deepEqual(memberKeys(rows, "group", trace), settings.members);
});

test("saved buses keep missing-bit slots across absent probes, ID changes and reappearance", () => {
  const rows = makeRows();
  const saved = encodeLayout({ ...defaultOptions, rows }, trace);
  const missing: Trace = {
    ...trace,
    id: "new",
    signals: signals
      .slice(1)
      .map((signal) => ({ ...signal, id: "new-" + signal.id })),
  };
  const restored = decodeLayout(saved, missing);
  assert.deepEqual(restored.rows[0].bus!.members, settings.members);
  assert.equal(
    formatBus(buildBuses(restored.rows, missing).get("group")!.initial, {
      ...settings,
      radix: 2,
    }),
    "0b000x",
  );
  const resaved = encodeLayout(
    { ...defaultOptions, rows: restored.rows },
    missing,
  );
  const returned = decodeLayout(resaved, trace);
  assert.deepEqual(memberKeys(returned.rows, "group", trace), settings.members);
  assert.equal(buildBuses(returned.rows, trace).get("group")!.initial.bits, 1n);
  const changed = syncBusMembership(
    restored.rows,
    restored.rows.map((row) => ({ ...row, color: "#112233" })),
    missing,
  );
  assert.deepEqual(changed[0].bus!.members, settings.members);
  const session = initialSession(trace);
  assert.deepEqual(
    parseSession(
      JSON.stringify({ version: 1, traceId: trace.id, ...session, rows }),
      trace,
    ).rows[0].bus,
    settings,
  );
});

test("malformed bus settings are rejected without affecting older ordinary groups", () => {
  const rows = makeRows();
  const encode = (bus: unknown) =>
    JSON.stringify({
      version: 1,
      traceId: trace.id,
      ...initialSession(trace),
      rows: [{ ...rows[0], bus }, ...rows.slice(1)],
    });
  for (const bad of [
    null,
    { ...settings, radix: 3 },
    { ...settings, inverted: 1 },
    { ...settings, members: [] },
    { ...settings, members: ["a", "a"] },
    { ...settings, members: Array(1025).fill("a") },
    { ...settings, members: [3] },
  ])
    assert.throws(() => parseSession(encode(bad), trace));
  assert.equal(parseSession(encode(undefined), trace).rows[0].bus, undefined);
});

test("moving groups never splits another group's members or changes bus significance", () => {
  const rows = makeRows();
  const other = { ...rows[0], id: "other", bus: undefined };
  const outsider = { ...rows[1], id: "outsider", groupId: "other" };
  const original = [other, outsider, ...rows];
  const moved = syncBusMembership(
    original,
    moveRow(original, "group", "outsider"),
    trace,
  );
  assert.deepEqual(
    moved.map((row) => row.id),
    [...rows.map((row) => row.id), "other", "outsider"],
  );
  assert.equal(moved[0].bus, settings);
  const back = moveRow(moved, "group", "other", true);
  assert.deepEqual(
    back.map((row) => row.id),
    original.map((row) => row.id),
  );
});
