import { useEffect, useRef, useState } from "react";

/** Initial visible string for a freshly observed target. */
export function typewriterInitial(_target: string | null): string {
  return "";
}

/**
 * Returns the next visible string given the previous and the target.
 *
 * Rules:
 *   - If target is null or empty → "".
 *   - If prev is a strict prefix of target with length < target.length → prev + 1 char.
 *   - If prev === target → target (no change).
 *   - Otherwise (text changed mid-typing or prev longer than target) → restart from
 *     the first character of target.
 */
export function nextTypewriterStep(prev: string, target: string | null): string {
  if (target === null || target === "") {
    return "";
  }
  if (prev === target) {
    return target;
  }
  if (
    prev.length < target.length &&
    target.startsWith(prev)
  ) {
    return target.slice(0, prev.length + 1);
  }
  // Text changed; restart from the first character.
  return target.slice(0, 1);
}

/**
 * React hook that returns a "typewriter" version of `text` — characters
 * appear one at a time at `speedMs` per character.  When `text` changes the
 * animation restarts from the first character.
 *
 * Note: when SSR or test environments lack a window we still safely manage
 * the interval via `setInterval` / `clearInterval` which exist in both
 * jsdom and node test runners.
 */
export function useTypewriter(text: string | null, speedMs: number = 25): string {
  const [visible, setVisible] = useState<string>(() => typewriterInitial(text));
  const targetRef = useRef<string | null>(text);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  useEffect(() => {
    targetRef.current = text;
    // Always restart from empty when text changes — keeps the visual behavior
    // identical regardless of whether the new text is a superstring of the
    // current visible (avoids "jump from middle" feel).
    setVisible(typewriterInitial(text));

    if (intervalRef.current !== null) {
      clearInterval(intervalRef.current);
      intervalRef.current = null;
    }

    if (text === null || text === "") {
      return;
    }

    intervalRef.current = setInterval(() => {
      setVisible((prev) => {
        const next = nextTypewriterStep(prev, targetRef.current);
        if (next === prev && intervalRef.current !== null) {
          // Done typing — clean up the interval.
          clearInterval(intervalRef.current);
          intervalRef.current = null;
        }
        return next;
      });
    }, speedMs);

    return () => {
      if (intervalRef.current !== null) {
        clearInterval(intervalRef.current);
        intervalRef.current = null;
      }
    };
  }, [text, speedMs]);

  return visible;
}
