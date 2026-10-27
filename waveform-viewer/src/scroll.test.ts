import assert from "node:assert/strict";
import { test } from "node:test";
import { detectDevice, scrollMotion } from "./scroll.ts";

const wheel = {
  deltaX: 0,
  deltaY: 90,
  deltaMode: 0,
  shiftKey: false,
  ctrlKey: false,
  metaKey: false,
};
test("mouse pans time, trackpad splits axes, labels and Shift always scroll rows", () => {
  assert.deepEqual(scrollMotion(wheel, "mouse", "mouse", false, 500), {
    kind: "pan",
    x: 90,
    y: 0,
  });
  assert.deepEqual(
    scrollMotion({ ...wheel, deltaX: 40 }, "touchpad", "touchpad", false, 500),
    { kind: "pan", x: 40, y: 90 },
  );
  assert.deepEqual(
    scrollMotion({ ...wheel, deltaX: 40 }, "mouse", "mouse", false, 500),
    { kind: "pan", x: 40, y: 90 },
  );
  assert.deepEqual(
    scrollMotion({ ...wheel, shiftKey: true }, "mouse", "mouse", false, 500),
    { kind: "rows", y: 90 },
  );
  assert.deepEqual(
    scrollMotion({ ...wheel, ctrlKey: true }, "mouse", "mouse", true, 500),
    { kind: "rows", y: 90 },
  );
});
test("explicit mouse zoom uses one octave; auto/trackpad zoom remains continuous", () => {
  assert.deepEqual(
    scrollMotion(
      { ...wheel, deltaY: -2, ctrlKey: true },
      "mouse",
      "mouse",
      false,
      500,
    ),
    { kind: "zoom", factor: 0.5 },
  );
  assert.deepEqual(
    scrollMotion(
      { ...wheel, deltaY: 9, ctrlKey: true },
      "touchpad",
      "touchpad",
      false,
      500,
    ),
    { kind: "zoom", factor: Math.SQRT2 },
  );
  assert.equal(
    detectDevice({ deltaY: 10, deltaMode: 0, wheelDeltaY: -30 }),
    "touchpad",
  );
  assert.equal(
    detectDevice({ deltaY: 100, deltaMode: 0, wheelDeltaY: -120 }),
    "mouse",
  );
  assert.equal(detectDevice({ deltaY: 3, deltaMode: 1 }), "mouse");
});
