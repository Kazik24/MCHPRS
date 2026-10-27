export type ScrollMode = "auto" | "mouse" | "touchpad";
export type Device = Exclude<ScrollMode, "auto">;
interface WheelInput {
  deltaX: number;
  deltaY: number;
  deltaMode: number;
  shiftKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
  wheelDeltaY?: number;
}
export type ScrollMotion =
  | { kind: "rows"; y: number }
  | { kind: "zoom"; factor: number }
  | { kind: "pan"; x: number; y: number };

export function detectDevice(
  event: Pick<WheelInput, "deltaY" | "deltaMode" | "wheelDeltaY">,
): Device {
  if (event.wheelDeltaY !== undefined)
    return event.wheelDeltaY === -3 * event.deltaY ? "touchpad" : "mouse";
  return event.deltaMode === 0 ? "touchpad" : "mouse";
}

export function scrollMotion(
  event: WheelInput,
  mode: ScrollMode,
  detected: Device,
  onLabels: boolean,
  pageHeight: number,
): ScrollMotion {
  const multiplier =
    event.deltaMode === 1 ? 32 : event.deltaMode === 2 ? pageHeight : 1;
  const x = event.deltaX * multiplier,
    y = event.deltaY * multiplier;
  if (onLabels || event.shiftKey)
    return { kind: "rows", y: onLabels ? y : y || x };
  if (event.ctrlKey || event.metaKey) {
    const amount =
      mode === "mouse"
        ? Math.sign(event.deltaY)
        : Math.max(-18, Math.min(18, event.deltaY)) / 18;
    return { kind: "zoom", factor: 2 ** amount };
  }
  const device = mode === "auto" ? detected : mode;
  return device === "touchpad" || x !== 0
    ? { kind: "pan", x, y }
    : { kind: "pan", x: y, y: 0 };
}
