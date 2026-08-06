// @vitest-environment jsdom
import { fireEvent, render, screen, cleanup } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ProviderSnapshot, WidgetPreferences } from "../types";
import { QuotaCard, QuotaOrb } from "./QuotaCard";

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

const preferences: WidgetPreferences = {
  locked: false,
  alwaysOnTop: true,
  widgetMode: "expanded",
  widgetSize: "custom",
  compactSize: 144,
  expandedSize: 460,
  toggleCorner: "nw",
  pinnedProvider: null,
  autoRotateSeconds: 12,
  language: "en",
  appearance: "light",
  license: null,
  licenses: [],
  unlockedSkin: null,
  unlockedSkins: [],
  selectedSkin: "default",
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

  it("keeps the release click suppressed when native drag replays mousedown", () => {
    const { orb, onExpand } = renderOrb();

    fireEvent.mouseDown(orb, { button: 0, clientX: 36, clientY: 36 });
    fireEvent.mouseMove(window, { clientX: 50, clientY: 50 });
    // WebKit can send a second mousedown as the native drag hands control back
    // to the WebView. It must not reset the guard before the matching click.
    fireEvent.mouseDown(orb, { button: 0, clientX: 50, clientY: 50 });
    fireEvent.mouseMove(window, { clientX: 80, clientY: 80 });
    fireEvent.click(orb);

    expect(onExpand).not.toHaveBeenCalled();
  });

  it("allows a fresh click after the drag release guard expires", async () => {
    vi.useFakeTimers();
    try {
      const { orb, onExpand } = renderOrb();

      fireEvent.mouseDown(orb, { button: 0, clientX: 36, clientY: 36 });
      fireEvent.mouseMove(window, { clientX: 50, clientY: 50 });
      fireEvent.mouseUp(window, { clientX: 50, clientY: 50 });
      await Promise.resolve();
      await Promise.resolve();
      vi.setSystemTime(new Date(Date.now() + 351));
      fireEvent.mouseDown(orb, { button: 0, clientX: 36, clientY: 36 });
      fireEvent.mouseUp(window, { clientX: 36, clientY: 36 });
      fireEvent.click(orb);

      expect(onExpand).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("does not expand when a resize edge receives a replayed mousedown", () => {
    const onExpand = vi.fn();
    render(
      <QuotaOrb
        snapshot={snapshot}
        onDrag={vi.fn()}
        onExpand={onExpand}
        onResizeStart={async () => {}}
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

    fireEvent.mouseDown(orb, { button: 0, clientX: 2, clientY: 2 });
    fireEvent.mouseDown(orb, { button: 0, clientX: 2, clientY: 2 });
    fireEvent.click(orb);

    expect(onExpand).not.toHaveBeenCalled();
  });

  it("still expands after a click without movement", () => {
    const { orb, onExpand } = renderOrb();

    fireEvent.mouseDown(orb, { button: 0, clientX: 36, clientY: 36 });
    fireEvent.mouseUp(window, { clientX: 36, clientY: 36 });
    fireEvent.click(orb);

    expect(onExpand).toHaveBeenCalledTimes(1);
  });

  it("keeps the orb content in a separate compositing layer", () => {
    const { orb } = renderOrb();
    expect(orb.querySelector(".orb-content")).not.toBeNull();
    expect(orb.querySelector(".orb-metric")?.parentElement?.className).toBe("orb-content");
  });
});

it("marks every collapse corner for symmetric layout rules", () => {
  for (const corner of ["nw", "ne", "sw", "se"] as const) {
    render(
      <QuotaCard
        snapshot={snapshot}
        preferences={{ ...preferences, toggleCorner: corner }}
        providerCount={1}
        onPrevious={vi.fn()}
        onNext={vi.fn()}
        onTogglePin={vi.fn()}
        onLock={vi.fn()}
        onCollapse={vi.fn()}
        toggleCorner={corner}
        onDrag={vi.fn()}
      />,
    );
    expect(screen.getByRole("main").className).toContain(`quota-card--toggle-${corner}`);
    cleanup();
  }
});

describe("QuotaCard mode toggle", () => {
  it("renders the collapse control in the active corner without moving it into the header actions", () => {
    const onCollapse = vi.fn();
    render(
      <QuotaCard
        snapshot={snapshot}
        preferences={preferences}
        providerCount={1}
        onPrevious={vi.fn()}
        onNext={vi.fn()}
        onTogglePin={vi.fn()}
        onLock={vi.fn()}
        onCollapse={onCollapse}
        toggleCorner="nw"
        onDrag={vi.fn()}
      />,
    );
    const button = screen.getByRole("button", { name: "Collapse widget" });
    const pin = screen.getByRole("button", { name: "Disable always on top" });
    expect(button.className).toContain("widget-toggle--nw");
    expect(pin.className).toContain("pin-button--active");
    expect(button.closest(".quota-card")?.className).toContain("quota-card--toggle-nw");
    expect(screen.getByText("0 reset credits").parentElement?.className).toContain("reset-credit-row");
    expect(screen.queryByRole("img", { name: "GPT" })).toBeNull();
    expect(screen.queryByLabelText("Codex")).toBeNull();
    fireEvent.click(button);
    expect(onCollapse).toHaveBeenCalledTimes(1);
  });
});
