import { compareSeq, signalKey, valueAt } from "./trace.ts";
import type { Signal, Trace, Value } from "./trace.ts";
import type { Row } from "./state.ts";

export interface BusSettings {
  radix: 2 | 8 | 10 | 16;
  inverted: boolean;
  signed?: boolean;
  members: string[];
}
export interface BusValue {
  bits: bigint;
  known: bigint;
}
export interface Bus {
  members: (Signal | undefined)[];
  initial: BusValue;
  changes: { tick: number; seq: number | string; value: BusValue }[];
}
export const busFormats = [
  { radix: 2, label: "Binary", signed: false },
  { radix: 8, label: "Octal", signed: false },
  { radix: 10, label: "Decimal", signed: false },
  { radix: 10, label: "Decimal U2", signed: true },
  { radix: 16, label: "Hexadecimal", signed: false },
] as const;

export function memberKeys(rows: Row[], group: string, trace: Trace): string[] {
  const signals = new Map(trace.signals.map((signal) => [signal.id, signal]));
  return rows
    .filter((row) => row.kind === "signal" && row.groupId === group)
    .flatMap((row) => {
      const signal = signals.get(row.signalId!);
      return signal ? [signalKey(signal)] : [];
    });
}

export function syncBusMembership(
  before: Row[],
  after: Row[],
  trace: Trace,
): Row[] {
  const children = (rows: Row[], id: string) =>
    rows
      .filter((row) => row.kind === "signal" && row.groupId === id)
      .map((row) => row.id);
  return after.map((row) => {
    if (
      !row.bus ||
      JSON.stringify(children(before, row.id)) ===
        JSON.stringify(children(after, row.id))
    )
      return row;
    const members = memberKeys(after, row.id, trace);
    return {
      ...row,
      bus: members.length ? { ...row.bus, members } : undefined,
      clockEdge: members.length ? row.clockEdge : undefined,
    };
  });
}

export function buildBus(
  settings: BusSettings,
  signals: Map<string, Signal>,
): Bus {
  const members = settings.members.map((key) => signals.get(key));
  let bits = 0n,
    known = 0n;
  const apply = (bit: number, value: Value) => {
    const mask = 1n << BigInt(bit);
    bits = value === true ? bits | mask : bits & ~mask;
    known = value === null ? known & ~mask : known | mask;
  };
  members.forEach((signal, bit) => apply(bit, signal?.initial ?? null));
  const initial = { bits, known };
  const records = members.flatMap(
    (signal, bit) =>
      signal?.changes.map((change) => ({ ...change, bit })) ?? [],
  );
  records.sort(
    (a, b) => a.tick - b.tick || compareSeq(a.seq, b.seq) || a.bit - b.bit,
  );
  const changes: Bus["changes"] = [];
  for (const record of records) {
    const previousBits = bits,
      previousKnown = known;
    apply(record.bit, record.value);
    if (bits !== previousBits || known !== previousKnown)
      changes.push({
        tick: record.tick,
        seq: record.seq,
        value: { bits, known },
      });
  }
  return { members, initial, changes };
}

export function buildBuses(rows: Row[], trace: Trace): Map<string, Bus> {
  const signals = new Map(
    trace.signals.map((signal) => [signalKey(signal), signal]),
  );
  return new Map(
    rows
      .filter((row) => row.bus && row.enabled)
      .map((row) => [row.id, buildBus(row.bus!, signals)]),
  );
}

export function formatBus(value: BusValue, settings: BusSettings): string {
  const width = settings.members.length;
  const mask = (1n << BigInt(width)) - 1n;
  const bits =
    (settings.inverted ? value.bits ^ mask : value.bits) & value.known & mask;
  const radix =
    settings.radix === 10 && (value.known & mask) !== mask ? 2 : settings.radix;
  if (radix === 10)
    return (settings.signed ? BigInt.asIntN(width, bits) : bits).toString(10);
  const size = radix === 16 ? 4 : radix === 8 ? 3 : 1;
  let result = "";
  for (let offset = 0; offset < width; offset += size) {
    const digitMask =
      ((1n << BigInt(Math.min(size, width - offset))) - 1n) << BigInt(offset);
    result =
      ((value.known & digitMask) === digitMask
        ? ((bits & digitMask) >> BigInt(offset)).toString(radix).toUpperCase()
        : "x") + result;
  }
  return (radix === 16 ? "0x" : radix === 8 ? "0o" : "0b") + result;
}

export function numericBus(
  value: BusValue,
  settings: BusSettings,
): bigint | null {
  const width = settings.members.length;
  const mask = (1n << BigInt(width)) - 1n;
  if ((value.known & mask) !== mask) return null;
  const bits = (settings.inverted ? value.bits ^ mask : value.bits) & mask;
  return settings.radix === 10 && settings.signed
    ? BigInt.asIntN(width, bits)
    : bits;
}

export function nextBusEdge(
  bus: Bus,
  tick: number,
  direction: -1 | 1,
): number | undefined {
  return direction === 1
    ? bus.changes.find((change) => change.tick > tick)?.tick
    : bus.changes
        .slice()
        .reverse()
        .find((change) => change.tick < tick)?.tick;
}

export function busPreview(
  bus: Bus,
  settings: BusSettings,
  tick: number,
): string {
  const changes = bus.changes.filter((change) => change.tick === tick);
  const values =
    changes.length > 1
      ? "\nChanges: " +
        changes.map((change) => formatBus(change.value, settings)).join(" → ")
      : "";
  return formatBus(valueAt(bus, tick), settings) + values;
}
