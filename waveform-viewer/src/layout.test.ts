import assert from "node:assert/strict";
import { test } from "node:test";
import { demoTrace } from "./demo.ts";
import { signalKey } from "./trace.ts";
import { initialSession } from "./state.ts";
import {
  defaultOptions,
  decodeLayout,
  encodeLayout,
  layoutKey,
  readLayout,
  writeLayout,
} from "./layout.ts";

test("layout survives capture and probe-ID changes, preserves omitted probes, and merges new probes", () => {
  const [a, b, c] = demoTrace.signals;
  const original = { ...demoTrace, signals: [a, b, c] };
  const rows = initialSession(original).rows;
  const group = {
    ...rows[0],
    id: "group",
    kind: "group" as const,
    signalId: undefined,
    label: "My group",
    collapsed: true,
  };
  const saved = encodeLayout(
    {
      ...defaultOptions,
      netlistWidth: 333,
      labelWidth: 440,
      eventsExpanded: true,
      snap: false,
      scrollMode: "touchpad",
      rows: [
        group,
        { ...rows[1], enabled: false, color: "#123456", groupId: "group" },
        rows[0],
      ],
    },
    original,
  );
  const changed = {
    ...original,
    id: "new capture",
    signals: [
      { ...b, id: "new-b" },
      { ...a, id: "new-a" },
      { ...c, id: "new-c" },
      { ...c, id: "brand-new", name: "brand-new" },
    ],
  };
  const restored = decodeLayout(saved, changed);
  assert.deepEqual(
    restored.rows.map((row) => row.signalId ?? row.id),
    ["group", "new-b", "new-a", "brand-new"],
  );
  assert.equal(restored.rows[1].enabled, false);
  assert.equal(restored.rows[1].groupId, "group");
  assert.equal(restored.rows[1].color, "#123456");
  assert.equal(restored.eventsExpanded, true);
  assert.equal(restored.netlistWidth, 333);
  assert.equal(restored.labelWidth, 440);
  assert.equal(restored.scrollMode, "touchpad");
  const removed = decodeLayout(saved, {
    ...changed,
    signals: [changed.signals[1]],
  });
  assert.deepEqual(
    removed.rows.map((row) => row.signalId ?? row.id),
    ["group", "new-a"],
  );
  const json = JSON.parse(saved);
  assert.equal(json.events, undefined);
  assert.equal(
    json.signals.some((key: string) => key.includes("changes")),
    false,
  );
  assert.equal(saved.includes("seq"), false);
  assert.notEqual(layoutKey("demo", [3, -2]), layoutKey("live", [3, -2]));
  assert.notEqual(layoutKey("live", [3, -2]), layoutKey("live", [4, -2]));
});

test("malformed and unavailable storage safely falls back to the default layout", () => {
  const broken = {
    getItem: () => "{broken",
    setItem: () => {
      throw Error("quota");
    },
  };
  assert.deepEqual(readLayout("key", demoTrace, broken), {
    ...defaultOptions,
    rows: initialSession(demoTrace).rows,
  });
  assert.doesNotThrow(() =>
    writeLayout("key", { ...defaultOptions, rows: [] }, demoTrace, broken),
  );
  const invalid = JSON.parse(
    encodeLayout(
      { ...defaultOptions, rows: initialSession(demoTrace).rows },
      demoTrace,
    ),
  );
  invalid.labelWidth = Infinity;
  assert.throws(() => decodeLayout(JSON.stringify(invalid), demoTrace));
});

test("zero reference persists exactly and older browser layouts default to absolute ticks", () => {
  const saved = encodeLayout(
    {
      ...defaultOptions,
      zeroTick: "18446744073709551615",
      rows: initialSession(demoTrace).rows,
    },
    demoTrace,
  );
  assert.equal(decodeLayout(saved, demoTrace).zeroTick, "18446744073709551615");
  const old = JSON.parse(saved);
  delete old.zeroTick;
  assert.equal(decodeLayout(JSON.stringify(old), demoTrace).zeroTick, "0");
  old.zeroTick = "-1";
  assert.throws(() => decodeLayout(JSON.stringify(old), demoTrace));
});

test("clock selection and edge locks survive layout and absent probes", () => {
  const rows = initialSession(demoTrace).rows;
  rows[1] = { ...rows[1], clockEdge: "falling" };
  const key = signalKey(demoTrace.signals[0]);
  const text = encodeLayout({ ...defaultOptions, clock: key, rows }, demoTrace);
  const restored = decodeLayout(text, demoTrace);
  assert.equal(restored.clock, key);
  assert.equal(restored.rows[1].clockEdge, "falling");
  const missing = decodeLayout(text, {
    ...demoTrace,
    signals: demoTrace.signals.slice(1),
  });
  assert.equal(missing.clock, key);
  assert.equal(missing.rows[0].clockEdge, "falling");
  const old = JSON.parse(text);
  delete old.clock;
  assert.equal(decodeLayout(JSON.stringify(old), demoTrace).clock, null);
  old.clock = 12;
  assert.throws(() => decodeLayout(JSON.stringify(old), demoTrace));
});
