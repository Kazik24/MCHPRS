export function parsePlot(text: string): [number, number] | null {
  const match = /^\s*([+-]?\d+)(?:\s*,\s*|\s+)([+-]?\d+)\s*$/.exec(text);
  if (!match) return null;
  const x = Number(match[1]),
    y = Number(match[2]);
  return [x, y].every(
    (value) =>
      Number.isInteger(value) && value >= -2147483648 && value <= 2147483647,
  )
    ? [x, y]
    : null;
}
