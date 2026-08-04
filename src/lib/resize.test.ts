import { describe, expect, it } from "vitest";
import { clampResizeSize, getResizeEdge, RESIZE_CORNER_HIT_SIZE, resizeDelta, resizeHasMoved, resizeSizeFromPointer } from "./resize";

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

  it("keeps the diagonal cursor active across the larger corner hit areas", () => {
    const inset = RESIZE_CORNER_HIT_SIZE - 1;
    expect(getResizeEdge(rect.left + inset, rect.top + inset, rect)).toBe("nw");
    expect(getResizeEdge(rect.right - inset, rect.top + inset, rect)).toBe("ne");
    expect(getResizeEdge(rect.left + inset, rect.bottom - inset, rect)).toBe("sw");
    expect(getResizeEdge(rect.right - inset, rect.bottom - inset, rect)).toBe("se");
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

  it("requires movement before committing an edge gesture", () => {
    expect(resizeHasMoved(10, 10, 14, 13)).toBe(false);
    expect(resizeHasMoved(10, 10, 16, 13)).toBe(true);
  });
});
