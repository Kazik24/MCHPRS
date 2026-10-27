import type { Signal, Trace } from "./trace";
import { compareSeq } from "./trace.ts";

const names = [
  "clock",
  "reset_n",
  "fetch",
  "decode",
  "execute",
  "write_enable",
  "carry",
  "branch_taken",
  "memory_read",
  "memory_ready",
  "output_valid",
  "halt",
  "input_a",
  "input_b",
  "lamp_output",
  "piston_enable",
];
const signals: Signal[] = names.map((name, i) => {
  const period = i === 0 ? 4 : 12 + i * 9;
  const initial = i === 1;
  const changes = [];
  let last = initial;
  for (let tick = 1; tick <= 1000; tick++) {
    const value =
      i === 1
        ? tick > 28
        : (tick + i * 5) % period < Math.max(2, Math.floor(period / 3));
    if (value !== last) {
      changes.push({ tick, seq: tick * 100 + i, value });
      last = value;
    }
  }
  return {
    id: `probe-${i}`,
    name,
    scope: i < 8 ? "cpu.control" : "cpu.io",
    position: [1540 + i * 2, 64 + (i % 3), -1010],
    initial,
    changes,
  };
});

// The sequence records preserve the pulse even though its settled tick value is unchanged.
const glitch = signals[7];
const before =
  glitch.changes.filter((c) => c.tick < 448).at(-1)?.value ?? glitch.initial;
glitch.changes = glitch.changes
  .filter((c) => c.tick !== 448)
  .concat([
    { tick: 448, seq: 44850, value: !before },
    { tick: 448, seq: 44851, value: before },
  ])
  .sort((a, b) => a.tick - b.tick || compareSeq(a.seq, b.seq));

export const demoTrace: Trace = {
  id: "demo-plot-3-minus-2",
  plot: [3, -2],
  start: 0,
  end: 1000,
  signals,
  events: [
    {
      tick: 368,
      seq: "36890",
      position: [1540, 65, -1010],
      text: "CPU reset complete",
      originalBytes: 0,
    },
    {
      tick: 448,
      seq: "44852",
      position: [1542, 65, -1010],
      text: "Branch taken\nJumping to address 128",
      originalBytes: 0,
    },
    {
      tick: 448,
      seq: "44853",
      position: [1544, 65, -1010],
      text: "Debug: carry = 1 <test>",
      originalBytes: 0,
    },
    {
      tick: 512,
      seq: "51290",
      position: [1540, 65, -1010],
      text: "Output ready: Hello, world!",
      originalBytes: 0,
    },
  ],
};

export function demoForPlot(x: number, z: number): Trace {
  return {
    ...demoTrace,
    id: `demo-plot-${x}-${z}`,
    plot: [x, z],
    events: demoTrace.events?.map((event) => ({
      ...event,
      position: [
        event.position[0] + (x - 3) * 512,
        event.position[1],
        event.position[2] + (z + 2) * 512,
      ],
    })),
    signals: demoTrace.signals.map((signal) => ({
      ...signal,
      position: [
        signal.position[0] + (x - 3) * 512,
        signal.position[1],
        signal.position[2] + (z + 2) * 512,
      ],
    })),
  };
}
