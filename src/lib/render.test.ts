import { describe, expect, it } from "vitest";
import { orbCornerRadiusForSize, snapLogicalSizeToDevicePixels, widgetScaleForSize } from "./render";

describe("render sizing", () => {
  it("snaps logical sizes to the physical pixel grid", () => {
    expect(snapLogicalSizeToDevicePixels(451.24, 2)).toBe(451);
    expect(snapLogicalSizeToDevicePixels(451.26, 2)).toBe(451.5);
  });

  it("keeps the scale based on the snapped visual size", () => {
    expect(widgetScaleForSize(451.26, 306, 2)).toBeCloseTo(451.5 / 306);
    expect(widgetScaleForSize(306, 306, 2)).toBe(1);
  });

  it("keeps the default orb radius at 25 percent of its size", () => {
    expect(orbCornerRadiusForSize(72, 1)).toBe(18);
    expect(orbCornerRadiusForSize(48, 2)).toBe(12);
    expect(orbCornerRadiusForSize(144, 2)).toBe(36);
    expect(orbCornerRadiusForSize(51, 2)).toBe(13);
  });

  it("falls back safely for invalid values", () => {
    expect(snapLogicalSizeToDevicePixels(Number.NaN, 2)).toBe(0);
    expect(widgetScaleForSize(306, 0, 2)).toBe(1);
  });
});
