import { beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({
  calls: [] as string[],
  invoke: vi.fn(async (command: string) => {
    api.calls.push(`start:${command}`);
    await Promise.resolve();
    api.calls.push(`end:${command}`);
  }),
  currentMonitor: vi.fn(async () => ({
    workArea: { position: { x: 0, y: 0 }, size: { width: 1920, height: 1040 } },
  })),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: api.invoke }));
vi.mock("@tauri-apps/api/window", () => ({ currentMonitor: api.currentMonitor }));

beforeEach(() => {
  vi.clearAllMocks();
  api.calls.length = 0;
  vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
});

describe("widget transitions", () => {
  it("passes the requested manual mode and monitor work area to Rust", async () => {
    const { setWidgetMode } = await import("./bridge");
    await setWidgetMode("expanded");
    expect(api.invoke).toHaveBeenCalledWith("set_widget_mode", {
      mode: "expanded",
      workArea: { position: { x: 0, y: 0 }, size: { width: 1920, height: 1040 } },
    });
  });

  it("serializes rapid manual mode changes", async () => {
    const { setWidgetMode } = await import("./bridge");
    await Promise.all([setWidgetMode("expanded"), setWidgetMode("compact")]);
    expect(api.calls).toEqual([
      "start:set_widget_mode",
      "end:set_widget_mode",
      "start:set_widget_mode",
      "end:set_widget_mode",
    ]);
  });

  it("passes the requested widget size and monitor work area to Rust", async () => {
    const { setWidgetSize } = await import("./bridge");
    await setWidgetSize("large");
    expect(api.invoke).toHaveBeenCalledWith("set_widget_size", {
      size: "large",
      workArea: { position: { x: 0, y: 0 }, size: { width: 1920, height: 1040 } },
    });
  });

  it("captures the work area once and keeps preview writes independent of it", async () => {
    const { beginWidgetResize, finishWidgetResize, previewWidgetResize } = await import("./bridge");
    await beginWidgetResize("compact", "se");
    previewWidgetResize(96);
    await finishWidgetResize("compact", 96);
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(api.invoke).toHaveBeenCalledWith("begin_widget_resize", {
      mode: "compact",
      edge: "se",
      workArea: { position: { x: 0, y: 0 }, size: { width: 1920, height: 1040 } },
    });
    expect(api.invoke).toHaveBeenCalledWith("preview_widget_resize", {
      size: 96,
    });
    expect(api.invoke).toHaveBeenCalledWith("finish_widget_resize", {
      size: 96,
    });
    expect(api.currentMonitor).toHaveBeenCalledTimes(1);
  });

  it("resets only the requested widget mode", async () => {
    const { resetWidgetSize } = await import("./bridge");
    await resetWidgetSize("expanded");
    expect(api.invoke).toHaveBeenCalledWith("reset_widget_size", { mode: "expanded" });
  });
});
