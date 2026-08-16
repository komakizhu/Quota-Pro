import { describe, expect, it } from "vitest";
import { clampResizeSize, COMPACT_SIZE_RANGE, getOrbResizeHitSizes, getResizeEdge, RESIZE_CORNER_HIT_SIZE, resizeDelta, resizeHasMoved, resizePointerDelta, resizeSizeFromPointer } from "./resize";

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

  it("scales the orb resize hit areas while keeping corner priority", () => {
    expect(getOrbResizeHitSizes(48)).toEqual({ corner: 14, edge: 2 });
    expect(getOrbResizeHitSizes(72)).toEqual({ corner: 18, edge: 3 });
    expect(getOrbResizeHitSizes(144)).toEqual({ corner: 22, edge: 5 });

    const orb = { left: 0, top: 0, right: 72, bottom: 72 };
    const { corner, edge } = getOrbResizeHitSizes(72);
    expect(getResizeEdge(17, 17, orb, edge, corner)).toBe("nw");
    expect(getResizeEdge(3, 36, orb, edge, corner)).toBe("w");
    expect(getResizeEdge(4, 36, orb, edge, corner)).toBeNull();
  });

  it("keeps square dimensions while fixing the opposite edge", () => {
    expect(resizeSizeFromPointer(72, "e", 20, 0, { min: 48, max: 144 })).toBe(92);
    expect(resizeSizeFromPointer(72, "w", -20, 0, { min: 48, max: 144 })).toBe(92);
    expect(resizeSizeFromPointer(72, "n", 0, -20, { min: 48, max: 144 })).toBe(92);
    expect(resizeSizeFromPointer(72, "se", 20, 20, { min: 48, max: 144 })).toBe(92);
    expect(resizeDelta("nw", -20, -20)).toBe(20);
    expect(resizeDelta("se", 30, 10)).toBe(20);
    expect(resizeDelta("nw", -30, -10)).toBe(20);
  });

  it.each([
    ["ne", 30, -10],
    ["nw", -30, -10],
    ["se", 30, 10],
    ["sw", -30, 10],
  ] as const)("uses nearest diagonal projection for %s corner motion", (edge, deltaX, deltaY) => {
    expect(resizeDelta(edge, deltaX, deltaY)).toBe(20);
  });

  it("clamps compact and expanded ranges", () => {
    expect(clampResizeSize(10, { min: 48, max: 144 })).toBe(48);
    expect(clampResizeSize(900, { min: 220, max: 460 })).toBe(460);
  });

  it("requires movement before committing an edge gesture", () => {
    expect(resizeHasMoved(10, 10, 14, 13)).toBe(false);
    expect(resizeHasMoved(10, 10, 16, 13)).toBe(true);
  });

  it("keeps west and north resize sizes stable when window movement changes client coordinates", () => {
    const startClient = { x: 10, y: 10 };
    const startScreen = { screenX: 110, screenY: 210 };
    // The pointer moves 10 screen pixels north-west, then remains stationary
    // while west/north window resizing alternately shifts the content origin.
    const feedbackSamples = [
      { clientX: 0, clientY: 0, screenX: 100, screenY: 200 },
      { clientX: 10, clientY: 10, screenX: 100, screenY: 200 },
      { clientX: 0, clientY: 0, screenX: 100, screenY: 200 },
      { clientX: 10, clientY: 10, screenX: 100, screenY: 200 },
    ];
    const oldWestSizes = feedbackSamples.map((sample) => resizeSizeFromPointer(72, "w", sample.clientX - startClient.x, sample.clientY - startClient.y, COMPACT_SIZE_RANGE));
    const oldNorthSizes = feedbackSamples.map((sample) => resizeSizeFromPointer(72, "n", sample.clientX - startClient.x, sample.clientY - startClient.y, COMPACT_SIZE_RANGE));
    const screenWestSizes = feedbackSamples.map((sample) => {
      const delta = resizePointerDelta(startScreen, sample);
      return resizeSizeFromPointer(72, "w", delta.x, delta.y, COMPACT_SIZE_RANGE);
    });
    const screenNorthSizes = feedbackSamples.map((sample) => {
      const delta = resizePointerDelta(startScreen, sample);
      return resizeSizeFromPointer(72, "n", delta.x, delta.y, COMPACT_SIZE_RANGE);
    });

    expect(oldWestSizes).toEqual([82, 72, 82, 72]);
    expect(oldNorthSizes).toEqual([82, 72, 82, 72]);
    expect(screenWestSizes).toEqual([82, 82, 82, 82]);
    expect(screenNorthSizes).toEqual([82, 82, 82, 82]);
  });
});
