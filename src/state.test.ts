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
  dismissDecision,
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

function makePayload(
  overlay: OverlaySnapshot,
  configOverrides: Partial<AppPayload["config"]> = {},
): AppPayload {
  return {
    codexSelectedPetId: null,
    config: {
      attached: true,
      language: "ko",
      trackedApp: "auto",
      petOverrideId: null,
      petScale: 1,
      watchClaude: true,
      watchCodex: true,
      petHidden: false,
      ...configOverrides,
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
  it("hides the active session when it is neither in progress nor runtime-completed", () => {
    const session = makeSession({
      sessionId: "s-active",
      inProgress: false,
      lastActivityAt: NOW - 1_000,
    });
    const overlay = makeOverlay({
      sessions: [session],
      activeSession: session,
      completedRuntimeSessionIds: [],
      effectiveState: "idle",
    });

    const visible = pickVisibleSessions(makePayload(overlay), fixedClock);

    expect(visible).toHaveLength(0);
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
  it("hides dismissed sessions even when in_progress is true", () => {
    const session = makeSession({
      sessionId: "s-revived",
      inProgress: true,
      lastActivityAt: NOW - 1_000,
    });
    const overlay = makeOverlay({
      sessions: [session],
      dismissedSessionIds: ["s-revived"],
    });

    const visible = pickVisibleSessions(makePayload(overlay), fixedClock);

    expect(visible).toHaveLength(0);
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

describe("pickVisibleSessions — archived & visual-state policy", () => {
  it("never shows archived sessions", () => {
    const session = makeSession({
      sessionId: "s-archived",
      isArchived: true,
      inProgress: true, // even an in-progress archived session is hidden
    });
    const overlay = makeOverlay({ sessions: [session] });

    expect(pickVisibleSessions(makePayload(overlay), fixedClock)).toHaveLength(0);
  });

  it("keeps waiting sessions visible while in_progress stays true", () => {
    const session = makeSession({
      sessionId: "s-waiting",
      inProgress: true,
      lastActivityAt: NOW - 31_000,
    });
    const overlay = makeOverlay({ sessions: [session] });

    expect(pickVisibleSessions(makePayload(overlay), fixedClock)).toHaveLength(1);
  });

  it("hides idle sessions even when they were recently active within 6 hours", () => {
    const session = makeSession({
      sessionId: "s-idle",
      inProgress: false,
      lastActivityAt: NOW - 60_000,
    });
    const overlay = makeOverlay({ sessions: [session] });

    expect(pickVisibleSessions(makePayload(overlay), fixedClock)).toHaveLength(0);
  });
});

describe("pickVisibleSessions — ordering & cap", () => {
  it("sorts running sessions before waving sessions, then by lastActivityAt desc", () => {
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

describe("dismissDecision — click policy (v0.1.40)", () => {
  it("returns true for idle cards backed by completedRuntime", () => {
    expect(dismissDecision("idle", new Set(["s-done"]), "s-done")).toBe(true);
  });

  it("returns false for quiet idle cards without completedRuntime membership", () => {
    expect(dismissDecision("idle", new Set<string>(), "s-active")).toBe(false);
  });

  it("returns false for running cards regardless of completedRuntime membership", () => {
    expect(dismissDecision("running", new Set(["s-running"]), "s-running")).toBe(
      false,
    );
  });

  const cases: ReadonlyArray<
    [state: PetAnimationState, completedRuntime: string[], sessionId: string, expected: boolean]
  > = [
    ["running", [], "s-running-no-runtime", false],
    ["waiting", ["s-waiting"], "s-waiting", false],
    ["waiting", [], "s-waiting-no-runtime", false],
    ["waving", ["s-waving"], "s-waving", true],
    ["jumping", ["s-jumping"], "s-jumping", true],
    ["review", ["s-review"], "s-review", true],
    ["failed", ["s-failed"], "s-failed", true],
  ];

  for (const [state, completedRuntime, sessionId, expected] of cases) {
    it(`returns ${expected} for visualState='${state}' with completedRuntime=${completedRuntime.length > 0}`, () => {
      expect(
        dismissDecision(state, new Set(completedRuntime), sessionId),
      ).toBe(expected);
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

describe("pickVisibleSessions — watch toggles per app", () => {
  function makeBothApps() {
    const claude = makeSession({
      sessionId: "s-claude",
      appKind: "claude" as SessionAppKind,
      inProgress: true,
    });
    const codex = makeSession({
      sessionId: "s-codex",
      appKind: "codex" as SessionAppKind,
      inProgress: true,
    });
    return { claude, codex };
  }

  it("hides all Claude sessions when watchClaude=false", () => {
    const { claude, codex } = makeBothApps();
    const overlay = makeOverlay({ sessions: [claude, codex] });

    const visible = pickVisibleSessions(
      makePayload(overlay, { watchClaude: false, watchCodex: true }),
      fixedClock,
    );

    expect(visible.map((s) => s.sessionId)).toEqual(["s-codex"]);
  });

  it("hides all Codex sessions when watchCodex=false", () => {
    const { claude, codex } = makeBothApps();
    const overlay = makeOverlay({ sessions: [claude, codex] });

    const visible = pickVisibleSessions(
      makePayload(overlay, { watchClaude: true, watchCodex: false }),
      fixedClock,
    );

    expect(visible.map((s) => s.sessionId)).toEqual(["s-claude"]);
  });

  it("returns no sessions when both watch toggles are off", () => {
    const { claude, codex } = makeBothApps();
    const overlay = makeOverlay({
      sessions: [claude, codex],
      // Even the active session must hide when its app is unwatched.
      activeSession: claude,
    });

    const visible = pickVisibleSessions(
      makePayload(overlay, { watchClaude: false, watchCodex: false }),
      fixedClock,
    );

    expect(visible).toHaveLength(0);
  });

  it("keeps active in_progress session visible regardless of effectiveState waiting/idle/jumping", () => {
    const states: PetAnimationState[] = ["waiting", "idle", "jumping"];
    for (const effective of states) {
      const session = makeSession({
        sessionId: "s-active",
        inProgress: true,
        lastActivityAt: NOW - 60_000,
      });
      const overlay = makeOverlay({
        sessions: [session],
        activeSession: session,
        effectiveState: effective,
      });
      const payload = makePayload(overlay);
      const visible = pickVisibleSessions(payload, fixedClock);
      expect(visible, `effective=${effective}`).toHaveLength(1);
      expect(visible[0].sessionId).toBe("s-active");
    }
  });

  it("keeps non-active in_progress session visible even if lastActivityAt is old", () => {
    const session = makeSession({
      sessionId: "s-bg",
      inProgress: true,
      lastActivityAt: NOW - 5 * 60_000,
    });
    const overlay = makeOverlay({ sessions: [session] });
    const payload = makePayload(overlay);
    expect(pickVisibleSessions(payload, fixedClock)).toHaveLength(1);
  });

  it("hides non-in_progress session that is not in completedRuntime", () => {
    const session = makeSession({
      sessionId: "s-cold",
      inProgress: false,
      lastActivityAt: NOW - 1_000,
    });
    const overlay = makeOverlay({ sessions: [session] });
    const payload = makePayload(overlay);
    expect(pickVisibleSessions(payload, fixedClock)).toHaveLength(0);
  });

  it("shows non-in_progress session if completedRuntimeSessionIds includes it", () => {
    const session = makeSession({ sessionId: "s-done", inProgress: false });
    const overlay = makeOverlay({
      sessions: [session],
      completedRuntimeSessionIds: ["s-done"],
    });
    const payload = makePayload(overlay);
    expect(pickVisibleSessions(payload, fixedClock)).toHaveLength(1);
  });
});
