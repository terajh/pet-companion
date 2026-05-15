// Pure state-derivation helpers extracted from `App.tsx` so they can be unit
// tested without a React or Tauri runtime.
//
// The bug surface this module covers (v0.1.30):
//   • Card-disappearance regression: when a session completes (in_progress
//     true → false) the card must remain visible in a "완료" (waving) visual
//     state, not vanish.
//   • Dismiss-on-click decoupling: the dismiss decision is now driven by the
//     *visual* state the user sees, not by the backend's `in_progress` flag,
//     because Claude's `in_progress = latest_user_at > latest_assistant_at`
//     can stay true after a turn finishes if the .jsonl timestamps quirk.
//
// All time-sensitive branches accept an optional `clock` so tests can pin
// `now()` and reason about RECENT_ACTIVITY_WINDOW_MS boundaries deterministically.

import type {
  AppPayload,
  PetAnimationState,
  SessionAppKind,
  SessionSummary,
} from "./types";

export const MAX_VISIBLE_CARDS = 6;

// Sessions whose latest activity is within this window remain visible even if
// they ended (in_progress=false) before the pet app launched. Matters most
// for Codex: its `task_complete` events flip `in_progress` false the moment
// the run ends, so without a recency window a Codex session the user worked
// on minutes ago would never appear on a card.
//
// 60 min was too short and the cards visibly disappeared during long
// verification sessions ("갑자기 카드가 다 날라가는 현상"). 6 hours spans
// typical day-of usage without keeping yesterday's clutter.
export const RECENT_ACTIVITY_WINDOW_MS = 6 * 60 * 60 * 1000; // 6 hours

// Backend's `last_activity_at` is stored in milliseconds since epoch (see
// `current_time_millis` in `src-tauri/src/lib.rs`).
export interface VisibilityClock {
  now: () => number;
}

const defaultClock: VisibilityClock = { now: Date.now };

/**
 * Determines which session cards the overlay should render.
 *
 * Visibility policy (evaluated in order; first match wins):
 *   1. Archived sessions are never shown.
 *   2. The currently tracked active session is always shown unless it has
 *      been dismissed.
 *   3. In-progress sessions (running / waiting) are always shown.
 *   4. Dismissed sessions are hidden.
 *   5. Runtime-completed sessions (was in_progress=true at some point during
 *      this app run, then transitioned to false) are shown — these are the
 *      "완료" cards the user wants to keep around until they click them.
 *   6. Recently-active sessions (last activity within RECENT_ACTIVITY_WINDOW_MS)
 *      are shown so the user can pop back to a Codex session that completed
 *      before the pet app launched.
 *   7. Everything else is hidden.
 *
 * Tied sessions are sorted by `inProgress` first, then by `lastActivityAt`
 * descending. The result is capped at MAX_VISIBLE_CARDS.
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
  const dismissed = new Set(payload.overlay.dismissedSessionIds);
  const completedRuntime = new Set(payload.overlay.completedRuntimeSessionIds);
  const activeId = payload.overlay.activeSession?.sessionId ?? null;
  const now = clock.now();

  const candidates = payload.overlay.sessions.filter((session) => {
    if (session.isArchived) return false;
    // Per-app watch toggle: unchecked apps disappear entirely from the
    // overlay (even their active session card), so the user can focus on
    // one assistant at a time without losing the other's session data.
    if (!isAppWatched(session.appKind, payload.config)) return false;
    if (session.sessionId === activeId && !dismissed.has(session.sessionId)) {
      return true;
    }
    if (session.inProgress) return true;
    if (dismissed.has(session.sessionId)) return false;
    if (completedRuntime.has(session.sessionId)) return true;
    return now - session.lastActivityAt < RECENT_ACTIVITY_WINDOW_MS;
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
 * The frontend drives this decision off the *visual* state the user sees,
 * not the backend's `in_progress` flag, because Claude's in_progress can
 * stay true after a turn finishes (B-T1-R1 rule: `latest_user_at >
 * latest_assistant_at`).
 *
 *   • running / waiting → user perceives the session as still working →
 *     focus only, do not hide the card.
 *   • everything else (waving / idle / jumping / sleeping / review / failed)
 *     → user perceives the turn as "done" → clicking is their way of saying
 *     "got it, hide it until something new happens here".
 */
export function dismissDecisionForVisualState(state: PetAnimationState): boolean {
  return state !== "running" && state !== "waiting";
}
