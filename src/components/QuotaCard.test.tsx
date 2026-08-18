// @vitest-environment jsdom
import { fireEvent, render, screen, cleanup } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ProviderSnapshot, WidgetPreferences } from "../types";
import { orbCornerRadiusForSize, widgetScaleForSize } from "../lib/render";
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

const weeklyPrimarySnapshot: ProviderSnapshot = {
  ...snapshot,
  shortWindow: null,
  weeklyWindow: { remainingPercent: 43, resetsAt: null, windowSeconds: 604_800 },
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
  autoCheckUpdates: true,
  language: "en",
  appearance: "light",
  selectedSkin: "default",
  glassStyle: "dock",
  customSkins: [],
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

const resizeGestures = [
  ["west", "w", { x: 2, y: 36, deltaX: -10, deltaY: 0 }],
  ["north", "n", { x: 36, y: 2, deltaX: 0, deltaY: -10 }],
  ["northwest", "nw", { x: 17, y: 17, deltaX: -10, deltaY: -10 }],
  ["east", "e", { x: 70, y: 36, deltaX: 10, deltaY: 0 }],
  ["south", "s", { x: 36, y: 70, deltaX: 0, deltaY: 10 }],
  ["southeast", "se", { x: 55, y: 55, deltaX: 10, deltaY: 10 }],
] as const;

afterEach(() => {
  vi.useRealTimers();
  cleanup();
});

describe("QuotaOrb drag and click gestures", () => {
  it("applies the Glass skin without changing the orb geometry", () => {
    render(<QuotaOrb snapshot={snapshot} onDrag={vi.fn()} onExpand={vi.fn()} skin="glass" glassStyle="liquid" nativeGlass />);
    const orb = screen.getByRole("button");
    expect(orb.className).toContain("quota-orb--skin-glass");
    expect(orb.className).toContain("quota-orb--glass-liquid");
    expect(orb.className).toContain("quota-orb--native-glass");
    expect(orb.style.getPropertyValue("--orb-corner-radius")).toBe(`${orbCornerRadiusForSize(72, 1)}px`);
  });
  it.each(resizeGestures)("keeps %s resize monotonic and preserves its edge priority", async (_name, edge, gesture) => {
    vi.useFakeTimers();
    const onResizeStart = vi.fn().mockResolvedValue(undefined);
    const onResizePreview = vi.fn();
    render(
      <QuotaOrb
        snapshot={snapshot}
        onDrag={vi.fn()}
        onExpand={vi.fn()}
        onResizeStart={onResizeStart}
        onResizePreview={onResizePreview}
      />,
    );
    const orb = screen.getByRole("button");
    vi.spyOn(orb, "getBoundingClientRect").mockReturnValue({
      x: 0, y: 0, top: 0, left: 0, right: 72, bottom: 72, width: 72, height: 72, toJSON: () => ({}),
    });

    fireEvent.mouseDown(orb, { button: 0, clientX: gesture.x, clientY: gesture.y, screenX: 110, screenY: 210 });
    await Promise.resolve();
    fireEvent.mouseMove(window, { clientX: gesture.x + gesture.deltaX, clientY: gesture.y + gesture.deltaY, screenX: 110 + gesture.deltaX, screenY: 210 + gesture.deltaY });
    fireEvent.mouseMove(window, { clientX: gesture.x, clientY: gesture.y, screenX: 110 + gesture.deltaX, screenY: 210 + gesture.deltaY });
    fireEvent.mouseMove(window, { clientX: gesture.x + gesture.deltaX, clientY: gesture.y + gesture.deltaY, screenX: 110 + gesture.deltaX, screenY: 210 + gesture.deltaY });

    expect(onResizeStart).toHaveBeenCalledWith(edge);
    expect(onResizePreview).not.toHaveBeenCalled();
    vi.advanceTimersToNextFrame();
    expect(onResizePreview.mock.calls.map(([size]) => size)).toEqual([82]);
  });

  it("does not expand after a drag has been held before release", () => {
    const { orb, onDrag, onExpand } = renderOrb();

    fireEvent.mouseDown(orb, { button: 0, clientX: 36, clientY: 36 });
    fireEvent.mouseMove(window, { clientX: 50, clientY: 50, screenX: 50, screenY: 50 });
    fireEvent.mouseUp(window, { clientX: 50, clientY: 50 });
    fireEvent.click(orb);

    expect(onDrag).toHaveBeenCalledTimes(1);
    expect(onExpand).not.toHaveBeenCalled();
  });

  it("keeps the release click suppressed when native drag replays mousedown", () => {
    const { orb, onExpand } = renderOrb();

    fireEvent.mouseDown(orb, { button: 0, clientX: 36, clientY: 36 });
    fireEvent.mouseMove(window, { clientX: 50, clientY: 50, screenX: 50, screenY: 50 });
    // WebKit can send a second mousedown as the native drag hands control back
    // to the WebView. It must not reset the guard before the matching click.
    fireEvent.mouseDown(orb, { button: 0, clientX: 50, clientY: 50 });
    fireEvent.mouseMove(window, { clientX: 80, clientY: 80, screenX: 80, screenY: 80 });
    fireEvent.click(orb);

    expect(onExpand).not.toHaveBeenCalled();
  });

  it("allows a fresh click after the drag release guard expires", async () => {
    vi.useFakeTimers();
    try {
      const { orb, onExpand } = renderOrb();

      fireEvent.mouseDown(orb, { button: 0, clientX: 36, clientY: 36 });
      fireEvent.mouseMove(window, { clientX: 50, clientY: 50, screenX: 50, screenY: 50 });
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

  it("allows another drag immediately after the previous drag settles", async () => {
    const { orb, onDrag } = renderOrb();

    fireEvent.mouseDown(orb, { button: 0, clientX: 36, clientY: 36 });
    fireEvent.mouseMove(window, { clientX: 50, clientY: 50, screenX: 50, screenY: 50 });
    fireEvent.mouseUp(window, { clientX: 50, clientY: 50 });
    await Promise.resolve();
    fireEvent.mouseDown(orb, { button: 0, clientX: 36, clientY: 36 });
    fireEvent.mouseMove(window, { clientX: 50, clientY: 50, screenX: 50, screenY: 50 });

    expect(onDrag).toHaveBeenCalledTimes(2);
  });

  it("cleans an orphaned native-drag listener before starting the next drag", async () => {
    let resolveFirstDrag!: () => void;
    const firstDrag = new Promise<void>((resolve) => { resolveFirstDrag = resolve; });
    const onDrag = vi.fn()
      .mockReturnValueOnce(firstDrag)
      .mockResolvedValueOnce(undefined);
    render(
      <QuotaOrb snapshot={snapshot} onDrag={onDrag} onExpand={vi.fn()} />,
    );
    const orb = screen.getByRole("button");
    vi.spyOn(orb, "getBoundingClientRect").mockReturnValue({
      x: 0, y: 0, top: 0, left: 0, right: 72, bottom: 72, width: 72, height: 72, toJSON: () => ({}),
    });

    fireEvent.mouseDown(orb, { button: 0, clientX: 36, clientY: 36 });
    fireEvent.mouseMove(window, { clientX: 50, clientY: 50, screenX: 50, screenY: 50 });
    resolveFirstDrag(); // Native drag settles but the platform omits mouseup.
    await Promise.resolve();
    await Promise.resolve();

    const removeListener = vi.spyOn(window, "removeEventListener");
    try {
      fireEvent.mouseDown(orb, { button: 0, clientX: 36, clientY: 36 });

      expect(removeListener).toHaveBeenCalledWith("mouseup", expect.any(Function));
      fireEvent.mouseMove(window, { clientX: 50, clientY: 50, screenX: 50, screenY: 50 });
      expect(onDrag).toHaveBeenCalledTimes(2);
    } finally {
      removeListener.mockRestore();
    }
  });

  it("starts a move near a non-corner orb edge", () => {
    const { orb, onDrag } = renderOrb();

    fireEvent.mouseDown(orb, { button: 0, clientX: 4, clientY: 36 });
    fireEvent.mouseMove(window, { clientX: 18, clientY: 50, screenX: 18, screenY: 50 });

    expect(onDrag).toHaveBeenCalledTimes(1);
  });

  it("starts a resize from an orb corner", async () => {
    const onDrag = vi.fn();
    const onResizeStart = vi.fn().mockResolvedValue(undefined);
    render(
      <QuotaOrb
        snapshot={snapshot}
        onDrag={onDrag}
        onExpand={vi.fn()}
        onResizeStart={onResizeStart}
      />,
    );
    const orb = screen.getByRole("button");
    vi.spyOn(orb, "getBoundingClientRect").mockReturnValue({
      x: 0, y: 0, top: 0, left: 0, right: 72, bottom: 72, width: 72, height: 72, toJSON: () => ({}),
    });

    fireEvent.mouseDown(orb, { button: 0, clientX: 17, clientY: 17 });
    await Promise.resolve();

    expect(onResizeStart).toHaveBeenCalledWith("nw");
    expect(onDrag).not.toHaveBeenCalled();
  });

  it("keeps the orb corner radius synchronized during a live resize preview", async () => {
    vi.useFakeTimers();
    const onResizeStart = vi.fn().mockResolvedValue(undefined);
    render(
      <QuotaOrb
        snapshot={snapshot}
        onDrag={vi.fn()}
        onExpand={vi.fn()}
        onResizeStart={onResizeStart}
        onResizePreview={vi.fn()}
      />,
    );
    const orb = screen.getByRole("button");
    vi.spyOn(orb, "getBoundingClientRect").mockReturnValue({
      x: 0, y: 0, top: 0, left: 0, right: 72, bottom: 72, width: 72, height: 72, toJSON: () => ({}),
    });

    fireEvent.mouseDown(orb, { button: 0, clientX: 55, clientY: 55, screenX: 110, screenY: 210 });
    await Promise.resolve();
    fireEvent.mouseMove(window, { clientX: 65, clientY: 65, screenX: 120, screenY: 220 });
    vi.advanceTimersToNextFrame();

    const previewSize = 82;
    expect(orb.style.getPropertyValue("--widget-scale")).toBe(String(widgetScaleForSize(previewSize, 72, 1)));
    expect(orb.style.getPropertyValue("--orb-corner-radius")).toBe(`${orbCornerRadiusForSize(previewSize, 1)}px`);

    // A React render during the gesture must not restore the old inline size.
    fireEvent.mouseEnter(orb);
    expect(orb.style.getPropertyValue("--widget-scale")).toBe(String(widgetScaleForSize(previewSize, 72, 1)));
    expect(orb.style.getPropertyValue("--orb-corner-radius")).toBe(`${orbCornerRadiusForSize(previewSize, 1)}px`);
  });

  it("does not restore the old visual size while the native commit is pending", async () => {
    vi.useFakeTimers();
    let resolveCommit!: () => void;
    const onResizeCommit = vi.fn(() => new Promise<void>((resolve) => { resolveCommit = resolve; }));
    const view = render(
      <QuotaOrb
        snapshot={snapshot}
        onDrag={vi.fn()}
        onExpand={vi.fn()}
        onResizeStart={vi.fn().mockResolvedValue(undefined)}
        onResizePreview={vi.fn()}
        onResizeCommit={onResizeCommit}
        resizeSize={72}
      />,
    );
    const orb = screen.getByRole("button");
    vi.spyOn(orb, "getBoundingClientRect").mockReturnValue({
      x: 0, y: 0, top: 0, left: 0, right: 72, bottom: 72, width: 72, height: 72, toJSON: () => ({}),
    });

    fireEvent.mouseDown(orb, { button: 0, clientX: 55, clientY: 55, screenX: 110, screenY: 210 });
    await Promise.resolve();
    fireEvent.mouseMove(window, { clientX: 65, clientY: 65, screenX: 120, screenY: 220 });
    vi.advanceTimersToNextFrame();
    fireEvent.mouseUp(window, { clientX: 65, clientY: 65, screenX: 120, screenY: 220 });
    await Promise.resolve();

    // Simulate a parent render with the old preference while native commit is
    // still in flight. The direct preview must remain visible.
    view.rerender(
      <QuotaOrb
        snapshot={snapshot}
        onDrag={vi.fn()}
        onExpand={vi.fn()}
        onResizeStart={vi.fn().mockResolvedValue(undefined)}
        onResizePreview={vi.fn()}
        onResizeCommit={onResizeCommit}
        resizeSize={72}
      />,
    );
    expect(orb.style.getPropertyValue("--widget-scale")).toBe(String(widgetScaleForSize(82, 72, 1)));

    resolveCommit();
    await Promise.resolve();
    await Promise.resolve();
    expect(onResizeCommit).toHaveBeenCalledWith(82);
    expect(orb.style.getPropertyValue("--orb-corner-radius")).toBe(`${orbCornerRadiusForSize(82, 1)}px`);
  });

  it("allows an immediate center click after a resize edge gesture is cancelled", async () => {
    const onExpand = vi.fn();
    render(
      <QuotaOrb
        snapshot={snapshot}
        onDrag={vi.fn()}
        onExpand={onExpand}
        onResizeStart={vi.fn().mockResolvedValue(undefined)}
        onResizeCancel={vi.fn().mockResolvedValue(undefined)}
      />,
    );
    const orb = screen.getByRole("button");
    vi.spyOn(orb, "getBoundingClientRect").mockReturnValue({
      x: 0, y: 0, top: 0, left: 0, right: 72, bottom: 72, width: 72, height: 72, toJSON: () => ({}),
    });

    fireEvent.mouseDown(orb, { button: 0, clientX: 2, clientY: 36 });
    fireEvent.mouseUp(window, { clientX: 2, clientY: 36 });
    await Promise.resolve();
    await Promise.resolve();
    fireEvent.click(orb); // The cancelled edge gesture's own click is suppressed.
    fireEvent.mouseDown(orb, { button: 0, clientX: 36, clientY: 36 });
    fireEvent.mouseUp(window, { clientX: 36, clientY: 36 });
    fireEvent.click(orb);

    expect(onExpand).toHaveBeenCalledTimes(1);
  });

  it("allows an immediate center click when starting a resize fails", async () => {
    const onExpand = vi.fn();
    render(
      <QuotaOrb
        snapshot={snapshot}
        onDrag={vi.fn()}
        onExpand={onExpand}
        onResizeStart={vi.fn().mockRejectedValue(new Error("resize failed"))}
      />,
    );
    const orb = screen.getByRole("button");
    vi.spyOn(orb, "getBoundingClientRect").mockReturnValue({
      x: 0, y: 0, top: 0, left: 0, right: 72, bottom: 72, width: 72, height: 72, toJSON: () => ({}),
    });

    fireEvent.mouseDown(orb, { button: 0, clientX: 2, clientY: 36 });
    await Promise.resolve();
    await Promise.resolve();
    fireEvent.click(orb); // The failed edge gesture's own click is suppressed.
    fireEvent.mouseDown(orb, { button: 0, clientX: 36, clientY: 36 });
    fireEvent.mouseUp(window, { clientX: 36, clientY: 36 });
    fireEvent.click(orb);

    expect(onExpand).toHaveBeenCalledTimes(1);
  });

  it("resets the orb size when its edge is double-clicked", () => {
    const onResizeReset = vi.fn().mockResolvedValue(undefined);
    render(
      <QuotaOrb
        snapshot={snapshot}
        onDrag={vi.fn()}
        onExpand={vi.fn()}
        onResizeReset={onResizeReset}
      />,
    );
    const orb = screen.getByRole("button");
    vi.spyOn(orb, "getBoundingClientRect").mockReturnValue({
      x: 0, y: 0, top: 0, left: 0, right: 72, bottom: 72, width: 72, height: 72, toJSON: () => ({}),
    });

    fireEvent.doubleClick(orb, { clientX: 2, clientY: 36 });

    expect(onResizeReset).toHaveBeenCalledTimes(1);
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

it("applies the Glass skin to the card without changing its shape classes", () => {
  render(
    <QuotaCard
      snapshot={snapshot}
      preferences={preferences}
      providerCount={1}
      onPrevious={vi.fn()}
      onNext={vi.fn()}
      onTogglePin={vi.fn()}
      onLock={vi.fn()}
      onCollapse={vi.fn()}
      toggleCorner="nw"
      onDrag={vi.fn()}
      skin="glass"
      glassStyle="transparent"
    />,
  );
  const card = screen.getByRole("main");
  expect(card.className).toContain("quota-card--skin-glass");
  expect(card.className).toContain("quota-card--glass-transparent");
  expect(card.className).toContain("quota-card--toggle-nw");
  expect(card.style.borderRadius).toBe("");
});

describe("QuotaCard resize gestures", () => {
  it("renders the quota forecast directly below the reset time", () => {
    render(
      <QuotaCard
        snapshot={{ ...snapshot, shortWindow: { ...snapshot.shortWindow!, resetsAt: "2026-08-20T12:00:00Z" } }}
        preferences={preferences}
        providerCount={1}
        onPrevious={vi.fn()}
        onNext={vi.fn()}
        onTogglePin={vi.fn()}
        onLock={vi.fn()}
        onCollapse={vi.fn()}
        toggleCorner="nw"
        onDrag={vi.fn()}
        prediction={{ historyDays: 7, averageDailyUsagePercent: 10, daysAtAverage: 4.2, daysUntilReset: 3, recommendedDailyPercent: 16.7 }}
      />,
    );
    const resetTime = screen.getByText(/resets in|Reset time unknown/);
    const forecast = screen.getByLabelText("Quota forecast");
    expect(resetTime.nextElementSibling).toBe(forecast);
    expect(forecast.textContent).toContain("Remaining quota can last 4.2 days");
    expect(forecast.textContent).toContain("Recommended daily usage: 16.7%");
    expect(screen.getByText("4.2").className).toContain("quota-forecast-value");
    expect(screen.getByText("16.7").className).toContain("quota-forecast-value");
  });

  it("starts moving from the card header instead of treating the contents wrapper as a resize box", () => {
    const onDrag = vi.fn();
    render(
      <QuotaCard
        snapshot={snapshot}
        preferences={preferences}
        providerCount={1}
        onPrevious={vi.fn()}
        onNext={vi.fn()}
        onTogglePin={vi.fn()}
        onLock={vi.fn()}
        onCollapse={vi.fn()}
        toggleCorner="nw"
        onDrag={onDrag}
      />,
    );
    const card = screen.getByRole("main");
    vi.spyOn(card, "getBoundingClientRect").mockReturnValue({
      x: 0, y: 0, top: 0, left: 0, right: 306, bottom: 306, width: 306, height: 306, toJSON: () => ({}),
    });
    const header = card.querySelector(".card-header");
    expect(header).not.toBeNull();

    fireEvent.mouseDown(header!, { button: 0, clientX: 153, clientY: 48, screenX: 153, screenY: 48 });

    expect(onDrag).toHaveBeenCalledTimes(1);
  });

  it("keeps the card content at its native layout scale during a live preview", async () => {
    const onResizeStart = vi.fn().mockResolvedValue(undefined);
    render(
      <QuotaCard
        snapshot={snapshot}
        preferences={preferences}
        providerCount={1}
        onPrevious={vi.fn()}
        onNext={vi.fn()}
        onTogglePin={vi.fn()}
        onLock={vi.fn()}
        onCollapse={vi.fn()}
        toggleCorner="nw"
        onDrag={vi.fn()}
        onResizeStart={onResizeStart}
      />,
    );
    const card = screen.getByRole("main");
    vi.spyOn(card, "getBoundingClientRect").mockReturnValue({
      x: 0, y: 0, top: 0, left: 0, right: 306, bottom: 306, width: 306, height: 306, toJSON: () => ({}),
    });

    fireEvent.mouseDown(card, { button: 0, clientX: 304, clientY: 153, screenX: 100, screenY: 200 });
    await Promise.resolve();

    expect(onResizeStart).toHaveBeenCalledWith("e");
    expect(card.className).not.toContain("is-resizing");

    fireEvent.mouseMove(window, { clientX: 314, clientY: 153, screenX: 110, screenY: 200 });
    expect(card.className).toContain("is-resizing");
    expect((card.querySelector(".quota-card-content") as HTMLElement | null)?.style.transform).toBe("");
  });

  it("keeps the card preview scale after a parent render during commit", async () => {
    vi.useFakeTimers();
    let resolveCommit!: () => void;
    const onResizeCommit = vi.fn(() => new Promise<void>((resolve) => { resolveCommit = resolve; }));
    const view = render(
      <QuotaCard
        snapshot={snapshot}
        preferences={preferences}
        providerCount={1}
        onPrevious={vi.fn()}
        onNext={vi.fn()}
        onTogglePin={vi.fn()}
        onLock={vi.fn()}
        onCollapse={vi.fn()}
        toggleCorner="nw"
        onDrag={vi.fn()}
        onResizeStart={vi.fn().mockResolvedValue(undefined)}
        onResizePreview={vi.fn()}
        onResizeCommit={onResizeCommit}
        resizeSize={306}
      />,
    );
    const card = screen.getByRole("main");
    vi.spyOn(card, "getBoundingClientRect").mockReturnValue({
      x: 0, y: 0, top: 0, left: 0, right: 306, bottom: 306, width: 306, height: 306, toJSON: () => ({}),
    });

    fireEvent.mouseDown(card, { button: 0, clientX: 304, clientY: 153, screenX: 100, screenY: 200 });
    await Promise.resolve();
    fireEvent.mouseMove(window, { clientX: 314, clientY: 153, screenX: 110, screenY: 200 });
    vi.advanceTimersToNextFrame();
    fireEvent.mouseUp(window, { clientX: 314, clientY: 153, screenX: 110, screenY: 200 });
    await Promise.resolve();

    view.rerender(
      <QuotaCard
        snapshot={snapshot}
        preferences={preferences}
        providerCount={1}
        onPrevious={vi.fn()}
        onNext={vi.fn()}
        onTogglePin={vi.fn()}
        onLock={vi.fn()}
        onCollapse={vi.fn()}
        toggleCorner="nw"
        onDrag={vi.fn()}
        onResizeStart={vi.fn().mockResolvedValue(undefined)}
        onResizePreview={vi.fn()}
        onResizeCommit={onResizeCommit}
        resizeSize={306}
      />,
    );
    expect(card.style.getPropertyValue("--widget-scale")).toBe(String(widgetScaleForSize(316, 306, 1)));

    resolveCommit();
    await Promise.resolve();
    await Promise.resolve();
    expect(onResizeCommit).toHaveBeenCalledWith(316);
  });

  it("ignores a second resize mousedown while the native begin is pending", async () => {
    let resolveStart!: () => void;
    const onResizeStart = vi.fn(() => new Promise<void>((resolve) => { resolveStart = resolve; }));
    const onResizeCancel = vi.fn().mockResolvedValue(undefined);
    render(
      <QuotaCard
        snapshot={snapshot}
        preferences={preferences}
        providerCount={1}
        onPrevious={vi.fn()}
        onNext={vi.fn()}
        onTogglePin={vi.fn()}
        onLock={vi.fn()}
        onCollapse={vi.fn()}
        toggleCorner="nw"
        onDrag={vi.fn()}
        onResizeStart={onResizeStart}
        onResizeCancel={onResizeCancel}
      />,
    );
    const card = screen.getByRole("main");
    vi.spyOn(card, "getBoundingClientRect").mockReturnValue({
      x: 0, y: 0, top: 0, left: 0, right: 306, bottom: 306, width: 306, height: 306, toJSON: () => ({}),
    });
    const addListener = vi.spyOn(window, "addEventListener");
    const removeListener = vi.spyOn(window, "removeEventListener");

    fireEvent.mouseDown(card, { button: 0, clientX: 2, clientY: 153, screenX: 100, screenY: 200 });
    fireEvent.mouseDown(card, { button: 0, clientX: 304, clientY: 153, screenX: 100, screenY: 200 });

    expect(onResizeStart).toHaveBeenCalledExactlyOnceWith("w");
    expect(addListener.mock.calls.filter(([type]) => type === "mousemove")).toHaveLength(1);

    fireEvent.keyDown(window, { key: "Escape" });
    await Promise.resolve();
    expect(onResizeCancel).toHaveBeenCalledTimes(1);
    expect(removeListener.mock.calls.filter(([type]) => type === "mousemove")).toHaveLength(1);

    resolveStart();
    await Promise.resolve();
    await Promise.resolve();
    expect(onResizeStart).toHaveBeenCalledTimes(1);
    expect(onResizeCancel).toHaveBeenCalledTimes(1);
  });

  it.each(resizeGestures)("keeps %s resize monotonic and preserves its edge priority", async (_name, edge, gesture) => {
    vi.useFakeTimers();
    const onResizeStart = vi.fn().mockResolvedValue(undefined);
    const onResizePreview = vi.fn();
    render(
      <QuotaCard
        snapshot={snapshot}
        preferences={preferences}
        providerCount={1}
        onPrevious={vi.fn()}
        onNext={vi.fn()}
        onTogglePin={vi.fn()}
        onLock={vi.fn()}
        onCollapse={vi.fn()}
        toggleCorner="nw"
        onDrag={vi.fn()}
        onResizeStart={onResizeStart}
        onResizePreview={onResizePreview}
      />,
    );
    const card = screen.getByRole("main");
    vi.spyOn(card, "getBoundingClientRect").mockReturnValue({
      x: 0, y: 0, top: 0, left: 0, right: 306, bottom: 306, width: 306, height: 306, toJSON: () => ({}),
    });
    const cardHit = {
      w: { x: 2, y: 153 },
      n: { x: 153, y: 2 },
      nw: { x: 17, y: 17 },
      e: { x: 304, y: 153 },
      s: { x: 153, y: 304 },
      se: { x: 289, y: 289 },
    }[edge];
    const { x, y } = cardHit;
    const { deltaX, deltaY } = gesture;

    fireEvent.mouseDown(card, { button: 0, clientX: x, clientY: y, screenX: 110, screenY: 210 });
    await Promise.resolve();
    fireEvent.mouseMove(window, { clientX: x + deltaX, clientY: y + deltaY, screenX: 110 + gesture.deltaX, screenY: 210 + gesture.deltaY });
    fireEvent.mouseMove(window, { clientX: x, clientY: y, screenX: 110 + gesture.deltaX, screenY: 210 + gesture.deltaY });
    fireEvent.mouseMove(window, { clientX: x + deltaX, clientY: y + deltaY, screenX: 110 + gesture.deltaX, screenY: 210 + gesture.deltaY });

    expect(onResizeStart).toHaveBeenCalledWith(edge);
    expect(onResizePreview).not.toHaveBeenCalled();
    vi.advanceTimersToNextFrame();
    expect(onResizePreview.mock.calls.map(([size]) => size)).toEqual([316]);
  });
});

it("marks only the southwest weekly-primary layout for bottom-text alignment", () => {
  const onCollapse = vi.fn();
  render(
    <QuotaCard
      snapshot={weeklyPrimarySnapshot}
      preferences={{ ...preferences, toggleCorner: "sw" }}
      providerCount={1}
      onPrevious={vi.fn()}
      onNext={vi.fn()}
      onTogglePin={vi.fn()}
      onLock={vi.fn()}
      onCollapse={onCollapse}
      toggleCorner="sw"
      onDrag={vi.fn()}
    />,
  );
  const southwestCard = screen.getByRole("main");
  expect(southwestCard.className).toContain("quota-card--toggle-sw");
  expect(southwestCard.className).toContain("quota-card--toggle-sw-weekly-primary");
  const southwestLabelRow = southwestCard.querySelector(".weekly-label-row");
  expect(southwestLabelRow?.children[0].className).toBe("reset-credit-row");
  expect(southwestLabelRow?.children[1].className).toContain("weekly-note");
  expect(southwestCard.querySelector(".weekly-metric strong")?.textContent).toBe("--");
  fireEvent.click(screen.getByRole("button", { name: "Collapse widget" }));
  expect(onCollapse).toHaveBeenCalledTimes(1);
  cleanup();

  render(
    <QuotaCard
      snapshot={weeklyPrimarySnapshot}
      preferences={{ ...preferences, toggleCorner: "se" }}
      providerCount={1}
      onPrevious={vi.fn()}
      onNext={vi.fn()}
      onTogglePin={vi.fn()}
      onLock={vi.fn()}
      onCollapse={vi.fn()}
      toggleCorner="se"
      onDrag={vi.fn()}
    />,
  );
  const southeastCard = screen.getByRole("main");
  expect(southeastCard.className).not.toContain("quota-card--toggle-sw-weekly-primary");
  const southeastLabelRow = southeastCard.querySelector(".weekly-label-row");
  expect(southeastLabelRow?.children[0].className).toContain("weekly-note");
  expect(southeastLabelRow?.children[1].className).toBe("reset-credit-row");
  cleanup();

  render(
    <QuotaCard
      snapshot={snapshot}
      preferences={{ ...preferences, toggleCorner: "sw" }}
      providerCount={1}
      onPrevious={vi.fn()}
      onNext={vi.fn()}
      onTogglePin={vi.fn()}
      onLock={vi.fn()}
      onCollapse={vi.fn()}
      toggleCorner="sw"
      onDrag={vi.fn()}
    />,
  );
  expect(screen.getByRole("main").className).not.toContain("quota-card--toggle-sw-weekly-primary");
  cleanup();

  render(
    <QuotaCard
      snapshot={weeklyPrimarySnapshot}
      preferences={{ ...preferences, toggleCorner: "sw" }}
      providerCount={1}
      onPrevious={vi.fn()}
      onNext={vi.fn()}
      onTogglePin={vi.fn()}
      onLock={vi.fn()}
      onCollapse={vi.fn()}
      toggleCorner="sw"
      onDrag={vi.fn()}
      skin="computer"
    />,
  );
  const computerCard = screen.getByRole("main");
  expect(computerCard.className).toContain("quota-card--toggle-sw-weekly-primary");
  const computerLabelRow = computerCard.querySelector(".weekly-label-row");
  expect(computerLabelRow?.children[0].className).toBe("reset-credit-row");
  expect(computerLabelRow?.children[1].className).toContain("weekly-note");
  expect(computerCard.querySelector(".computer-gpt-mark")).toBeNull();
});

describe("QuotaCard mode toggle", () => {
  it("renders the collapse control in the active corner without moving it into the header actions", () => {
    const onCollapse = vi.fn();
    const onLock = vi.fn();
    render(
      <QuotaCard
        snapshot={snapshot}
        preferences={preferences}
        providerCount={1}
        onPrevious={vi.fn()}
        onNext={vi.fn()}
        onTogglePin={vi.fn()}
        onLock={onLock}
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
    fireEvent.click(pin);
    expect(onLock).toHaveBeenCalledTimes(1);
  });
});

describe("QuotaCard settings control", () => {
  it.each([
    ["en", "Settings"],
    ["zh-CN", "设置"],
  ] as const)("opens settings in %s without starting a card drag", (language, label) => {
    const onSettings = vi.fn();
    const onDrag = vi.fn();
    render(
      <QuotaCard
        snapshot={snapshot}
        preferences={{ ...preferences, language }}
        providerCount={1}
        onPrevious={vi.fn()}
        onNext={vi.fn()}
        onTogglePin={vi.fn()}
        onLock={vi.fn()}
        onCollapse={vi.fn()}
        onSettings={onSettings}
        toggleCorner="nw"
        onDrag={onDrag}
      />,
    );

    const button = screen.getByRole("button", { name: label });
    expect(button.className).toContain("settings-button");
    fireEvent.mouseDown(button, { button: 0, clientX: 120, clientY: 36 });
    fireEvent.click(button);

    expect(onSettings).toHaveBeenCalledTimes(1);
    expect(onDrag).not.toHaveBeenCalled();
  });

  it("keeps the compact orb free of an extra settings button", () => {
    render(<QuotaOrb snapshot={snapshot} onDrag={vi.fn()} onExpand={vi.fn()} language="en" />);
    expect(screen.queryByRole("button", { name: "Settings" })).toBeNull();
  });
});

it("renders a custom image skin in expanded and compact modes without adding compact controls", () => {
  const customStyle = {
    "--custom-skin-image": "url(data:image/png;base64,LAKE)",
    "--custom-skin-overlay": "rgba(0,0,0,.42)",
    "--custom-text-color": "#FFFFFF",
    "--custom-accent-color": "#123456",
  } as React.CSSProperties;
  render(
    <QuotaCard
      snapshot={snapshot}
      preferences={preferences}
      providerCount={1}
      onPrevious={vi.fn()}
      onNext={vi.fn()}
      onTogglePin={vi.fn()}
      onLock={vi.fn()}
      onCollapse={vi.fn()}
      toggleCorner="nw"
      onDrag={vi.fn()}
      customSkin
      style={customStyle}
    />,
  );
  const card = screen.getByRole("main");
  expect(card.className).toContain("quota-card--skin-custom");
  expect(card.style.getPropertyValue("--custom-accent-color")).toBe("#123456");
  cleanup();

  render(<QuotaOrb snapshot={snapshot} onDrag={vi.fn()} onExpand={vi.fn()} customSkin style={customStyle} />);
  const orb = screen.getByRole("button");
  expect(orb.className).toContain("quota-orb--skin-custom");
  expect(orb.style.getPropertyValue("--custom-skin-overlay")).toBe("rgba(0,0,0,.42)");
  expect(orb.querySelectorAll("button")).toHaveLength(0);
});
