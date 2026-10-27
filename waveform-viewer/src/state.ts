import type { Trace, Viewport } from "./trace";
import { parseZeroTick } from "./trace.ts";
import type { BusSettings } from "./bus";

export const colors = [
  "#ef7272",
  "#b5cea8",
  "#73c5df",
  "#d7ba7d",
  "#c586c0",
  "#ce9178",
  "#dcdcaa",
  "#9cdcfe",
];
export interface Row {
  id: string;
  kind: "signal" | "group" | "separator";
  signalId?: string;
  label: string;
  enabled: boolean;
  color: string;
  height: number;
  groupId?: string;
  collapsed?: boolean;
  guides: "none" | "rising" | "falling" | "both";
  shortName: boolean;
  bus?: BusSettings;
}
export interface Session {
  zeroTick: string;
  rows: Row[];
  viewport: Viewport;
  marker: number;
  altMarker: number | null;
}

export function initialSession(trace: Trace): Session {
  return {
    zeroTick: "0",
    rows: trace.signals.slice(0, 12).map((signal) => ({
      id: signal.id,
      signalId: signal.id,
      kind: "signal",
      label: signal.name,
      enabled: true,
      color: colors[0],
      height: 38,
      guides: "none",
      shortName: false,
    })),
    viewport:
      trace.tickOrigin === undefined
        ? { start: 320, end: 576 }
        : { start: trace.start, end: trace.end },
    marker: trace.tickOrigin === undefined ? 448 : trace.start,
    altMarker: trace.tickOrigin === undefined ? 490 : null,
  };
}

export function visibleRows(rows: Row[]): Row[] {
  return rows.filter(
    (row) =>
      row.enabled &&
      (!row.groupId ||
        !rows.some(
          (group) =>
            group.id === row.groupId && (group.collapsed || !group.enabled),
        )),
  );
}

export function moveRow(
  rows: Row[],
  from: string,
  to: string,
  after = false,
): Row[] {
  if (from === to) return rows;
  const row = rows.find((r) => r.id === from),
    target = rows.find((r) => r.id === to);
  if (!row || !target) return rows;
  const moving = rows.filter(
    (item) =>
      item.id === from || (row.kind === "group" && item.groupId === from),
  );
  if (moving.some((item) => item.id === to)) return rows;
  const result = rows.filter(
    (item) => !moving.some((moved) => moved.id === item.id),
  );
  if (row.kind === "group") {
    const anchor = target.groupId ?? target.id;
    let index = result.findIndex((item) => item.id === anchor);
    if (after) {
      index++;
      while (result[index]?.groupId === anchor) index++;
    }
    result.splice(index, 0, ...moving);
    return result;
  }
  const index =
    result.findIndex((item) => item.id === to) +
    (target.kind === "group" || after ? 1 : 0);
  result.splice(index, 0, {
    ...row,
    groupId: target.kind === "group" ? target.id : target.groupId,
  });
  return result;
}

export function ungroupRows(rows: Row[], selected: string[]): Row[] {
  const detached = rows.filter(
    (row) => row.groupId && selected.includes(row.id),
  );
  const result = rows.filter((row) => !detached.includes(row));
  for (const row of detached.reverse()) {
    let end = result.length - 1;
    while (
      end >= 0 &&
      result[end].id !== row.groupId &&
      result[end].groupId !== row.groupId
    )
      end--;
    result.splice(end + 1, 0, { ...row, groupId: undefined });
  }
  return result;
}

export function parseSession(text: string, trace: Trace): Session {
  const value = JSON.parse(text);
  const zeroTick = parseZeroTick(value.zeroTick ?? "0");
  if (
    value.version !== 1 ||
    value.traceId !== trace.id ||
    !Array.isArray(value.rows) ||
    value.rows.length > 4096 ||
    zeroTick === null
  )
    throw Error("This session does not match the current capture.");
  const ids = new Set<string>();
  const rows: Row[] = value.rows.map((row: Row) => {
    if (
      !row ||
      typeof row.id !== "string" ||
      ids.has(row.id) ||
      !["signal", "group", "separator"].includes(row.kind) ||
      typeof row.label !== "string" ||
      row.label.length > 128 ||
      typeof row.enabled !== "boolean" ||
      !/^#[0-9a-f]{6}$/i.test(row.color) ||
      ![24, 38, 56, 76].includes(row.height) ||
      !["none", "rising", "falling", "both"].includes(row.guides) ||
      typeof row.shortName !== "boolean" ||
      (row.groupId !== undefined && typeof row.groupId !== "string") ||
      (row.collapsed !== undefined && typeof row.collapsed !== "boolean") ||
      (row.bus !== undefined &&
        (row.kind !== "group" ||
          !row.bus ||
          ![2, 8, 10, 16].includes(row.bus.radix) ||
          typeof row.bus.inverted !== "boolean" ||
          !Array.isArray(row.bus.members) ||
          row.bus.members.length < 1 ||
          row.bus.members.length > 1024 ||
          row.bus.members.some(
            (key) => typeof key !== "string" || key.length > 512,
          ) ||
          new Set(row.bus.members).size !== row.bus.members.length))
    )
      throw Error("Invalid session row.");
    if (
      row.kind === "signal" &&
      !trace.signals.some((signal) => signal.id === row.signalId)
    )
      throw Error("Unknown signal in session.");
    ids.add(row.id);
    return { ...row };
  });
  if (
    rows.some(
      (row) =>
        row.groupId &&
        !rows.some(
          (group) =>
            group.id === row.groupId &&
            group.kind === "group" &&
            !group.groupId,
        ),
    )
  )
    throw Error("Invalid group in session.");
  if (
    !value.viewport ||
    !Number.isFinite(value.viewport.start) ||
    !Number.isFinite(value.viewport.end) ||
    value.viewport.end <= value.viewport.start ||
    value.viewport.start < trace.start ||
    value.viewport.end > trace.end ||
    !Number.isFinite(value.marker) ||
    value.marker < trace.start ||
    value.marker > trace.end ||
    (value.altMarker !== null &&
      (!Number.isFinite(value.altMarker) ||
        value.altMarker < trace.start ||
        value.altMarker > trace.end))
  )
    throw Error("Invalid session time range.");
  return {
    zeroTick,
    rows,
    viewport: value.viewport,
    marker: value.marker,
    altMarker: value.altMarker,
  };
}
