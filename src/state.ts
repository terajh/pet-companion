// Pure state-derivation helpers extracted from `App.tsx` so they can be unit
// tested without a React or Tauri runtime.
//
// The bug surface this module covers (v0.1.30):
//   • Card-disappearance regression: when a session completes (in_progress
//     true → false) the card must remain visible in a "완료" (waving) visual
//     state, not vanish.
//   • Dismiss-on-click guard (v0.1.40): the dismiss decision must consider
//     both the visual state and backend authoritative completed-runtime
//     membership so quiet-but-still-active sessions are not hidden.
//   • Card visibility redesign (v0.1.40): card visibility is authoritative
//     `in_progress || completedRuntime`, decoupled from derived visual state.
//
// All time-sensitive branches accept an optional `clock` so tests can pin
// `now()` and reason about visual-state boundaries deterministically.

import type {
  AppPayload,
  PetAnimationState,
  SessionAppKind,
  SessionSummary,
} from "./types";

export const MAX_VISIBLE_CARDS = 6;

// Backend's `last_activity_at` is stored in milliseconds since epoch (see
// `current_time_millis` in `src-tauri/src/lib.rs`).
export interface VisibilityClock {
  now: () => number;
}

const defaultClock: VisibilityClock = { now: Date.now };

/**
 * Determines which session cards the overlay should render.
 *
 * Visibility policy (v0.1.40):
 *   1. Archived / unwatched / dismissed sessions are hidden.
 *   2. Remaining sessions are shown only when `inProgress` is true or the
 *      session id appears in `completedRuntimeSessionIds`.
 *   3. In-progress cards sort above completed-runtime cards; ties sort by
 *      `lastActivityAt` descending.
 *   4. The result is capped at MAX_VISIBLE_CARDS.
 */
function isAppWatched(
  appKind: SessionAppKind,
  config: AppPayload["config"],
): boolean {
  return appKind === "claude" ? config.watchClaude : config.watchCodex;
}

export function pickVisibleSessions(
  payload: AppPayload,
  clock: VisibilityClock = defaultClock,
): SessionSummary[] {
  void clock;
  const dismissed = new Set(payload.overlay.dismissedSessionIds);
  const completedRuntime = new Set(payload.overlay.completedRuntimeSessionIds);

  const candidates = payload.overlay.sessions.filter((session) => {
    if (session.isArchived) return false;
    if (!isAppWatched(session.appKind, payload.config)) return false;
    if (dismissed.has(session.sessionId)) return false;
    if (session.inProgress) return true;
    if (completedRuntime.has(session.sessionId)) return true;
    return false;
  });

  const sorted = [...candidates].sort((a, b) => {
    if (a.inProgress !== b.inProgress) return a.inProgress ? -1 : 1;
    return b.lastActivityAt - a.lastActivityAt;
  });

  const seen = new Set<string>();
  const unique: SessionSummary[] = [];
  for (const session of sorted) {
    if (seen.has(session.sessionId)) continue;
    seen.add(session.sessionId);
    unique.push(session);
    if (unique.length >= MAX_VISIBLE_CARDS) break;
  }
  return unique;
}

/**
 * Computes the per-card animation/visual state.
 *
 *   • Active session → mirrors `payload.overlay.effectiveState` so the card
 *     stays in sync with the backend's authoritative reading.
 *   • Non-active in-progress session → `running` if last activity is fresh,
 *     `waiting` if it has been quiet for more than 30 seconds.
 *   • Runtime-completed session → `waving` (the "완료됨" label).
 *   • Everything else → `idle`.
 */
export function sessionVisualState(
  session: SessionSummary,
  payload: AppPayload,
  clock: VisibilityClock = defaultClock,
): PetAnimationState {
  if (session.sessionId === payload.overlay.activeSession?.sessionId) {
    return payload.overlay.effectiveState;
  }
  if (session.inProgress) {
    const ageMs = clock.now() - session.lastActivityAt;
    return ageMs > 30_000 ? "waiting" : "running";
  }
  if (payload.overlay.completedRuntimeSessionIds.includes(session.sessionId)) {
    return "waving";
  }
  return "idle";
}

/**
 * Decides whether a click on a card should dismiss it.
 *
 * The frontend drives this decision off the visual state the user sees AND
 * the backend's authoritative `completedRuntimeSessionIds` membership.
 *
 *   • running / waiting → user perceives the session as still working →
 *     focus only, do not hide the card.
 *   • completed-runtime + visually done → user perceives the turn as "done"
 *     and the backend agrees this session completed during runtime →
 *     clicking is their way of saying "got it, hide it until something new
 *     happens here".
 *   • visually idle without completed-runtime membership must NOT dismiss:
 *     this covers quiet active sessions whose effectiveState cooled to idle.
 */
export function dismissDecision(
  state: PetAnimationState,
  completedRuntimeSessionIds: ReadonlySet<string>,
  sessionId: string,
): boolean {
  return (
    state !== "running" &&
    state !== "waiting" &&
    completedRuntimeSessionIds.has(sessionId)
  );
}
