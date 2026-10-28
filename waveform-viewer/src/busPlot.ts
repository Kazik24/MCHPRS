import { numericBus } from "./bus.ts";
import type { Bus, BusSettings } from "./bus.ts";
import { valueAt } from "./trace.ts";
import type { Viewport } from "./trace.ts";

export function busPlot(bus: Bus, settings: BusSettings, viewport: Viewport) {
  const samples = [
    {
      tick: viewport.start,
      value: numericBus(valueAt(bus, viewport.start), settings),
    },
    ...bus.changes
      .filter(
        (change) => change.tick > viewport.start && change.tick <= viewport.end,
      )
      .map((change) => ({
        tick: change.tick,
        value: numericBus(change.value, settings),
      })),
  ];
  const known = samples.flatMap((sample) =>
    sample.value === null ? [] : [sample.value],
  );
  if (!known.length) return { path: "", min: null, max: null };
  const min = known.reduce((a, b) => (a < b ? a : b));
  const max = known.reduce((a, b) => (a > b ? a : b));
  const x = (tick: number) =>
    100 + ((tick - viewport.start) / (viewport.end - viewport.start)) * 880;
  const y = (value: bigint) =>
    min === max
      ? 145
      : 270 -
        (Number(((value - min) * 1000000n) / (max - min)) / 1000000) * 250;
  let path = "";
  for (let i = 0; i < samples.length; i++) {
    const sample = samples[i],
      next = samples[i + 1];
    if (sample.value === null) continue;
    path += `M${x(sample.tick)},${y(sample.value)} H${x(next?.tick ?? viewport.end)}`;
    if (next?.value !== null && next?.value !== undefined)
      path += ` V${y(next.value)}`;
  }
  return { path, min, max };
}
