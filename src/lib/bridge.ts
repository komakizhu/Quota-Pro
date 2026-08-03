import type { ProviderSnapshot, SupporterStatus, WidgetMode, WidgetPreferences, WidgetSize, WidgetSkin } from "../types";
import type { ResizeEdge } from "./resize";

const defaultPreferences: WidgetPreferences = { locked: false, alwaysOnTop: true, widgetMode: "compact", widgetSize: "medium", compactSize: 72, expandedSize: 306, pinnedProvider: null, autoRotateSeconds: 12, language: "zh-CN", appearance: "light", license: null, licenses: [], unlockedSkin: null, unlockedSkins: [], selectedSkin: "default" };

const mockSnapshot: ProviderSnapshot = {
  provider: "codex",
  displayName: "CODEX",
  plan: "PRO",
  shortWindow: { remainingPercent: 74, resetsAt: new Date(Date.now() + 78 * 60_000).toISOString(), windowSeconds: 18_000 },
  weeklyWindow: { remainingPercent: 42, resetsAt: new Date(Date.now() + 3.2 * 86_400_000).toISOString(), windowSeconds: 604_800 },
  resetCredits: 1,
  resetCreditExpiresAt: [new Date(Date.now() + 9 * 86_400_000).toISOString()],
  updatedAt: new Date().toISOString(),
  status: "ok",
  message: null,
};

let widgetTransition: Promise<unknown> = Promise.resolve();
let resizePreviewLatest: number | null = null;
let resizePreviewRunning = false;
let resizePreviewDrain: Promise<void> = Promise.resolve();

function enqueueWidgetTransition<T>(operation: () => Promise<T>): Promise<T> {
  const next = widgetTransition.then(operation, operation);
  widgetTransition = next.then(() => undefined, () => undefined);
  return next;
}

async function currentWorkArea() {
  const { currentMonitor } = await import("@tauri-apps/api/window");
  const monitor = await currentMonitor().catch(() => null);
  return monitor ? {
    position: { x: monitor.workArea.position.x, y: monitor.workArea.position.y },
    size: { width: monitor.workArea.size.width, height: monitor.workArea.size.height },
  } : null;
}

export const isTauri = () => "__TAURI_INTERNALS__" in window;

export async function fetchSnapshots(force = false): Promise<ProviderSnapshot[]> {
  if (!isTauri()) return [mockSnapshot];
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<ProviderSnapshot[]>(force ? "refresh_snapshots" : "get_snapshots");
}

export async function getPreferences(): Promise<WidgetPreferences> {
  if (!isTauri()) return defaultPreferences;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<WidgetPreferences>("get_preferences");
}

export async function updatePreferences(value: WidgetPreferences): Promise<void> {
  if (!isTauri()) return;
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("set_preferences", { preferences: value });
}

export async function setClickThrough(locked: boolean): Promise<WidgetPreferences> {
  if (!isTauri()) return { ...defaultPreferences, locked };
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<WidgetPreferences>("set_widget_locked", { locked });
}

export async function setAlwaysOnTop(alwaysOnTop: boolean): Promise<WidgetPreferences> {
  if (!isTauri()) return { ...defaultPreferences, alwaysOnTop };
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<WidgetPreferences>("set_widget_always_on_top", { alwaysOnTop });
}

export async function syncWidgetAppearance(appearance: "light" | "dark"): Promise<void> {
  if (!isTauri()) return;
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("sync_widget_appearance", { appearance });
}

export async function getSupporterStatus(): Promise<SupporterStatus> {
  if (!isTauri()) return { requestCode: "Browser preview does not have a device code", active: false, message: "Supporter activation is available in the desktop app.", unlockedSkin: null, unlockedSkins: [], selectedSkin: "default", availableSkins: ["default"] };
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<SupporterStatus>("get_supporter_status");
}

export async function activateSupporterLicense(license: string): Promise<SupporterStatus> {
  if (!isTauri()) throw new Error("Supporter activation is available in the desktop app.");
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<SupporterStatus>("activate_supporter_license", { license });
}

export async function selectSupporterSkin(skinId: WidgetSkin): Promise<SupporterStatus> {
  if (!isTauri()) throw new Error("Supporter skin selection is available in the desktop app.");
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<SupporterStatus>("select_supporter_skin", { skinId });
}

export async function startDragging(): Promise<void> {
  if (!isTauri()) return;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  const { invoke } = await import("@tauri-apps/api/core");
  const currentWindow = getCurrentWindow();
  await invoke("begin_widget_drag");
  await currentWindow.startDragging();
  let previous = await currentWindow.outerPosition();
  let stableTicks = 0;
  let attempts = 0;
  const finishWhenStable = window.setInterval(() => {
    void currentWindow.outerPosition()
      .then((next) => {
        attempts += 1;
        const stable = Math.abs(next.x - previous.x) <= 1 && Math.abs(next.y - previous.y) <= 1;
        stableTicks = stable ? stableTicks + 1 : 0;
        previous = next;
        if (stableTicks >= 3 || attempts >= 25) {
          window.clearInterval(finishWhenStable);
          void invoke("finish_widget_drag").catch(() => undefined);
        }
      })
      .catch(() => {
        window.clearInterval(finishWhenStable);
        void invoke("finish_widget_drag").catch(() => undefined);
      });
  }, 80);
}

export function setWidgetMode(mode: WidgetMode): Promise<WidgetPreferences | undefined> {
  if (!isTauri()) return Promise.resolve({ ...defaultPreferences, widgetMode: mode });
  return enqueueWidgetTransition(async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    const workArea = await currentWorkArea();
    return invoke<WidgetPreferences>("set_widget_mode", { mode, workArea });
  });
}

export function setWidgetSize(size: WidgetSize): Promise<WidgetPreferences | undefined> {
  if (!isTauri()) {
    const factor = size === "small" ? 0.84 : size === "large" ? 1.16 : 1;
    return Promise.resolve({ ...defaultPreferences, widgetSize: size, compactSize: 72 * factor, expandedSize: 306 * factor });
  }
  return enqueueWidgetTransition(async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    const workArea = await currentWorkArea();
    return invoke<WidgetPreferences>("set_widget_size", { size, workArea });
  });
}

export async function beginWidgetResize(mode: WidgetMode, edge: ResizeEdge): Promise<void> {
  if (!isTauri()) return;
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("begin_widget_resize", { mode, edge, workArea: await currentWorkArea() });
}

async function drainWidgetResizePreviews(): Promise<void> {
  if (!isTauri()) return;
  if (resizePreviewRunning) return resizePreviewDrain;
  resizePreviewRunning = true;
  resizePreviewDrain = (async () => {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      while (resizePreviewLatest !== null) {
        const size = resizePreviewLatest;
        resizePreviewLatest = null;
        await invoke("preview_widget_resize", { size, workArea: await currentWorkArea() });
      }
    } finally {
      resizePreviewRunning = false;
    }
  })();
  return resizePreviewDrain;
}

export function previewWidgetResize(size: number): void {
  if (!isTauri()) return;
  resizePreviewLatest = size;
  void drainWidgetResizePreviews().catch(() => undefined);
}

export async function finishWidgetResize(mode: WidgetMode, size: number): Promise<WidgetPreferences | undefined> {
  if (!isTauri()) return { ...defaultPreferences, [mode === "compact" ? "compactSize" : "expandedSize"]: size, widgetSize: "custom", widgetMode: mode };
  await drainWidgetResizePreviews();
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<WidgetPreferences>("finish_widget_resize", { size, workArea: await currentWorkArea() });
}

export async function cancelWidgetResize(): Promise<void> {
  if (!isTauri()) return;
  resizePreviewLatest = null;
  await drainWidgetResizePreviews();
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("cancel_widget_resize");
}

export async function listenDesktopEvents(handlers: {
  onPreferences: (value: WidgetPreferences) => void;
  onRefresh: () => void;
  onUpdate: () => void;
}): Promise<() => void> {
  if (!isTauri()) return () => undefined;
  const { listen } = await import("@tauri-apps/api/event");
  const unlistenPreferences = await listen<WidgetPreferences>("preferences-changed", (event) => handlers.onPreferences(event.payload));
  const unlistenRefresh = await listen("refresh-requested", handlers.onRefresh);
  const unlistenUpdate = await listen("update-check-requested", handlers.onUpdate);
  return () => { unlistenPreferences(); unlistenRefresh(); unlistenUpdate(); };
}
