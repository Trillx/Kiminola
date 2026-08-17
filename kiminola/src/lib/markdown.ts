import { marked } from "marked";
import DOMPurify from "isomorphic-dompurify";

marked.setOptions({
  breaks: true,
  gfm: true,
});

/**
 * Render Markdown to sanitized HTML.
 *
 * Supports standard GitHub-flavored Markdown: headings, lists, bold/italic,
 * code blocks, links, etc. Output is sanitized with DOMPurify before being
 * injected into the DOM via {@html ...}.
 */
export function renderMarkdown(md: string): string {
  const raw = marked.parse(md, { async: false }) as string;
  return DOMPurify.sanitize(raw);
}
