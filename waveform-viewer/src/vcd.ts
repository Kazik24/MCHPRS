import type { CommandEvent, Trace, Value } from "./trace";
import { compareSeq } from "./trace.ts";

export function toVcd(trace: Trace): string {
  const lines = [
    "$version MROWW Waveform Viewer $end",
    "$timescale 1 ms $end",
    "$comment 20 game ticks per second; one tick is 50 ms. Same-tick changes retain sequence order at the same timestamp. $end",
    "$scope module minecraft $end",
  ];
  const ids = trace.signals.map((_, index) => `s${index}`);
  const outputId = `s${trace.signals.length}`;
  const bit = (value: Value) => (value === null ? "x" : value ? "1" : "0");
  trace.signals.forEach((signal, index) => {
    const name = `${signal.scope}_${signal.name}`.replace(
      /[^A-Za-z0-9_$]/g,
      "_",
    );
    lines.push(`$var wire 1 ${ids[index]} ${name}_${index} $end`);
  });
  if (trace.events?.length)
    lines.push(`$var string 1 ${outputId} command_output $end`);
  lines.push("$upscope $end", "$enddefinitions $end", "#0", "$dumpvars");
  trace.signals.forEach((signal, index) =>
    lines.push(`${bit(signal.initial)}${ids[index]}`),
  );
  if (trace.events?.length) lines.push(`s- ${outputId}`);
  lines.push("$end");
  const changes: { tick: number; seq: number | string; line: string }[] =
    trace.signals.flatMap((signal, index) =>
      signal.changes.map((change) => ({
        tick: change.tick,
        seq: change.seq,
        line: `${bit(change.value)}${ids[index]}`,
      })),
    );
  const events = new Map<number, CommandEvent[]>();
  for (const event of [...(trace.events ?? [])].sort(
    (a, b) => a.tick - b.tick || compareSeq(a.seq, b.seq),
  )) {
    const group = events.get(event.tick) ?? [];
    group.push(event);
    events.set(event.tick, group);
  }
  for (const [tick, group] of events) {
    const text = group
      .map(
        (event) =>
          `(${event.position.join(", ")}) ${event.text}${event.originalBytes > 0 ? ` [truncated; ${event.originalBytes} bytes originally]` : ""}`,
      )
      .join("\n");
    const escaped = [...text]
      .map((char) => {
        const code = char.codePointAt(0)!;
        if (char === "\\") return "\\\\";
        return code <= 0x20 || code === 0x7f
          ? `\\${code.toString(8).padStart(3, "0")}`
          : char;
      })
      .join("");
    changes.push({
      tick,
      seq: group[group.length - 1].seq,
      line: `s${escaped} ${outputId}`,
    });
  }
  changes.sort((a, b) => a.tick - b.tick || compareSeq(a.seq, b.seq));
  let timestamp = 0;
  for (const change of changes) {
    const next = (change.tick - trace.start) * 50;
    if (!Number.isSafeInteger(next) || next < timestamp)
      throw Error("Capture timestamps cannot be exported safely.");
    if (next !== timestamp) {
      lines.push(`#${next}`);
      timestamp = next;
    }
    lines.push(change.line);
  }
  const end = (trace.end - trace.start) * 50;
  if (!Number.isSafeInteger(end) || end < timestamp)
    throw Error("Invalid capture end time.");
  if (end > timestamp) lines.push(`#${end}`);
  return lines.join("\n") + "\n";
}
