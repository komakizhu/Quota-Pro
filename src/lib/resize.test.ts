import { describe, expect, it } from "vitest";
import { clampResizeSize, getResizeEdge, resizeDelta, resizeSizeFromPointer } from "./resize";

const rect = { left: 100, top: 200, right: 400, bottom: 500 };

describe("resize helpers", () => {
  it("detects all edges and corners with corner priority", () => {
    expect(getResizeEdge(101, 201, rect)).toBe("nw");
    expect(getResizeEdge(399, 201, rect)).toBe("ne");
    expect(getResizeEdge(101, 499, rect)).toBe("sw");
    expect(getResizeEdge(399, 499, rect)).toBe("se");
    expect(getResizeEdge(250, 201, rect)).toBe("n");
    expect(getResizeEdge(250, 499, rect)).toBe("s");
    expect(getResizeEdge(101, 350, rect)).toBe("w");
    expect(getResizeEdge(399, 350, rect)).toBe("e");
    expect(getResizeEdge(250, 350, rect)).toBeNull();
  });

  it("keeps square dimensions while fixing the opposite edge", () => {
    expect(resizeSizeFromPointer(72, "e", 20, 0, { min: 48, max: 144 })).toBe(92);
    expect(resizeSizeFromPointer(72, "w", -20, 0, { min: 48, max: 144 })).toBe(92);
    expect(resizeSizeFromPointer(72, "n", 0, -20, { min: 48, max: 144 })).toBe(92);
    expect(resizeSizeFromPointer(72, "se", 20, 20, { min: 48, max: 144 })).toBe(100);
    expect(resizeDelta("nw", -20, -20)).toBeCloseTo(Math.sqrt(800));
  });

  it("clamps compact and expanded ranges", () => {
    expect(clampResizeSize(10, { min: 48, max: 144 })).toBe(48);
    expect(clampResizeSize(900, { min: 220, max: 460 })).toBe(460);
  });
});
