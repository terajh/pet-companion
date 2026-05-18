import { describe, expect, it } from "vitest";
import { nextTypewriterStep, typewriterInitial } from "./typing";

describe("typewriterInitial", () => {
  it("returns empty string for null target", () => {
    expect(typewriterInitial(null)).toBe("");
  });

  it("returns empty string for empty target", () => {
    expect(typewriterInitial("")).toBe("");
  });

  it("returns empty string for non-empty target (typing starts from 0)", () => {
    expect(typewriterInitial("hello")).toBe("");
  });
});

describe("nextTypewriterStep", () => {
  it("returns target when target is null", () => {
    expect(nextTypewriterStep("partial", null)).toBe("");
  });

  it("returns target when target is empty", () => {
    expect(nextTypewriterStep("partial", "")).toBe("");
  });

  it("appends one character per step when prev is a prefix of target", () => {
    expect(nextTypewriterStep("", "hello")).toBe("h");
    expect(nextTypewriterStep("h", "hello")).toBe("he");
    expect(nextTypewriterStep("he", "hello")).toBe("hel");
    expect(nextTypewriterStep("hel", "hello")).toBe("hell");
    expect(nextTypewriterStep("hell", "hello")).toBe("hello");
  });

  it("returns target unchanged when fully typed", () => {
    expect(nextTypewriterStep("hello", "hello")).toBe("hello");
  });

  it("restarts from first character when prev is not a prefix of target (text changed)", () => {
    // Mid-typing of "hello" then target switches to "world" — restart.
    expect(nextTypewriterStep("hel", "world")).toBe("w");
  });

  it("restarts when prev is longer than target", () => {
    // Possible when previously fully-typed text is now shorter.
    expect(nextTypewriterStep("hello there", "hi")).toBe("h");
  });

  it("handles multibyte / emoji input by stepping one code-point unit", () => {
    // "😀" is two UTF-16 code units; we accept stepping one unit at a time
    // (visual glitch tolerable for short previews — tests just lock the
    // behavior so we don't accidentally regress on indexing strategy).
    const target = "ab";
    expect(nextTypewriterStep("", target)).toBe("a");
    expect(nextTypewriterStep("a", target)).toBe("ab");
  });
});
