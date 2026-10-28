import { useEffect, useState } from "react";
import { busPlot } from "../busPlot";
import { formatBus } from "../bus";
import type { Bus, BusSettings } from "../bus";
import { tickLabel, valueAt } from "../trace";
import type { Trace, Viewport } from "../trace";
import { MaterialIcon } from "./MaterialIcon";

interface Props {
  bus: Bus;
  settings: BusSettings;
  label: string;
  color: string;
  trace: Trace;
  viewport: Viewport;
  marker: number;
  zeroTick: string;
  setMarker: (tick: number, alternate: boolean) => void;
  close: () => void;
}

export function BusPlot({
  bus,
  settings,
  label,
  color,
  trace,
  viewport,
  marker,
  zeroTick,
  setMarker,
  close,
}: Props) {
  const [hover, setHover] = useState<number | null>(null);
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (event.key === "Escape") close();
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [close]);
  const plot = busPlot(bus, settings, viewport);
  const axisLabel = (n: bigint | null) =>
    n === null
      ? ""
      : n.toString().length > 16
        ? n.toString().slice(0, 13) + "…"
        : n.toString();
  const pointerTick = (event: React.PointerEvent<SVGSVGElement>) => {
    const rect = event.currentTarget.getBoundingClientRect();
    const x = ((event.clientX - rect.left) / rect.width) * 1000;
    return Math.round(
      viewport.start +
        Math.max(0, Math.min(1, (x - 100) / 880)) *
          (viewport.end - viewport.start),
    );
  };
  const tick = hover ?? marker;
  return (
    <div className="modal-backdrop" onClick={close}>
      <section
        className="help-dialog bus-plot-dialog"
        role="dialog"
        aria-modal="true"
        aria-label={`Plot ${label}`}
        onClick={(event) => event.stopPropagation()}
      >
        <button
          className="dialog-close"
          aria-label="Close bus plot"
          onClick={close}
          autoFocus
        >
          <MaterialIcon name="close" size={16} />
        </button>
        <h2>{label} — bus plot</h2>
        <div className="bus-plot-value">
          Tick {tickLabel(trace, tick, zeroTick)}:{" "}
          {formatBus(valueAt(bus, tick), settings)}
        </div>
        <svg
          viewBox="0 0 1000 300"
          preserveAspectRatio="none"
          role="img"
          aria-label={`Numeric values of ${label} over the visible ticks`}
          onPointerMove={(event) => setHover(pointerTick(event))}
          onPointerLeave={() => setHover(null)}
          onPointerDown={(event) => {
            if (event.button !== 2)
              setMarker(pointerTick(event), event.altKey || event.button === 1);
          }}
        >
          <path d="M100 20 V270 H980" fill="none" stroke="#666" />
          <text x="90" y="24" textAnchor="end">
            <title>{plot.max?.toString()}</title>
            {axisLabel(plot.max)}
          </text>
          <text x="90" y="270" textAnchor="end">
            <title>{plot.min?.toString()}</title>
            {axisLabel(plot.min)}
          </text>
          <text x="100" y="294">
            {tickLabel(trace, viewport.start, zeroTick)}
          </text>
          <text x="980" y="294" textAnchor="end">
            {tickLabel(trace, viewport.end, zeroTick)}
          </text>
          <path
            d={plot.path}
            fill="none"
            stroke={color}
            strokeWidth="2"
            vectorEffect="non-scaling-stroke"
          />
          {marker >= viewport.start && marker <= viewport.end && (
            <line
              x1={
                100 +
                ((marker - viewport.start) / (viewport.end - viewport.start)) *
                  880
              }
              x2={
                100 +
                ((marker - viewport.start) / (viewport.end - viewport.start)) *
                  880
              }
              y1="20"
              y2="270"
              stroke="#aaa"
              strokeDasharray="5 5"
            />
          )}
        </svg>
        {plot.min === null && <p>No known values in the visible range.</p>}
      </section>
    </div>
  );
}
