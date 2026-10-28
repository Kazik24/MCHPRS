import assert from "node:assert/strict";
import { test } from "node:test";
import {
  initialSession,
  moveRow,
  parseSession,
  visibleRows,
  ungroupRows,
  selectRows,
} from "./state.ts";
import { demoTrace } from "./demo.ts";

test("shift selection spans visible rows and control-shift adds the range", () => {
  const rows = initialSession(demoTrace).rows.slice(0, 5);
  rows[2] = { ...rows[2], enabled: false };
  assert.deepEqual(
    selectRows(rows, [rows[0].id], rows[0].id, rows[4].id, false, true),
    [rows[0].id, rows[1].id, rows[3].id, rows[4].id],
  );
  assert.deepEqual(
    selectRows(rows, [rows[4].id], rows[0].id, rows[1].id, true, true),
    [rows[4].id, rows[0].id, rows[1].id],
  );
  assert.deepEqual(
    selectRows(rows, [rows[0].id], "missing", rows[3].id, false, true),
    [rows[3].id],
  );
  assert.deepEqual(
    selectRows(rows, [rows[0].id], rows[0].id, rows[0].id, true, false),
    [],
  );
});

test("session preserves clock settings and rejects invalid edge modes", () => {
  const session = initialSession(demoTrace);
  session.clock = "clock-key";
  session.rows[1] = { ...session.rows[1], clockEdge: "rising" };
  const encode = () =>
    JSON.stringify({ version: 1, traceId: demoTrace.id, ...session });
  assert.equal(parseSession(encode(), demoTrace).clock, "clock-key");
  assert.equal(parseSession(encode(), demoTrace).rows[1].clockEdge, "rising");
  const invalid = JSON.parse(encode());
  invalid.rows[1].clockEdge = "either";
  assert.throws(() => parseSession(JSON.stringify(invalid), demoTrace));
});

test("ungroup keeps the signal after its former group and visible when collapsed", () => {
  const [first, second, outside] = initialSession(demoTrace).rows;
  const group = {
    ...first,
    id: "group",
    kind: "group" as const,
    collapsed: true,
  };
  const rows = [
    group,
    { ...first, groupId: group.id },
    { ...second, groupId: group.id },
    outside,
  ];
  const result = ungroupRows(rows, [first.id]);
  assert.deepEqual(
    result.map((row) => row.id),
    [group.id, second.id, first.id, outside.id],
  );
  assert.equal(result[2].groupId, undefined);
  assert.equal(result[2].signalId, first.signalId);
  assert.ok(visibleRows(result).some((row) => row.id === first.id));
  assert.equal(rows[1].groupId, group.id);
  assert.deepEqual(
    ungroupRows(rows, [first.id, second.id]).map((row) => row.id),
    [group.id, first.id, second.id, outside.id],
  );
});

test("hidden probes remain in session and group movement keeps children attached", () => {
  const session = initialSession(demoTrace);
  const [first, second, third] = session.rows;
  const group = {
    ...first,
    id: "group",
    kind: "group" as const,
    signalId: undefined,
  };
  const rows = [group, { ...first, groupId: "group" }, second, third];
  const moved = moveRow(rows, "group", third.id);
  assert.deepEqual(
    moved.map((row) => row.id),
    [second.id, "group", first.id, third.id],
  );
  assert.equal(visibleRows([{ ...first, enabled: false }, second]).length, 1);
  assert.equal(moveRow(rows, "group", first.id), rows);
});
test("session input rejects unknown signals, duplicates, invalid bounds and group cycles", () => {
  const session = initialSession(demoTrace);
  const encode = (patch: object) =>
    JSON.stringify({ version: 1, traceId: demoTrace.id, ...session, ...patch });
  assert.deepEqual(parseSession(encode({}), demoTrace), session);
  assert.throws(() =>
    parseSession(
      encode({ rows: [session.rows[0], session.rows[0]] }),
      demoTrace,
    ),
  );
  assert.throws(() =>
    parseSession(
      encode({ rows: [{ ...session.rows[0], signalId: "missing" }] }),
      demoTrace,
    ),
  );
  assert.throws(() =>
    parseSession(encode({ viewport: { start: 0, end: Infinity } }), demoTrace),
  );
  assert.throws(() =>
    parseSession(
      encode({
        rows: [
          { ...session.rows[0], kind: "group", groupId: session.rows[0].id },
        ],
      }),
      demoTrace,
    ),
  );
});

test("sessions support all 1024 probes admitted by the API", () => {
  const trace = {
    ...demoTrace,
    signals: Array.from({ length: 1024 }, (_, index) => ({
      ...demoTrace.signals[0],
      id: `probe-${index}`,
    })),
  };
  const session = initialSession(trace);
  session.rows = trace.signals.map((signal) => ({
    ...session.rows[0],
    id: signal.id,
    signalId: signal.id,
  }));
  assert.equal(
    parseSession(
      JSON.stringify({ version: 1, traceId: trace.id, ...session }),
      trace,
    ).rows.length,
    1024,
  );
});
