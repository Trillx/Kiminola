const ACTION_HEADING = /^#{1,6}\s*(?:action items?|(?:ask\s*\/\s*)?next steps?|follow[- ]?ups?)\s*:?[ \t]*$/i;
const OTHER_HEADING = /^#{1,6}\s+/;
const LIST_ITEM = /^\s*(?:[-*+]|\d+[.)])\s+(?:\[[ xX]\]\s*)?(.+?)\s*$/;
const LIST_ITEM_PARTS = /^(\s*(?:[-*+]|\d+[.)])\s+(?:\[[ xX]\]\s*)?)(.+?)(\s*)$/;

export interface ActionItemEntry {
  sourceIndex: number;
  title: string;
}

/** Extract every action-item occurrence in document order. */
export function extractActionItemEntries(markdown: string): ActionItemEntry[] {
  const entries: ActionItemEntry[] = [];
  let inActionSection = false;

  for (const rawLine of markdown.split("\n")) {
    const line = rawLine.endsWith(String.fromCharCode(13)) ? rawLine.slice(0, -1) : rawLine;
    if (ACTION_HEADING.test(line)) {
      inActionSection = true;
      continue;
    }
    if (inActionSection && OTHER_HEADING.test(line)) break;
    if (!inActionSection) continue;

    const match = LIST_ITEM.exec(line);
    const title = match?.[1].trim();
    if (!title) continue;
    entries.push({ sourceIndex: entries.length, title });
  }

  return entries;
}

/** Extract user-selectable action items from the enhanced-notes action section. */
export function extractActionItems(markdown: string): string[] {
  const items: string[] = [];
  const seen = new Set<string>();

  for (const { title } of extractActionItemEntries(markdown)) {
    const key = title.toLocaleLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    items.push(title);
  }

  return items;
}

/** Replace action-item occurrences while preserving their original list syntax. */
export function replaceActionItems(
  markdown: string,
  edits: Array<{ sourceIndex: number; title: string }>,
): string {
  const replacements = new Map(edits.map((edit) => [edit.sourceIndex, edit.title.trim()]));
  const firstLineBreak = markdown.indexOf("\n");
  const usesCrLf = firstLineBreak > 0 && markdown.charCodeAt(firstLineBreak - 1) === 13;
  const newline = usesCrLf ? String.fromCharCode(13, 10) : "\n";
  const lines = markdown.split("\n").map((line) =>
    line.endsWith(String.fromCharCode(13)) ? line.slice(0, -1) : line
  );
  let inActionSection = false;
  let sourceIndex = 0;

  for (let index = 0; index < lines.length; index++) {
    const line = lines[index];
    if (ACTION_HEADING.test(line)) {
      inActionSection = true;
      continue;
    }
    if (inActionSection && OTHER_HEADING.test(line)) break;
    if (!inActionSection) continue;

    const match = LIST_ITEM_PARTS.exec(line);
    if (!match) continue;
    const title = replacements.get(sourceIndex);
    if (title) lines[index] = `${match[1]}${title}${match[3]}`;
    sourceIndex++;
  }

  return lines.join(newline);
}
