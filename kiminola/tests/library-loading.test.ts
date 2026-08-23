import assert from "node:assert/strict";
import { test } from "node:test";
// @ts-expect-error Node's strip-types test runner imports the TypeScript source directly.
import { loadLibraryData } from "../src/lib/library-loading.ts";

test("keeps saved meetings visible when note drafts fail to load", async () => {
  const meeting = {
    id: 42,
    title: "Saved meeting",
    created_at: "2026-08-21T15:00:00Z",
    duration_seconds: 60,
    space_name: "Personal",
    location_path: "Personal",
    parent_meeting_id: null,
  };

  const result = await loadLibraryData({
    loadMeetings: async () => [meeting],
    loadNoteDrafts: async () => {
      throw new Error("drafts unavailable");
    },
  });

  assert.deepEqual(result.meetings, [meeting]);
  assert.equal(result.meetingsError, null);
  assert.match(String(result.draftsError), /drafts unavailable/);
});

test("reports a meeting load failure instead of treating it as an empty library", async () => {
  const result = await loadLibraryData({
    loadMeetings: async () => {
      throw new Error("database unavailable");
    },
    loadNoteDrafts: async () => [],
  });

  assert.deepEqual(result.meetings, []);
  assert.match(String(result.meetingsError), /database unavailable/);
  assert.equal(result.draftsError, null);
});
