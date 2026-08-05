import { describe, expect, it } from "vitest";
import { snapLogicalSizeToDevicePixels, widgetScaleForSize } from "./render";

describe("render sizing", () => {
  it("snaps logical sizes to the physical pixel grid", () => {
    expect(snapLogicalSizeToDevicePixels(451.24, 2)).toBe(451);
    expect(snapLogicalSizeToDevicePixels(451.26, 2)).toBe(451.5);
  });

  it("keeps the scale based on the snapped visual size", () => {
    expect(widgetScaleForSize(451.26, 306, 2)).toBeCloseTo(451.5 / 306);
    expect(widgetScaleForSize(306, 306, 2)).toBe(1);
  });

  it("falls back safely for invalid values", () => {
    expect(snapLogicalSizeToDevicePixels(Number.NaN, 2)).toBe(0);
    expect(widgetScaleForSize(306, 0, 2)).toBe(1);
  });
});
