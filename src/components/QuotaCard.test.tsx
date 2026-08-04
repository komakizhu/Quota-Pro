// @vitest-environment jsdom
import { fireEvent, render, screen, cleanup } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ProviderSnapshot } from "../types";
import { QuotaOrb } from "./QuotaCard";

const snapshot: ProviderSnapshot = {
  provider: "codex",
  displayName: "CODEX",
  plan: "PRO",
  shortWindow: { remainingPercent: 69, resetsAt: null, windowSeconds: 18_000 },
  weeklyWindow: null,
  resetCredits: 0,
  updatedAt: new Date().toISOString(),
  status: "ok",
  message: null,
};

function renderOrb() {
  const onDrag = vi.fn();
  const onExpand = vi.fn();
  render(
    <QuotaOrb
      snapshot={snapshot}
      onDrag={onDrag}
      onExpand={onExpand}
    />,
  );
  const orb = screen.getByRole("button");
  vi.spyOn(orb, "getBoundingClientRect").mockReturnValue({
    x: 0,
    y: 0,
    top: 0,
    left: 0,
    right: 72,
    bottom: 72,
    width: 72,
    height: 72,
    toJSON: () => ({}),
  });
  return { orb, onDrag, onExpand };
}

afterEach(() => cleanup());

describe("QuotaOrb drag and click gestures", () => {
  it("does not expand after a drag has been held before release", () => {
    const { orb, onDrag, onExpand } = renderOrb();

    fireEvent.mouseDown(orb, { button: 0, clientX: 36, clientY: 36 });
    fireEvent.mouseMove(window, { clientX: 50, clientY: 50 });
    fireEvent.mouseUp(window, { clientX: 50, clientY: 50 });
    fireEvent.click(orb);

    expect(onDrag).toHaveBeenCalledTimes(1);
    expect(onExpand).not.toHaveBeenCalled();
  });

  it("still expands after a click without movement", () => {
    const { orb, onExpand } = renderOrb();

    fireEvent.mouseDown(orb, { button: 0, clientX: 36, clientY: 36 });
    fireEvent.mouseUp(window, { clientX: 36, clientY: 36 });
    fireEvent.click(orb);

    expect(onExpand).toHaveBeenCalledTimes(1);
  });
});
