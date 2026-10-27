import { formatBus } from "../bus";
import type { Bus, BusSettings, BusValue } from "../bus";
import { valueAt } from "../trace";
import type { Viewport } from "../trace";

export function drawBus(
  c: CanvasRenderingContext2D,
  bus: Bus,
  settings: BusSettings,
  view: Viewport,
  width: number,
  y: number,
  height: number,
  color: string,
) {
  const xFor = (tick: number) =>
    ((tick - view.start) / (view.end - view.start)) * width;
  const high = y + 8,
    low = y + height - 8,
    middle = (high + low) / 2;
  c.save();
  c.beginPath();
  c.rect(0, y, width, height);
  c.clip();
  c.font = "12px Consolas, monospace";
  c.textAlign = "center";
  c.textBaseline = "middle";
  const segment = (start: number, end: number, value: BusValue) => {
    const left = Math.max(0, xFor(start)),
      right = Math.min(width, xFor(end));
    if (right <= left) return;
    const a = start <= view.start ? 0 : Math.min(5, (right - left) / 2);
    const b = end >= view.end ? 0 : Math.min(5, (right - left) / 2);
    c.beginPath();
    c.moveTo(left, middle);
    c.lineTo(left + a, high);
    c.lineTo(right - b, high);
    c.lineTo(right, middle);
    c.lineTo(right - b, low);
    c.lineTo(left + a, low);
    c.closePath();
    c.fillStyle = `${color}60`;
    c.fill();
    c.strokeStyle = color;
    c.stroke();
    const available = right - left - a - b - 8;
    if (available < 16) return;
    let label = formatBus(value, settings);
    if (c.measureText(label).width > available) {
      // Monospaced text allows truncation without measuring every prefix of a 1024-bit value.
      const length = Math.floor(available / c.measureText("0").width) - 1;
      label = label.slice(0, Math.max(0, length)) + "…";
    }
    c.fillStyle = "#f3dada";
    c.fillText(label, (left + right) / 2, middle);
  };
  let previous = view.start,
    value = valueAt(bus, view.start);
  const counts = new Map<number, number>();
  for (const change of bus.changes) {
    if (change.tick >= view.start && change.tick <= view.end)
      counts.set(change.tick, (counts.get(change.tick) ?? 0) + 1);
    if (change.tick <= view.start) continue;
    if (change.tick > view.end) break;
    segment(previous, change.tick, value);
    previous = change.tick;
    value = change.value;
  }
  segment(previous, view.end, value);
  c.fillStyle = color;
  for (const [tick, count] of counts) {
    if (count < 2) continue;
    const x = xFor(tick);
    c.beginPath();
    c.moveTo(x, y + 1);
    c.lineTo(x + 3, y + 4);
    c.lineTo(x, y + 7);
    c.lineTo(x - 3, y + 4);
    c.closePath();
    c.fill();
  }
  c.restore();
}
