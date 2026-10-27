import assert from "node:assert/strict";
import { test } from "node:test";
import { emptyState, ownerLabel } from "./empty.ts";
import { demoTrace } from "./demo.ts";

test("empty states show brief status and only the commands needed", () => {
  const empty = { ...demoTrace, signals: [], events: [] };
  assert.deepEqual(emptyState(empty, "loading", "unloaded", ""), {
    message: "Plot not loaded. Join it in Minecraft.",
  });
  assert.deepEqual(emptyState(empty, "empty", "ready", ""), {
    message: "Look at a block in Minecraft, then run:",
    commands: ["/waveform add clock", "/waveform on"],
  });
  assert.equal(
    emptyState(empty, "recording", "ready", "")?.message,
    "Waiting for command output.",
  );
  assert.equal(
    emptyState(empty, "empty", "error", "HTTP 503")?.message,
    "HTTP 503",
  );
  assert.deepEqual(
    emptyState(empty, "unsupported_backend", "ready", "")?.commands,
    ["/rp reset", "/waveform on"],
  );
});

test("quiet wires and event-only captures stay visible through transport failures", () => {
  const quiet = {
    ...demoTrace,
    signals: [{ ...demoTrace.signals[0], changes: [] }],
    events: [],
  };
  assert.equal(emptyState(quiet, "recording", "ready", ""), null);
  assert.equal(
    emptyState(quiet, "recording", "error", "Connection lost"),
    null,
  );
  assert.equal(
    emptyState(
      { ...demoTrace, signals: [] },
      "recording",
      "error",
      "Connection lost",
    ),
    null,
  );
  assert.deepEqual(emptyState(quiet, "disabled", "ready", "")?.commands, [
    "/waveform on",
  ]);
});

test("owner labels distinguish unclaimed and unavailable metadata", () => {
  assert.equal(ownerLabel("Maciej"), "Owner: Maciej");
  assert.equal(ownerLabel(null), "Unclaimed plot");
  assert.equal(ownerLabel(undefined), "Owner unavailable");
});
