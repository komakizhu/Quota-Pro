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
});
