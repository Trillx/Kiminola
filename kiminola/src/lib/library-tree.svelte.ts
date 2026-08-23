import { browser } from "$app/environment";
import type { LibraryLocation, LibraryNode } from "$lib/tauri";
import { recordingHrefForLocation } from "./library-tree";

export {
  canDropNode,
  locationFromSearchParams,
  recordingLocationFromSearchParams,
  moveOptions,
  nodeKey,
  nodeRef,
  type LibraryDestinationOption,
} from "./library-tree";

const LAST_LOCATION_KEY = "kiminola-last-meeting-location";

function parseLocation(value: string | null): LibraryLocation | null {
  if (!value) return null;
  try {
    const parsed = JSON.parse(value) as Partial<LibraryLocation>;
    if (
      (parsed.kind === "space" || parsed.kind === "meeting") &&
      typeof parsed.id === "number" &&
      Number.isInteger(parsed.id) &&
      parsed.id > 0
    ) {
      return { kind: parsed.kind, id: parsed.id } as LibraryLocation;
    }
  } catch {
    // Stale localStorage should not prevent recording.
  }
  return null;
}

function initialLastLocation(): LibraryLocation | null {
  if (!browser) return null;
  return parseLocation(localStorage.getItem(LAST_LOCATION_KEY));
}

export const libraryDestinationState = $state<{ last: LibraryLocation | null }>({
  last: initialLastLocation(),
});

export function rememberMeetingLocation(location: LibraryLocation) {
  libraryDestinationState.last = location;
  if (browser) localStorage.setItem(LAST_LOCATION_KEY, JSON.stringify(location));
}

export function recordingHref(location: LibraryLocation | null = libraryDestinationState.last): string {
  return recordingHrefForLocation(location);
}
