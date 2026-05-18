import { describe, expect, it } from "vitest";
import { computeShouldExpand } from "./expand";
import type { PetAnimationState } from "./types";

type State = PetAnimationState;

function asMap(entries: Array<[string, State]>): Map<string, State> {
  return new Map(entries);
}

describe("computeShouldExpand", () => {
  it("returns false when prev is empty (first mount seeds without expanding)", () => {
    const prev = new Map<string, State>();
    const curr = asMap([
      ["s1", "waiting"],
      ["s2", "waving"],
    ]);
    expect(computeShouldExpand(prev, curr)).toBe(false);
  });

  it("returns true when a new session enters waiting", () => {
    const prev = asMap([["s1", "running"]]);
    const curr = asMap([
      ["s1", "running"],
      ["s2", "waiting"],
    ]);
    expect(computeShouldExpand(prev, curr)).toBe(true);
  });

  it("returns true when a new session enters waving", () => {
    const prev = asMap([["s1", "running"]]);
    const curr = asMap([
      ["s1", "running"],
      ["s2", "waving"],
    ]);
    expect(computeShouldExpand(prev, curr)).toBe(true);
  });

  it("returns true when an existing session transitions running -> waiting", () => {
    const prev = asMap([["s1", "running"]]);
    const curr = asMap([["s1", "waiting"]]);
    expect(computeShouldExpand(prev, curr)).toBe(true);
  });

  it("returns true when an existing session transitions running -> waving", () => {
    const prev = asMap([["s1", "running"]]);
    const curr = asMap([["s1", "waving"]]);
    expect(computeShouldExpand(prev, curr)).toBe(true);
  });

  it("returns false when state stays the same", () => {
    const prev = asMap([
      ["s1", "waiting"],
      ["s2", "waving"],
    ]);
    const curr = asMap([
      ["s1", "waiting"],
      ["s2", "waving"],
    ]);
    expect(computeShouldExpand(prev, curr)).toBe(false);
  });

  it("returns false when a session leaves waiting/waving (e.g. waving -> idle)", () => {
    const prev = asMap([["s1", "waving"]]);
    const curr = asMap([["s1", "idle"]]);
    expect(computeShouldExpand(prev, curr)).toBe(false);
  });

  it("returns false when waiting persists across ticks (no new transition)", () => {
    const prev = asMap([["s1", "waiting"]]);
    const curr = asMap([["s1", "waiting"]]);
    expect(computeShouldExpand(prev, curr)).toBe(false);
  });

  it("returns true when a session that was waiting goes back to waiting via running -> waiting", () => {
    // Snapshot 1: waiting; Snapshot 2: running; Snapshot 3: waiting.
    // Step 2->3 should expand again because running->waiting is a fresh transition.
    const prev = asMap([["s1", "running"]]);
    const curr = asMap([["s1", "waiting"]]);
    expect(computeShouldExpand(prev, curr)).toBe(true);
  });

  it("returns false when a previously waiting session disappears from curr", () => {
    const prev = asMap([["s1", "waiting"]]);
    const curr = new Map<string, State>();
    expect(computeShouldExpand(prev, curr)).toBe(false);
  });
});
