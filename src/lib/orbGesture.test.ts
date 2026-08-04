import { describe, expect, it } from "vitest";
import { consumeOrbClick, createOrbDragState, recordOrbDrag } from "./orbGesture";

describe("orb drag click guard", () => {
  it("suppresses the click generated when a long drag is released", () => {
    const state = recordOrbDrag(createOrbDragState());
    const consumed = consumeOrbClick(state);
    expect(consumed.suppressed).toBe(true);
    expect(consumeOrbClick(consumed.state).suppressed).toBe(false);
  });
});
