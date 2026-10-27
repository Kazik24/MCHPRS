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

test("VCD exports ordered same-tick command output as a safe string waveform", () => {
  const trace = {
    id: "test",
    plot: [0, 0] as [number, number],
    start: 0,
    end: 3,
    signals: [
      {
        id: "a",
        name: "clock",
        scope: "cpu",
        position: [0, 0, 0] as [number, number, number],
        initial: false,
        changes: [
          { tick: 1, seq: "9007199254740993", value: true },
          { tick: 1, seq: "9007199254740996", value: false },
        ],
      },
    ],
    events: [
      {
        tick: 1,
        seq: "9007199254740995",
        position: [1, 2, 3] as [number, number, number],
        text: "two\\paths $end\n猫",
        originalBytes: 80,
      },
      {
        tick: 1,
        seq: "9007199254740994",
        position: [-1, 0, 5] as [number, number, number],
        text: "hello world",
        originalBytes: 0,
      },
      {
        tick: 2,
        seq: "9007199254740997",
        position: [0, 0, 0] as [number, number, number],
        text: "next",
        originalBytes: 0,
      },
    ],
  };
  const vcd = toVcd(trace);
  assert.match(vcd, /\$var string 1 s1 command_output \$end/);
  assert.ok(
    vcd.includes(String.raw`#50
1s0
s(-1,\0400,\0405)\040hello\040world\012(1,\0402,\0403)\040two\\paths\040$end\012猫\040[truncated;\04080\040bytes\040originally] s1
0s0
#100
s(0,\0400,\0400)\040next s1`),
  );
  assert.equal((vcd.match(/^s.* s1$/gm) ?? []).length, 3);
  assert.ok(
    toVcd({ ...trace, signals: [] }).includes("$var string 1 s0 command_output"),
  );
  assert.ok(!toVcd({ ...trace, events: [] }).includes("command_output"));
});
