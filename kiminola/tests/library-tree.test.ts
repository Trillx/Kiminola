import assert from "node:assert/strict";
import { test } from "node:test";

// @ts-expect-error Node's strip-types test runner imports the TypeScript source directly.
import { canDropNode, locationFromSearchParams, moveOptions, nodeKey, recordingHrefForLocation, recordingLocationFromSearchParams } from "../src/lib/library-tree.ts";

const tree = [
  {
    kind: "space" as const,
    id: 1,
    name: "Work",
    children: [
      {
        kind: "meeting" as const,
        id: 10,
        title: "Planning",
        created_at: "2026-08-23T12:00:00Z",
        duration_seconds: 60,
        children: [
          {
            kind: "meeting" as const,
            id: 11,
            title: "Follow-up",
            created_at: "2026-08-23T13:00:00Z",
            duration_seconds: 30,
            children: [],
          },
        ],
      },
      {
        kind: "space" as const,
        id: 2,
        name: "Engineering",
        children: [],
      },
    ],
  },
];

test("validates recursive library drop targets and cycles", () => {
  assert.equal(canDropNode({ kind: "space", id: 2 }, { kind: "meeting", id: 10 }, tree), false);
  assert.equal(canDropNode({ kind: "meeting", id: 10 }, { kind: "meeting", id: 11 }, tree), false);
  assert.equal(canDropNode({ kind: "meeting", id: 11 }, { kind: "space", id: 2 }, tree), true);
  assert.equal(canDropNode({ kind: "space", id: 1 }, { kind: "space", id: 2 }, tree), false);
  assert.equal(canDropNode({ kind: "meeting", id: 11 }, { kind: "meeting", id: 10 }, tree), true);
});

test("move options expose valid containers and disable invalid ones", () => {
  const options = moveOptions(tree, { kind: "meeting", id: 10 });
  const engineering = options.find((option) => nodeKey(option.location) === "space:2");
  const child = options.find((option) => nodeKey(option.location) === "meeting:11");
  assert.equal(engineering?.disabled, false);
  assert.equal(child?.disabled, true);

  const spaceOptions = moveOptions(tree, { kind: "space", id: 2 });
  assert.equal(
    spaceOptions
      .filter((option) => option.location.kind === "meeting")
      .every((option) => option.disabled),
    true,
  );
});

test("recording destinations round-trip through the URL", () => {
  assert.equal(recordingHrefForLocation(null), "/record");
  const meetingHref = recordingHrefForLocation({ kind: "meeting", id: 11 });
  assert.equal(meetingHref, "/record?parentMeetingId=11");
  assert.deepEqual(
    locationFromSearchParams(new URLSearchParams("parentMeetingId=11")),
    { kind: "meeting", id: 11 },
  );
  assert.deepEqual(
    locationFromSearchParams(new URLSearchParams("spaceId=2")),
    { kind: "space", id: 2 },
  );
  assert.deepEqual(
    recordingLocationFromSearchParams(
      new URLSearchParams("spaceId=2"),
      { kind: "meeting", id: 10 },
    ),
    { kind: "space", id: 2 },
  );
  assert.deepEqual(
    recordingLocationFromSearchParams(
      new URLSearchParams(),
      { kind: "meeting", id: 10 },
    ),
    { kind: "meeting", id: 10 },
  );
  assert.equal(locationFromSearchParams(new URLSearchParams("spaceId=nope")), null);
});
