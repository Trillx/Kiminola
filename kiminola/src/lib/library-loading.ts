import type { MeetingSummary, NoteDraftSummary } from "./tauri";

export interface LibraryLoaders {
  loadMeetings: () => Promise<MeetingSummary[]>;
  loadNoteDrafts: () => Promise<NoteDraftSummary[]>;
}

export interface LibraryLoadResult {
  meetings: MeetingSummary[];
  drafts: NoteDraftSummary[];
  meetingsError: unknown | null;
  draftsError: unknown | null;
}

/**
 * Load independent library resources without allowing an optional resource to
 * hide saved meetings when its own request fails.
 */
export async function loadLibraryData({
  loadMeetings,
  loadNoteDrafts,
}: LibraryLoaders): Promise<LibraryLoadResult> {
  const [meetingsResult, draftsResult] = await Promise.allSettled([
    loadMeetings(),
    loadNoteDrafts(),
  ]);

  return {
    meetings: meetingsResult.status === "fulfilled" ? meetingsResult.value : [],
    drafts: draftsResult.status === "fulfilled" ? draftsResult.value : [],
    meetingsError: meetingsResult.status === "rejected" ? meetingsResult.reason : null,
    draftsError: draftsResult.status === "rejected" ? draftsResult.reason : null,
  };
}
