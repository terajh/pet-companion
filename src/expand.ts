import type { PetAnimationState } from "./types";

/**
 * Returns true when at least one session in `curr` represents a NEW transition
 * into `waiting` or `waving` compared to `prev`.  A transition counts as new
 * when:
 *   - the session id did not exist in `prev` and its current state is
 *     `waiting` or `waving`, OR
 *   - the session existed in `prev` but its previous state was neither
 *     `waiting` nor `waving`, and its current state is one of those two.
 *
 * Returns false on the very first observation (when `prev` is empty) to avoid
 * unwanted auto-expand at startup; the caller seeds the ref on first mount.
 */
export function computeShouldExpand(
  prev: Map<string, PetAnimationState>,
  curr: Map<string, PetAnimationState>,
): boolean {
  if (prev.size === 0) {
    return false;
  }
  for (const [sessionId, currState] of curr) {
    if (currState !== "waiting" && currState !== "waving") {
      continue;
    }
    const prevState = prev.get(sessionId);
    if (prevState === currState) {
      continue;
    }
    if (prevState === "waiting" || prevState === "waving") {
      // Was already in an attention state; not a fresh transition.
      continue;
    }
    return true;
  }
  return false;
}
