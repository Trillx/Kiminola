export interface CompactWindowState {
  compactWindow: boolean;
  compactWindowResizing: boolean;
}

export interface CompactWindowMedia {
  readonly matches: boolean;
  addEventListener(type: "change", listener: () => void): void;
  removeEventListener(type: "change", listener: () => void): void;
}

interface CompactWindowSyncOptions {
  media: CompactWindowMedia;
  initialCompactWindow: boolean;
  requestFrame: (callback: () => void) => number;
  cancelFrame: (handle: number) => void;
  onStateChange: (state: CompactWindowState) => void;
}

export function setupCompactWindowSync({
  media,
  initialCompactWindow,
  requestFrame,
  cancelFrame,
  onStateChange,
}: CompactWindowSyncOptions): () => void {
  let compactWindow = initialCompactWindow;
  let clearResizeFrame: number | undefined;

  const syncCompactWindow = () => {
    const nextCompactWindow = media.matches;
    if (nextCompactWindow === compactWindow) return;

    compactWindow = nextCompactWindow;
    onStateChange({ compactWindow, compactWindowResizing: true });
    if (clearResizeFrame !== undefined) cancelFrame(clearResizeFrame);
    clearResizeFrame = requestFrame(() => {
      clearResizeFrame = requestFrame(() => {
        onStateChange({ compactWindow, compactWindowResizing: false });
        clearResizeFrame = undefined;
      });
    });
  };

  syncCompactWindow();
  media.addEventListener("change", syncCompactWindow);

  return () => {
    media.removeEventListener("change", syncCompactWindow);
    if (clearResizeFrame !== undefined) cancelFrame(clearResizeFrame);
  };
}
