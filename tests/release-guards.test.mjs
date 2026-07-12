import assert from "node:assert/strict";
import test from "node:test";
import { parseLocalDate } from "../src/lib/date.ts";
import { isNewSessionRecording } from "../src/lib/releaseGuards.ts";
import { selectNoteFormats } from "../src/lib/noteGeneration.ts";

test("new-session recording classification uses the explicit flag", () => {
  assert.equal(isNewSessionRecording({ isNewSession: true }), true);
  assert.equal(isNewSessionRecording({ isNewSession: false }), false);
  assert.equal(isNewSessionRecording(undefined), false);
});

test("date-only values are parsed as a stable local calendar date", () => {
  const date = parseLocalDate("2026-01-05");
  assert.equal(date.getFullYear(), 2026);
  assert.equal(date.getMonth(), 0);
  assert.equal(date.getDate(), 5);
  assert.equal(date.getHours(), 12);
});

test("adding a note selects only missing requested formats", () => {
  assert.deepEqual(
    selectNoteFormats(["soap"], [], { formats: ["dap", "soap"] }),
    ["dap"],
  );
});

test("regeneration can target one existing note", () => {
  assert.deepEqual(
    selectNoteFormats(["soap", "dap"], [], {
      regenerateExisting: true,
      formats: ["soap"],
    }),
    ["soap"],
  );
});
