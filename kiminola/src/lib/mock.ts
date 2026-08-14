/**
 * Mock data for the app shell. Mirrors the SQLite schema in SPEC.md §6
 * (meetings / transcript_segments / notes / spaces) so these shapes can be
 * swapped for Tauri `invoke` calls without touching the components.
 */

export type Channel = "you" | "others";

export interface TranscriptLine {
  channel: Channel;
  text: string;
}

export interface Meeting {
  id: string;
  title: string;
  /** Display string, e.g. "Today · 8 min". */
  meta: string;
  spaceName: string;
  /** Raw notepad markdown captured during the meeting. */
  notepad: string;
  transcript: TranscriptLine[];
  /** Mock AI-enhanced notes (markdown), produced by the "Enhance notes" button. */
  enhancedMarkdown: string;
}

export interface SpaceMeetingRef {
  id: string;
  title: string;
}

export interface Space {
  id: string;
  name: string;
  icon: string;
  meetings: SpaceMeetingRef[];
}

/** Result of a just-finished recording session (pre-persistence). */
export interface RecordedMeeting {
  title: string;
  durationText: string;
  notepad: string;
  transcript: TranscriptLine[];
}

export const meetings: Record<string, Meeting> = {
  "product-standup": {
    id: "product-standup",
    title: "Product standup",
    meta: "Today · 8 min",
    spaceName: "Personal",
    notepad: "DevOps API creds — follow up",
    transcript: [
      { channel: "others", text: "So the blocker is really just the API credentials." },
      { channel: "you", text: "I'll follow up with DevOps today and get those rotated." },
      { channel: "others", text: "Great, then we can merge the branch this afternoon." },
    ],
    enhancedMarkdown: [
      "## Summary",
      "",
      "The team is blocked on API credential rotation. Once resolved, the feature branch can merge this afternoon.",
      "",
      "## Action items",
      "",
      "- You: follow up with DevOps on API credentials today.",
      "- Team: merge branch after credentials rotate.",
      "",
      "## From your notepad",
      "",
      "DevOps API creds — follow up",
    ].join("\n"),
  },
  "1-1-with-sarah": {
    id: "1-1-with-sarah",
    title: "1:1 with Sarah",
    meta: "Yesterday · 24 min",
    spaceName: "Personal",
    notepad: "Onboarding refactor — review props Friday",
    transcript: [
      { channel: "others", text: "I think the onboarding flow is starting to confuse new users." },
      { channel: "you", text: "What if we simplified the first-run experience to one screen?" },
      {
        channel: "others",
        text: "I like that. I'll draft a proposal by Friday and we can review it next week.",
      },
      { channel: "you", text: "Sounds good — ping me when it's ready." },
    ],
    enhancedMarkdown: [
      "## Summary",
      "",
      "Sarah is considering a refactor of the onboarding flow. She will draft a proposal by Friday and review it with you next week.",
      "",
      "## Action items",
      "",
      "- Sarah: draft onboarding refactor proposal by Friday.",
      "- You: review proposal and schedule follow-up by Monday.",
      "",
      "## From your notepad",
      "",
      "Onboarding refactor — review props Friday",
    ].join("\n"),
  },
  "engineering-sync": {
    id: "engineering-sync",
    title: "Engineering sync",
    meta: "Aug 10 · 41 min",
    spaceName: "Work",
    notepad: "Observability migration — checklist + rollback",
    transcript: [
      { channel: "others", text: "Our current logging is making incident response too slow." },
      { channel: "you", text: "The new observability stack would give us tracing out of the box." },
      {
        channel: "others",
        text: "Let's commit to the migration next sprint, but only with a rollback plan in place.",
      },
      { channel: "you", text: "Agreed. I'll write the checklist and we'll review it before kickoff." },
    ],
    enhancedMarkdown: [
      "## Summary",
      "",
      "The team agreed to adopt the new observability stack. Migration work starts next sprint, with a rollback plan required before go-live.",
      "",
      "## Action items",
      "",
      "- You: prepare observability migration checklist.",
      "- Team: define rollback plan before next sprint review.",
      "",
      "## From your notepad",
      "",
      "Observability migration — checklist + rollback",
    ].join("\n"),
  },
};

export const recentMeetings: { id: string; title: string; meta: string; time: string }[] = [
  { id: "product-standup", title: "Product standup", meta: "Me", time: "Today" },
  { id: "1-1-with-sarah", title: "1:1 with Sarah", meta: "Me", time: "Yesterday" },
  { id: "engineering-sync", title: "Engineering sync", meta: "Me", time: "Aug 10" },
];

export const spaces: Space[] = [
  {
    id: "personal",
    name: "Personal",
    icon: "📁",
    meetings: [
      { id: "product-standup", title: "Product standup" },
      { id: "1-1-with-sarah", title: "1:1 with Sarah" },
    ],
  },
  {
    id: "work",
    name: "Work",
    icon: "🏢",
    meetings: [{ id: "engineering-sync", title: "Engineering sync" }],
  },
];

export interface LiveCue {
  channel: Channel;
  text: string;
  /** Milliseconds after recording start. */
  delay: number;
}

/** Mock streaming transcript for the recording screen. */
export const liveSimulation: LiveCue[] = [
  { channel: "others", text: "So the blocker is really just the API credentials.", delay: 1200 },
  { channel: "you", text: "I'll follow up with DevOps today and get those rotated.", delay: 3200 },
  { channel: "others", text: "Great, then we can merge the branch this afternoon.", delay: 5400 },
  { channel: "you", text: "I'll also update the runbook so this doesn't happen again.", delay: 8000 },
];

/** Build a Meeting view-model from a just-finished recording session. */
export function meetingFromRecording(r: RecordedMeeting): Meeting {
  const notepad = r.notepad.trim() || "No notes captured during this meeting.";
  const transcript = r.transcript.length
    ? r.transcript
    : liveSimulation.map((c) => ({ channel: c.channel, text: c.text }));
  const enhancedMarkdown = [
    "## Summary",
    "",
    "API credential rotation is the main blocker; merge is expected this afternoon once resolved.",
    "",
    "## Action items",
    "",
    "- You: follow up with DevOps on API credentials today.",
    "- Team: merge branch after credentials rotate.",
    ...(r.notepad.trim() ? [`- From your notepad: ${r.notepad.trim()}`] : []),
    "",
    "## From your notepad",
    "",
    notepad,
  ].join("\n");
  return {
    id: "latest",
    title: r.title,
    meta: `Just now · ${r.durationText}`,
    spaceName: "Personal",
    notepad,
    transcript,
    enhancedMarkdown,
  };
}
