import { compareSeq } from "./trace.ts";
import type { Signal } from "./trace.ts";

type Change<T> = { tick: number; seq: number | string; value: T };
type Series<T> = { initial: T; changes: Change<T>[] };

export function sampleOnClock<T>(
  clock: Signal | undefined,
  target: Series<T>,
  edge: "rising" | "falling",
  unknown: T,
  equal: (a: T, b: T) => boolean,
): Series<T> {
  const changes: Change<T>[] = [];
  if (!clock) return { initial: unknown, changes };
  let clockValue = clock.initial,
    value = target.initial,
    held = unknown,
    index = 0;
  for (const transition of clock.changes) {
    while (index < target.changes.length) {
      const change = target.changes[index];
      if (
        change.tick > transition.tick ||
        (change.tick === transition.tick &&
          compareSeq(change.seq, transition.seq) > 0)
      )
        break;
      value = change.value;
      index++;
    }
    const selected =
      edge === "rising"
        ? clockValue === false && transition.value === true
        : clockValue === true && transition.value === false;
    clockValue = transition.value;
    if (selected && !equal(held, value)) {
      changes.push({ tick: transition.tick, seq: transition.seq, value });
      held = value;
    }
  }
  return { initial: unknown, changes };
}
