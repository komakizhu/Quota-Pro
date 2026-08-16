// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { createResizePreviewScheduler } from "./resizePreview";

describe("resize preview scheduler", () => {
  afterEach(() => vi.useRealTimers());

  it("coalesces mouse moves in one frame and applies the latest rounded size once", () => {
    vi.useFakeTimers();
    const apply = vi.fn();
    const scheduler = createResizePreviewScheduler(apply);

    scheduler.schedule(72.2);
    scheduler.schedule(72.5);
    scheduler.schedule(72.8);

    expect(apply).not.toHaveBeenCalled();
    vi.advanceTimersToNextFrame();
    expect(apply).toHaveBeenCalledTimes(1);
    expect(apply).toHaveBeenCalledWith(73);
  });

  it("flushes the release size immediately and prevents its scheduled frame from replaying", () => {
    vi.useFakeTimers();
    const apply = vi.fn();
    const scheduler = createResizePreviewScheduler(apply);

    scheduler.schedule(82.2);
    scheduler.flush(82.8);

    expect(apply).toHaveBeenCalledExactlyOnceWith(83);
    vi.advanceTimersToNextFrame();
    expect(apply).toHaveBeenCalledExactlyOnceWith(83);
  });

  it("cancels a pending frame without a delayed preview", () => {
    vi.useFakeTimers();
    const apply = vi.fn();
    const scheduler = createResizePreviewScheduler(apply);

    scheduler.schedule(96.6);
    scheduler.cancel();
    vi.advanceTimersToNextFrame();

    expect(apply).not.toHaveBeenCalled();
  });
});
