import { describe, expect, it } from "vitest";
import { normalizeGlassStyle } from "./glass";

describe("normalizeGlassStyle", () => {
  it.each([
    [{ glassBlur: "light" }, "transparent"],
    [{ glassBlur: "medium" }, "dock"],
    [{ glassBlur: "heavy" }, "dock"],
    [{ glassBlur: "unknown" }, "dock"],
    [{}, "dock"],
  ] as const)("migrates %o to %s", (value, expected) => {
    expect(normalizeGlassStyle(value)).toBe(expected);
  });

  it.each(["transparent", "dock", "liquid"] as const)("preserves %s", (glassStyle) => {
    expect(normalizeGlassStyle({ glassStyle, glassBlur: "light" })).toBe(glassStyle);
  });

  it("does not let an obsolete field rescue an invalid new value", () => {
    expect(normalizeGlassStyle({ glassStyle: "unknown", glassBlur: "light" })).toBe("dock");
  });
});
