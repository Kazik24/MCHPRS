import assert from "node:assert/strict";
import { test } from "node:test";
import { toVcd } from "./vcd.ts";

test("VCD maps game ticks to 50 ms and preserves coincident event order", () => {
  const trace = {
    id: "test",
    plot: [0, 0] as [number, number],
    start: 100,
    end: 120,
    signals: [
      {
        id: "a",
        name: "clock",
        scope: "cpu",
        position: [0, 0, 0] as [number, number, number],
        initial: false,
        changes: [
          { tick: 101, seq: 2, value: false },
          { tick: 101, seq: 1, value: true },
          { tick: 102, seq: 3, value: null },
        ],
      },
    ],
  };
  const vcd = toVcd(trace);
  assert.match(vcd, /\$timescale 1 ms \$end/);
  assert.match(vcd, /#50\n1s0\n0s0\n#100\nxs0/);
  assert(vcd.endsWith("#1000\n"));
});
