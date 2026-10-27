import type { ConnectionState } from "./api.ts";
import type { Trace } from "./trace.ts";

export interface EmptyState {
  message: string;
  commands?: string[];
}

export function emptyState(
  trace: Trace,
  recorder: string,
  connection: ConnectionState,
  message: string,
): EmptyState | null {
  const inactive = ["disabled", "empty", "unsupported_backend"].includes(
    recorder,
  );
  if (
    trace.events?.length ||
    (trace.signals.length && !inactive && trace.id !== "loading")
  )
    return null;
  if (connection === "unloaded")
    return { message: "Plot not loaded. Join it in Minecraft." };
  if (connection === "error") return { message };
  if (connection === "loading") return { message: "Loading…" };
  if (recorder === "recording")
    return { message: "Waiting for command output." };
  if (recorder === "unsupported_backend")
    return {
      message: "Use the interpreter to record.",
      commands: ["/rp reset", "/waveform on"],
    };
  if (recorder === "stopped" || recorder === "overflow")
    return { message: "Recording stopped.", commands: ["/waveform on"] };
  return trace.signals.length
    ? { message: "Recording is off.", commands: ["/waveform on"] }
    : {
        message: "Look at a block in Minecraft, then run:",
        commands: ["/waveform add clock", "/waveform on"],
      };
}

export function ownerLabel(owner: string | null | undefined): string {
  return owner === null
    ? "Unclaimed plot"
    : owner === undefined
      ? "Owner unavailable"
      : `Owner: ${owner}`;
}
