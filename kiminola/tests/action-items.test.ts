import assert from "node:assert/strict";
import { test } from "node:test";
// @ts-expect-error Node strip-types imports the production TypeScript module.
import { extractActionItemEntries, extractActionItems, replaceActionItems } from "../src/lib/action-items.ts";

test("extracts action items from the VC pitch ask and next steps section", () => {
  const markdown = [
    "## Summary",
    "Pitch summary.",
    "## Ask / next steps",
    "- Send the data room link",
    "- Schedule the partner meeting",
    "## Risks",
    "- This is not an action item",
  ].join("\n");

  assert.deepEqual(extractActionItems(markdown), [
    "Send the data room link",
    "Schedule the partner meeting",
  ]);
});

test("replaces enhanced-note action items by occurrence without title collisions", () => {
  const markdown = [
    "## Summary",
    "Pitch summary.",
    "## Action items",
    "- [ ] Follow up with the team.",
    "- Send the revised follow-up.",
    "## Risks",
    "- Follow up with the team.",
  ].join("\n");

  assert.equal(replaceActionItems(markdown, [
    { sourceIndex: 0, title: "Send the revised follow-up." },
    { sourceIndex: 1, title: "Prepare the final response." },
  ]), [
    "## Summary",
    "Pitch summary.",
    "## Action items",
    "- [ ] Send the revised follow-up.",
    "- Prepare the final response.",
    "## Risks",
    "- Follow up with the team.",
  ].join("\n"));
});

test("preserves duplicate action-item occurrences for independent editing", () => {
  const markdown = "## Action items\n- Repeat this\n- Repeat this";
  assert.deepEqual(extractActionItemEntries(markdown), [
    { sourceIndex: 0, title: "Repeat this" },
    { sourceIndex: 1, title: "Repeat this" },
  ]);
});
