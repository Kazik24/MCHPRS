import type { Trace, Value } from "./trace";
import { compareSeq } from "./trace.ts";

export function toVcd(trace: Trace): string {
  const lines = [
    "$version MROWW Waveform Viewer $end",
    "$timescale 1 ms $end",
    "$comment 20 game ticks per second; one tick is 50 ms. Same-tick changes retain sequence order at the same timestamp. $end",
    "$scope module minecraft $end",
  ];
  const ids = trace.signals.map((_, index) => `s${index}`);
  const bit = (value: Value) => (value === null ? "x" : value ? "1" : "0");
  trace.signals.forEach((signal, index) => {
    const name = `${signal.scope}_${signal.name}`.replace(
      /[^A-Za-z0-9_$]/g,
      "_",
    );
    lines.push(`$var wire 1 ${ids[index]} ${name}_${index} $end`);
  });
  lines.push("$upscope $end", "$enddefinitions $end", "#0", "$dumpvars");
  trace.signals.forEach((signal, index) =>
    lines.push(`${bit(signal.initial)}${ids[index]}`),
  );
  lines.push("$end");
  const changes = trace.signals
    .flatMap((signal, index) =>
      signal.changes.map((change) => ({ ...change, id: ids[index] })),
    )
    .sort((a, b) => a.tick - b.tick || compareSeq(a.seq, b.seq));
  let timestamp = 0;
  for (const change of changes) {
    const next = (change.tick - trace.start) * 50;
    if (!Number.isSafeInteger(next) || next < timestamp)
      throw Error("Capture timestamps cannot be exported safely.");
    if (next !== timestamp) {
      lines.push(`#${next}`);
      timestamp = next;
    }
    lines.push(`${bit(change.value)}${change.id}`);
  }
  const end = (trace.end - trace.start) * 50;
  if (!Number.isSafeInteger(end) || end < timestamp)
    throw Error("Invalid capture end time.");
  if (end > timestamp) lines.push(`#${end}`);
  return lines.join("\n") + "\n";
}
