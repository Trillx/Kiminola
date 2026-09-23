import assert from "node:assert/strict";
import { test } from "node:test";
import { readFileSync } from "node:fs";

// @ts-expect-error Node's strip-types test runner imports the TypeScript source directly.
import { canDropNode, createMoveValidation, destinationKey, locationFromSearchParams, moveOptions, recordingHrefForLocation, recordingLocationFromSearchParams } from "../src/lib/library-tree.ts";

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
  const engineering = options.find((option) => destinationKey(option.location) === "space:2");
  const child = options.find((option) => destinationKey(option.location) === "meeting:11");
  assert.equal(engineering?.disabled, false);
  assert.equal(child?.disabled, true);

  const spaceOptions = moveOptions(tree, { kind: "space", id: 2 });
  const root = spaceOptions.find((option) => option.location === null);
  assert.equal(root?.label, "Library root");
  assert.equal(root?.disabled, false);
  assert.equal(
    spaceOptions
      .filter((option) => option.location?.kind === "meeting")
      .every((option) => option.disabled),
    true,
  );
});

test("move options scan the tree linearly even when the source is last", () => {
  let childReads = 0;
  const meetings = Array.from({ length: 400 }, (_, i) => ({
    kind: "meeting" as const, id: i + 1, title: `Meeting ${i + 1}`,
    created_at: "2026-09-21T12:00:00Z", duration_seconds: 60,
    get children() { childReads++; return []; },
  }));
  const library = [{ kind: "space" as const, id: 1, name: "Work", children: meetings }];
  const options = moveOptions(library, { kind: "meeting", id: 400 });
  assert.equal(options.length, 401);
  assert.equal(options.at(-1)?.disabled, true);
  assert.ok(childReads <= 3 * options.length, `expected linear traversal, read children ${childReads} times`);
});

test("dialog and drag targets reuse source descendant membership", () => {
  const validation = createMoveValidation(tree, { kind: "meeting", id: 10 });
  assert.equal(validation.canDrop({ kind: "meeting", id: 10 }), false);
  assert.equal(validation.canDrop({ kind: "meeting", id: 11 }), false);
  assert.equal(validation.canDrop({ kind: "space", id: 1 }), true);
  assert.equal(validation.canDrop(null), false);
  const options = moveOptions(tree, { kind: "meeting", id: 10 }, validation);
  for (const option of options) assert.equal(option.disabled, !validation.canDrop(option.location));

  const nodeSource = readFileSync(new URL("../src/lib/components/LibraryTreeNode.svelte", import.meta.url), "utf8");
  assert.doesNotMatch(nodeSource, /canDropNode\(/, "each target must not search the tree again");
  assert.match(nodeSource, /moveValidation\.canDrop\(location\)/);
});

test("the main Tauri window leaves HTML drag and drop to the frontend", () => {
  const config = JSON.parse(
    readFileSync(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8"),
  );
  assert.equal(
    config.app.windows[0].dragDropEnabled,
    false,
    "WebView2's native file-drop handler blocks HTML drag and drop on Windows",
  );
});

test("missing move sources fail closed and roots cannot move to root", () => {
  assert.equal(createMoveValidation(tree, null).canDrop({ kind: "space", id: 1 }), false);
  assert.equal(createMoveValidation(tree, { kind: "meeting", id: 99 }).canDrop({ kind: "space", id: 1 }), false);
  assert.equal(createMoveValidation(tree, { kind: "space", id: 1 }).canDrop(null), false);
  assert.equal(createMoveValidation(tree, { kind: "space", id: 2 }).canDrop(null), true);
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
  assert.deepEqual(
    recordingLocationFromSearchParams(
      new URLSearchParams(),
      { kind: "meeting", id: 10 },
      { kind: "space", id: 2 },
    ),
    { kind: "space", id: 2 },
  );
  assert.equal(locationFromSearchParams(new URLSearchParams("spaceId=nope")), null);
});
