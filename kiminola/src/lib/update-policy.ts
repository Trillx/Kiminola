export function isRecordingPath(pathname: string): boolean {
  return pathname === "/record";
}

export function updateProgress(downloadedBytes: number, contentLength: number | null): number {
  if (!contentLength || contentLength <= 0) return 0;
  return Math.min(100, Math.max(0, Math.round((downloadedBytes / contentLength) * 100)));
}

export function compactReleaseNotes(notes: string | null, maxLength = 240): string {
  const normalized = notes?.replace(/\s+/g, " ").trim() ?? "";
  if (normalized.length <= maxLength) return normalized;
  return `${normalized.slice(0, Math.max(0, maxLength - 3)).trimEnd()}...`;
}
