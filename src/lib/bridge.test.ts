import { beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({
  calls: [] as string[],
  eventHandlers: new Map<string, (event: { payload: unknown }) => void>(),
  invoke: vi.fn(async (command: string) => {
    api.calls.push(`start:${command}`);
    await Promise.resolve();
    api.calls.push(`end:${command}`);
  }),
  currentMonitor: vi.fn(async () => ({
    workArea: { position: { x: 0, y: 0 }, size: { width: 1920, height: 1040 } },
  })),
  currentWindow: {
    startDragging: vi.fn(async () => undefined),
    outerPosition: vi.fn(async () => ({ x: 0, y: 0 })),
  },
  listen: vi.fn(async (event: string, handler: (event: { payload: unknown }) => void) => {
    api.eventHandlers.set(event, handler);
    return () => api.eventHandlers.delete(event);
  }),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: api.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: api.listen }));
vi.mock("@tauri-apps/api/window", () => ({ currentMonitor: api.currentMonitor, getCurrentWindow: () => api.currentWindow }));

beforeEach(() => {
  vi.clearAllMocks();
  api.calls.length = 0;
  api.eventHandlers.clear();
  vi.stubGlobal("window", {
    __TAURI_INTERNALS__: {},
    setInterval: globalThis.setInterval,
    clearInterval: globalThis.clearInterval,
    setTimeout: globalThis.setTimeout,
    clearTimeout: globalThis.clearTimeout,
  });
});

describe("widget transitions", () => {
  it("selects a skin through the native normalization command", async () => {
    const { selectSkin } = await import("./bridge");
    await selectSkin("computer");
    expect(api.invoke).toHaveBeenCalledWith("select_skin", { id: "computer" });
  });

  it("passes Glass through the native skin command", async () => {
    const { selectSkin } = await import("./bridge");
    await selectSkin("glass");
    expect(api.invoke).toHaveBeenCalledWith("select_skin", { id: "glass" });
  });

  it("passes the requested manual mode and monitor work area to Rust", async () => {
    const { setWidgetMode } = await import("./bridge");
    await setWidgetMode("expanded");
    expect(api.invoke).toHaveBeenCalledWith("set_widget_mode", {
      mode: "expanded",
      workArea: { position: { x: 0, y: 0 }, size: { width: 1920, height: 1040 } },
      southwestWeeklyPrimary: false,
    });
  });

  it("passes the southwest weekly-primary layout flag when requested", async () => {
    const { setWidgetMode } = await import("./bridge");
    await setWidgetMode("expanded", true);
    expect(api.invoke).toHaveBeenCalledWith("set_widget_mode", {
      mode: "expanded",
      workArea: { position: { x: 0, y: 0 }, size: { width: 1920, height: 1040 } },
      southwestWeeklyPrimary: true,
    });
  });

  it("syncs southwest weekly-primary layout changes without changing preferences", async () => {
    const { syncWidgetLayout } = await import("./bridge");
    await syncWidgetLayout(true);
    expect(api.invoke).toHaveBeenCalledWith("sync_widget_layout", { southwestWeeklyPrimary: true });
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

  it("passes the requested widget size without borrowing another window's work area", async () => {
    const { setWidgetSize } = await import("./bridge");
    await setWidgetSize("large");
    expect(api.invoke).toHaveBeenCalledWith("set_widget_size", {
      size: "large",
    });
  });

  it("applies independent compact and expanded dimensions atomically", async () => {
    const { setWidgetDimensions } = await import("./bridge");
    await setWidgetDimensions(96, 360);
    expect(api.invoke).toHaveBeenCalledWith("set_widget_dimensions", {
      compactSize: 96,
      expandedSize: 360,
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

  it("suppresses duplicate previews and drains the released size before persisting", async () => {
    const { beginWidgetResize, finishWidgetResize, previewWidgetResize } = await import("./bridge");
    await beginWidgetResize("compact", "se");
    vi.clearAllMocks();
    api.calls.length = 0;
    previewWidgetResize(96);
    previewWidgetResize(96);
    await new Promise((resolve) => setTimeout(resolve, 0));
    // The first preview lazily imports the Tauri core module; give that
    // promise and the single-flight drain a second turn before asserting.
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(api.invoke).toHaveBeenCalledTimes(1);
    expect(api.invoke).toHaveBeenCalledWith("preview_widget_resize", { size: 96 });

    await finishWidgetResize("compact", 104);

    expect(api.calls).toEqual([
      "start:preview_widget_resize",
      "end:preview_widget_resize",
      "start:preview_widget_resize",
      "end:preview_widget_resize",
      "start:finish_widget_resize",
      "end:finish_widget_resize",
    ]);
  });

  it("keeps only the newest preview while a native frame is in flight", async () => {
    const { beginWidgetResize, previewWidgetResize } = await import("./bridge");
    await beginWidgetResize("compact", "se");
    vi.clearAllMocks();
    api.calls.length = 0;
    let releaseFirst!: () => void;
    api.invoke.mockImplementationOnce(async (command: string) => {
      api.calls.push(`start:${command}`);
      await new Promise<void>((resolve) => { releaseFirst = resolve; });
      api.calls.push(`end:${command}`);
    });

    previewWidgetResize(96);
    await new Promise((resolve) => setTimeout(resolve, 0));
    await new Promise((resolve) => setTimeout(resolve, 0));
    previewWidgetResize(112);
    expect(api.calls).toEqual(["start:preview_widget_resize"]);

    releaseFirst();
    await new Promise((resolve) => setTimeout(resolve, 0));
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(api.calls).toEqual([
      "start:preview_widget_resize",
      "end:preview_widget_resize",
      "start:preview_widget_resize",
      "end:preview_widget_resize",
    ]);
    expect(api.invoke).toHaveBeenLastCalledWith("preview_widget_resize", { size: 112 });
  });

  it("cancels natively after an in-flight preview fails", async () => {
    const { beginWidgetResize, cancelWidgetResize, previewWidgetResize } = await import("./bridge");
    await beginWidgetResize("compact", "se");
    vi.clearAllMocks();
    api.calls.length = 0;
    let rejectPreview!: (reason: Error) => void;
    api.invoke.mockImplementationOnce((command: string) => {
      api.calls.push(`start:${command}`);
      return new Promise<void>((_resolve, reject) => { rejectPreview = reject; });
    });

    previewWidgetResize(96);
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(rejectPreview).toBeTypeOf("function");
    const cancellation = cancelWidgetResize();
    await Promise.resolve();
    rejectPreview(new Error("preview failed"));

    await expect(cancellation).resolves.toBeUndefined();
    expect(api.invoke).toHaveBeenCalledWith("cancel_widget_resize");
  });

  it("resets only the requested widget mode", async () => {
    const { resetWidgetSize } = await import("./bridge");
    await resetWidgetSize("expanded");
    expect(api.invoke).toHaveBeenCalledWith("reset_widget_size", { mode: "expanded" });
  });

  it("finishes a native drag after the window position settles", async () => {
    const { startDragging } = await import("./bridge");
    await startDragging();
    expect(api.invoke).toHaveBeenCalledWith("begin_widget_drag");
    expect(api.currentWindow.startDragging).toHaveBeenCalledTimes(1);
    expect(api.invoke).toHaveBeenCalledWith("finish_widget_drag");
  });

  it("clears native drag state when platform dragging fails", async () => {
    api.currentWindow.startDragging.mockRejectedValueOnce(new Error("drag failed"));
    const { startDragging } = await import("./bridge");
    await expect(startDragging()).rejects.toThrow("drag failed");
    expect(api.invoke).toHaveBeenCalledWith("finish_widget_drag");
  });
});

describe("custom skin bridge", () => {
  it("passes byte arrays and editable metadata with Tauri's camel-case payload keys", async () => {
    const { deleteCustomSkin, getCustomSkinAsset, importCustomSkin, updateCustomSkin } = await import("./bridge");

    await importCustomSkin("lake.webp", new Uint8Array([1, 2, 255]));
    await getCustomSkinAsset("custom-123-abc");
    await updateCustomSkin("custom-123-abc", "Lake", "dark", "#123456");
    await deleteCustomSkin("custom-123-abc");

    expect(api.invoke).toHaveBeenNthCalledWith(1, "import_custom_skin", {
      name: "lake.webp",
      bytes: [1, 2, 255],
    });
    expect(api.invoke).toHaveBeenNthCalledWith(2, "get_custom_skin_asset", { id: "custom-123-abc" });
    expect(api.invoke).toHaveBeenNthCalledWith(3, "update_custom_skin", {
      id: "custom-123-abc",
      name: "Lake",
      textTone: "dark",
      accentColor: "#123456",
    });
    expect(api.invoke).toHaveBeenNthCalledWith(4, "delete_custom_skin", { id: "custom-123-abc" });
  });

  it("has deterministic no-op browser fallbacks without invoking native commands", async () => {
    vi.stubGlobal("window", {});
    const { deleteCustomSkin, getCustomSkinAsset, importCustomSkin, updateCustomSkin } = await import("./bridge");

    await expect(importCustomSkin("lake.png", new Uint8Array([1]))).resolves.toBeNull();
    await expect(getCustomSkinAsset("custom-123-abc")).resolves.toBeNull();
    await expect(updateCustomSkin("custom-123-abc", "Lake", "auto", "#5A90D6"))
      .resolves.toMatchObject({ selectedSkin: "glass", customSkins: [] });
    await expect(deleteCustomSkin("custom-123-abc"))
      .resolves.toMatchObject({ selectedSkin: "glass", customSkins: [] });
    expect(api.invoke).not.toHaveBeenCalled();
  });
});

describe("settings window bridge stubs", () => {
  it("reads native glass capabilities", async () => {
    api.invoke.mockResolvedValueOnce({ nativeGlass: true, supportsLiquidGlass: false } as never);
    const { getPlatformCapabilities } = await import("./bridge");

    await expect(getPlatformCapabilities()).resolves.toEqual({ nativeGlass: true, supportsLiquidGlass: false });
    expect(api.invoke).toHaveBeenCalledWith("get_platform_capabilities");
  });

  it("reports no native glass capabilities in browser previews", async () => {
    vi.stubGlobal("window", {});
    const { getPlatformCapabilities } = await import("./bridge");

    await expect(getPlatformCapabilities()).resolves.toEqual({ nativeGlass: false, supportsLiquidGlass: false });
    expect(api.invoke).not.toHaveBeenCalled();
  });

  it("uses the Task 7 native commands when running under Tauri", async () => {
    api.invoke.mockResolvedValueOnce(undefined).mockResolvedValueOnce(true as never).mockResolvedValueOnce(true as never);
    const { getLaunchAtLogin, setLaunchAtLogin, showSettings } = await import("./bridge");

    await showSettings();
    await expect(getLaunchAtLogin()).resolves.toBe(true);
    await expect(setLaunchAtLogin(true)).resolves.toBe(true);

    expect(api.invoke).toHaveBeenNthCalledWith(1, "show_settings");
    expect(api.invoke).toHaveBeenNthCalledWith(2, "get_launch_at_login");
    expect(api.invoke).toHaveBeenNthCalledWith(3, "set_launch_at_login", { enabled: true });
  });

  it("keeps browser settings previews deterministic", async () => {
    vi.stubGlobal("window", {});
    const { getLaunchAtLogin, setLaunchAtLogin, showSettings } = await import("./bridge");

    await expect(showSettings()).resolves.toBeUndefined();
    await expect(getLaunchAtLogin()).resolves.toBe(false);
    await expect(setLaunchAtLogin(true)).resolves.toBe(true);
    expect(api.invoke).not.toHaveBeenCalled();
  });

  it("forwards verified launch-at-login state events and disposes the listener", async () => {
    const onLaunchAtLogin = vi.fn();
    const { listenDesktopEvents } = await import("./bridge");

    const dispose = await listenDesktopEvents({
      onPreferences: vi.fn(),
      onRefresh: vi.fn(),
      onUpdate: vi.fn(),
      onLaunchAtLogin,
    });
    api.eventHandlers.get("launch-at-login-changed")?.({ payload: true });
    expect(onLaunchAtLogin).toHaveBeenCalledWith(true);

    dispose();
    expect(api.eventHandlers.has("launch-at-login-changed")).toBe(false);
  });
});
