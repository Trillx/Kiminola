import assert from "node:assert/strict";
import { test } from "node:test";
// @ts-expect-error Node strip-types imports the production TypeScript module.
import { extractActionItems } from "../src/lib/action-items.ts";

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
