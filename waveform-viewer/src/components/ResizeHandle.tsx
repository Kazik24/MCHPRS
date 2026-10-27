import { useRef } from "react";

interface Props {
  label: string;
  value: number;
  min: number;
  max: number;
  onChange: (value: number) => void;
  className?: string;
  style?: React.CSSProperties;
}

export function ResizeHandle({
  label,
  value,
  min,
  max,
  onChange,
  className = "",
  style,
}: Props) {
  const drag = useRef<{ x: number; value: number } | null>(null);
  const resize = (next: number) => onChange(Math.max(min, Math.min(max, next)));
  return (
    <div
      className={`resize-handle ${className}`}
      style={style}
      role="separator"
      aria-label={label}
      aria-orientation="vertical"
      aria-valuenow={value}
      aria-valuemin={min}
      aria-valuemax={max}
      tabIndex={0}
      onPointerDown={(event) => {
        event.preventDefault();
        event.currentTarget.setPointerCapture(event.pointerId);
        drag.current = { x: event.clientX, value };
      }}
      onPointerMove={(event) => {
        if (drag.current)
          resize(drag.current.value + event.clientX - drag.current.x);
      }}
      onPointerUp={() => {
        drag.current = null;
      }}
      onPointerCancel={() => {
        drag.current = null;
      }}
      onKeyDown={(event) => {
        if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
          event.preventDefault();
          event.stopPropagation();
          resize(value + (event.key === "ArrowLeft" ? -10 : 10));
        }
      }}
    />
  );
}
