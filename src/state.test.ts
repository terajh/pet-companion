import { describe, expect, it } from "vitest";
import type {
  AppPayload,
  OverlaySnapshot,
  PetAnimationState,
  SessionAppKind,
  SessionSummary,
} from "./types";
import {
  MAX_VISIBLE_CARDS,
  RECENT_ACTIVITY_WINDOW_MS,
  dismissDecisionForVisualState,
  pickVisibleSessions,
  sessionVisualState,
  type VisibilityClock,
} from "./state";

const NOW = 1_700_000_000_000;

const fixedClock: VisibilityClock = { now: () => NOW };

function makeSession(overrides: Partial<SessionSummary> = {}): SessionSummary {
  return {
    appKind: "claude",
    assistantPreview: null,
    completedPreview: null,
    completedTurns: 1,
    cwd: "/tmp/project",
    inProgress: false,
    isArchived: false,
    lastActivityAt: NOW - 1_000,
    sessionId: "session-1",
    title: "Test session",
    userPreview: "what is rust?",
    ...overrides,
  };
}

function makeOverlay(overrides: Partial<OverlaySnapshot> = {}): OverlaySnapshot {
  return {
    activeSession: null,
    claudeFrontmost: false,
    codexFrontmost: false,
    currentWindowTitle: null,
    effectiveState: "idle",
    messagePreview: null,
    manualSessionMissing: false,
    manualSessionPinned: false,
    permissionGranted: true,
    pet: {
      description: "",
      displayName: "Bori",
      id: "bori",
      source: "custom",
      spriteSheetPath: "",
    },
    sessions: [],
    showCard: false,
    stateLabel: "Idle",
    dismissedSessionIds: [],
    completedRuntimeSessionIds: [],
    cardsBelow: false,
    ...overrides,
  };
}

function makePayload(overlay: OverlaySnapshot): AppPayload {
  return {
    codexSelectedPetId: null,
    config: {
      attached: true,
      language: "ko",
      trackedApp: "auto",
      manualSessionApp: null,
      manualSessionId: null,
      petOverrideId: null,
      petScale: 1,
    },
    overlay,
    pets: [],
  };
}

describe("pickVisibleSessions — completion regression (the v0.1.30 bug)", () => {
  it("keeps the just-completed runtime card visible after in_progress flips to false", () => {
    // This is the exact scenario the user reported: a session that the
    // backend's prev_in_progress detector observed transition true → false
    // during this run.  It must remain visible so the user can see the
    // "완료" card and dismiss it manually.
    const session = makeSession({
      sessionId: "s-just-completed",
      inProgress: false,
      lastActivityAt: NOW - 2_000,
    });
    const overlay = makeOverlay({
      sessions: [session],
      completedRuntimeSessionIds: ["s-just-completed"],
      dismissedSessionIds: [],
      activeSession: null,
    });

    const visible = pickVisibleSessions(makePayload(overlay), fixedClock);

    expect(visible.map((s) => s.sessionId)).toContain("s-just-completed");
  });

  it("renders the runtime-completed card with the waving (완료) visual state", () => {
    const session = makeSession({
      sessionId: "s-just-completed",
      inProgress: false,
    });
    const overlay = makeOverlay({
      sessions: [session],
      completedRuntimeSessionIds: ["s-just-completed"],
    });

    expect(
      sessionVisualState(session, makePayload(overlay), fixedClock),
    ).toBe<PetAnimationState>("waving");
  });

  it("hides the runtime-completed card after the user dismisses it", () => {
    const session = makeSession({
      sessionId: "s-just-completed",
      inProgress: false,
    });
    const overlay = makeOverlay({
      sessions: [session],
      completedRuntimeSessionIds: ["s-just-completed"],
      dismissedSessionIds: ["s-just-completed"],
    });

    const visible = pickVisibleSessions(makePayload(overlay), fixedClock);

    expect(visible).toHaveLength(0);
  });
});

describe("pickVisibleSessions — active session policy", () => {
  it("always shows the active session even when in_progress is false and not in completedRuntime", () => {
    const session = makeSession({
      sessionId: "s-active",
      inProgress: false,
      lastActivityAt: NOW - 7 * 60 * 60 * 1000, // older than the 6h window
    });
    const overlay = makeOverlay({
      sessions: [session],
      activeSession: session,
      completedRuntimeSessionIds: [],
    });

    const visible = pickVisibleSessions(makePayload(overlay), fixedClock);

    expect(visible.map((s) => s.sessionId)).toEqual(["s-active"]);
  });

  it("hides the active session once it is dismissed", () => {
    const session = makeSession({ sessionId: "s-active" });
    const overlay = makeOverlay({
      sessions: [session],
      activeSession: session,
      dismissedSessionIds: ["s-active"],
    });

    const visible = pickVisibleSessions(makePayload(overlay), fixedClock);

    expect(visible).toHaveLength(0);
  });
});

describe("pickVisibleSessions — in-progress / dismissed precedence", () => {
  it("re-shows a previously dismissed session once in_progress flips back to true", () => {
    // CLAUDE.md policy: only a false→true in_progress transition unblocks a
    // dismissed card.  At the visible-list level this just means an
    // in-progress session always wins over the dismissed flag.
    const session = makeSession({
      sessionId: "s-revived",
      inProgress: true,
    });
    const overlay = makeOverlay({
      sessions: [session],
      // The dismissed_sessions retain pass in the backend would clear this,
      // but defensively the frontend should also surface it.
      dismissedSessionIds: ["s-revived"],
    });

    const visible = pickVisibleSessions(makePayload(overlay), fixedClock);

    expect(visible.map((s) => s.sessionId)).toContain("s-revived");
  });

  it("hides dismissed sessions that are NOT in progress", () => {
    const session = makeSession({
      sessionId: "s-dismissed",
      inProgress: false,
    });
    const overlay = makeOverlay({
      sessions: [session],
      dismissedSessionIds: ["s-dismissed"],
      completedRuntimeSessionIds: ["s-dismissed"],
    });

    expect(pickVisibleSessions(makePayload(overlay), fixedClock)).toHaveLength(0);
  });
});

describe("pickVisibleSessions — archived & recency policy", () => {
  it("never shows archived sessions", () => {
    const session = makeSession({
      sessionId: "s-archived",
      isArchived: true,
      inProgress: true, // even an in-progress archived session is hidden
    });
    const overlay = makeOverlay({ sessions: [session] });

    expect(pickVisibleSessions(makePayload(overlay), fixedClock)).toHaveLength(0);
  });

  it("shows recently-active sessions within RECENT_ACTIVITY_WINDOW_MS", () => {
    const session = makeSession({
      sessionId: "s-recent",
      inProgress: false,
      lastActivityAt: NOW - (RECENT_ACTIVITY_WINDOW_MS - 1_000),
    });
    const overlay = makeOverlay({ sessions: [session] });

    expect(pickVisibleSessions(makePayload(overlay), fixedClock)).toHaveLength(1);
  });

  it("hides sessions older than RECENT_ACTIVITY_WINDOW_MS", () => {
    const session = makeSession({
      sessionId: "s-stale",
      inProgress: false,
      lastActivityAt: NOW - (RECENT_ACTIVITY_WINDOW_MS + 1_000),
    });
    const overlay = makeOverlay({ sessions: [session] });

    expect(pickVisibleSessions(makePayload(overlay), fixedClock)).toHaveLength(0);
  });
});

describe("pickVisibleSessions — ordering & cap", () => {
  it("sorts in-progress sessions before non-in-progress, then by lastActivityAt desc", () => {
    const sessions: SessionSummary[] = [
      makeSession({
        sessionId: "s-old-running",
        inProgress: true,
        lastActivityAt: NOW - 5_000,
      }),
      makeSession({
        sessionId: "s-new-done",
        inProgress: false,
        lastActivityAt: NOW - 1_000,
      }),
      makeSession({
        sessionId: "s-new-running",
        inProgress: true,
        lastActivityAt: NOW - 100,
      }),
    ];
    const overlay = makeOverlay({
      sessions,
      completedRuntimeSessionIds: ["s-new-done"],
    });

    const visible = pickVisibleSessions(makePayload(overlay), fixedClock);

    expect(visible.map((s) => s.sessionId)).toEqual([
      "s-new-running",
      "s-old-running",
      "s-new-done",
    ]);
  });

  it("caps the result at MAX_VISIBLE_CARDS", () => {
    const sessions: SessionSummary[] = Array.from({ length: 10 }, (_, i) =>
      makeSession({
        sessionId: `s-${i}`,
        inProgress: true,
        lastActivityAt: NOW - i * 1_000,
      }),
    );
    const overlay = makeOverlay({ sessions });

    const visible = pickVisibleSessions(makePayload(overlay), fixedClock);

    expect(visible).toHaveLength(MAX_VISIBLE_CARDS);
  });

  it("deduplicates by sessionId", () => {
    const session = makeSession({
      sessionId: "s-dup",
      inProgress: true,
    });
    const overlay = makeOverlay({ sessions: [session, { ...session }] });

    const visible = pickVisibleSessions(makePayload(overlay), fixedClock);

    expect(visible).toHaveLength(1);
  });
});

describe("sessionVisualState", () => {
  it("uses overlay.effectiveState for the active session", () => {
    const session = makeSession({ sessionId: "s-active", inProgress: true });
    const overlay = makeOverlay({
      activeSession: session,
      effectiveState: "running",
    });

    expect(
      sessionVisualState(session, makePayload(overlay), fixedClock),
    ).toBe<PetAnimationState>("running");
  });

  it("returns 'running' for a non-active in-progress session with fresh activity", () => {
    const session = makeSession({
      sessionId: "s-bg",
      inProgress: true,
      lastActivityAt: NOW - 5_000,
    });
    const overlay = makeOverlay({ sessions: [session] });

    expect(
      sessionVisualState(session, makePayload(overlay), fixedClock),
    ).toBe<PetAnimationState>("running");
  });

  it("returns 'waiting' for a non-active in-progress session quiet > 30s", () => {
    const session = makeSession({
      sessionId: "s-bg",
      inProgress: true,
      lastActivityAt: NOW - 60_000,
    });
    const overlay = makeOverlay({ sessions: [session] });

    expect(
      sessionVisualState(session, makePayload(overlay), fixedClock),
    ).toBe<PetAnimationState>("waiting");
  });

  it("returns 'waving' for a runtime-completed non-active session", () => {
    const session = makeSession({
      sessionId: "s-completed",
      inProgress: false,
    });
    const overlay = makeOverlay({
      completedRuntimeSessionIds: ["s-completed"],
    });

    expect(
      sessionVisualState(session, makePayload(overlay), fixedClock),
    ).toBe<PetAnimationState>("waving");
  });

  it("returns 'idle' for a recently-active non-active non-runtime-completed session", () => {
    // This is the "session left over from before the pet app launched" case
    // surfaced by the RECENT_ACTIVITY_WINDOW_MS branch — the user has not
    // chatted in this conversation during the current pet app run.
    const session = makeSession({
      sessionId: "s-old-recent",
      inProgress: false,
    });
    const overlay = makeOverlay({});

    expect(
      sessionVisualState(session, makePayload(overlay), fixedClock),
    ).toBe<PetAnimationState>("idle");
  });
});

describe("dismissDecisionForVisualState — click policy (v0.1.30)", () => {
  const cases: ReadonlyArray<[PetAnimationState, boolean]> = [
    ["running", false],
    ["waiting", false],
    // Everything the user perceives as "done" must dismiss on click:
    ["waving", true],
    ["idle", true],
    ["sleeping", true],
    ["jumping", true],
    ["review", true],
    ["failed", true],
  ];

  for (const [state, expected] of cases) {
    it(`returns ${expected} for visualState='${state}'`, () => {
      expect(dismissDecisionForVisualState(state)).toBe(expected);
    });
  }
});

describe("appKind crossover (Claude + Codex coexistence)", () => {
  it("treats Claude and Codex sessions identically in the visibility filter", () => {
    const claudeSession = makeSession({
      sessionId: "s-claude",
      appKind: "claude" as SessionAppKind,
      inProgress: false,
    });
    const codexSession = makeSession({
      sessionId: "s-codex",
      appKind: "codex" as SessionAppKind,
      inProgress: false,
    });
    const overlay = makeOverlay({
      sessions: [claudeSession, codexSession],
      completedRuntimeSessionIds: ["s-claude", "s-codex"],
    });

    const visible = pickVisibleSessions(makePayload(overlay), fixedClock);

    expect(visible.map((s) => s.sessionId).sort()).toEqual([
      "s-claude",
      "s-codex",
    ]);
  });
});
