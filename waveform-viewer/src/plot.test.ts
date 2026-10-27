import assert from "node:assert/strict";
import { test } from "node:test";
import { parsePlot } from "./plot.ts";

test("plot field accepts space or comma separated signed coordinates", () => {
  for (const text of ["3 -2", "3,-2", "3, -2", " 3 , -2 ", "+3   -2", "3\t-2"])
    assert.deepEqual(parsePlot(text), [3, -2], text);
  assert.deepEqual(
    parsePlot("-2147483648,2147483647"),
    [-2147483648, 2147483647],
  );
  for (const text of [
    "",
    "3",
    "3,",
    "3 2 1",
    "3,,2",
    "3.2 1",
    "1e2 3",
    "0x10 2",
    "2147483648 0",
    "0 -2147483649",
    "3,foo",
  ])
    assert.equal(parsePlot(text), null, text);
});
