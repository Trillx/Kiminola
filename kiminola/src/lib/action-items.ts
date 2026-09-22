const ACTION_HEADING = /^#{1,6}\s*(?:action items?|(?:ask\s*\/\s*)?next steps?|follow[- ]?ups?)\s*:?[ \t]*$/i;
const OTHER_HEADING = /^#{1,6}\s+/;
const LIST_ITEM = /^\s*(?:[-*+]|\d+[.)])\s+(?:\[[ xX]\]\s*)?(.+?)\s*$/;

/** Extract user-selectable action items from the enhanced-notes action section. */
export function extractActionItems(markdown: string): string[] {
  const items: string[] = [];
  const seen = new Set<string>();
  let inActionSection = false;

  for (const line of markdown.split(/\r?\n/)) {
    if (ACTION_HEADING.test(line)) {
      inActionSection = true;
      continue;
    }
    if (inActionSection && OTHER_HEADING.test(line)) break;
    if (!inActionSection) continue;

    const match = LIST_ITEM.exec(line);
    if (!match) continue;
    const item = match[1].trim();
    const key = item.toLocaleLowerCase();
    if (!item || seen.has(key)) continue;
    seen.add(key);
    items.push(item);
  }

  return items;
}
