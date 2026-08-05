import type { ProviderSnapshot, SupporterStatus, WidgetMode, WidgetPreferences, WidgetSize, WidgetSkin } from "../types";
import type { ResizeEdge } from "./resize";

const defaultPreferences: WidgetPreferences = { locked: false, alwaysOnTop: true, widgetMode: "compact", widgetSize: "medium", compactSize: 72, expandedSize: 306, toggleCorner: "ne", pinnedProvider: null, autoRotateSeconds: 12, language: "zh-CN", appearance: "light", license: null, licenses: [], unlockedSkin: null, unlockedSkins: [], selectedSkin: "default" };

function widgetSizeMarker(compactSize: number, expandedSize: number): WidgetSize {
  const presets: Array<[WidgetSize, number, number]> = [
    ["small", 72 * 0.84, 306 * 0.84],
    ["medium", 72, 306],
    ["large", 72 * 1.16, 306 * 1.16],
  ];
  return presets.find(([, compact, expanded]) => Math.abs(compactSize - compact) <= 0.01 && Math.abs(expandedSize - expanded) <= 0.01)?.[0] ?? "custom";
}

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
  let began = false;
  try {
    await invoke("begin_widget_drag");
    began = true;
    await currentWindow.startDragging();
    let previous = await currentWindow.outerPosition();
    let stableTicks = 0;
    let attempts = 0;
    // Keep this promise pending until the native drag has settled. The orb
    // uses that lifecycle to keep the release click suppressed; resolving as
    // soon as startDragging() returns lets WebKit deliver a synthetic click
    // while the platform drag is still winding down.
    await new Promise<void>((resolve) => {
      let settled = false;
      let pollInFlight = false;
      let intervalId: number | null = null;
      let timeoutId: number | null = null;
      const cleanup = () => {
        if (intervalId !== null) window.clearInterval(intervalId);
        if (timeoutId !== null) window.clearTimeout(timeoutId);
      };
      const finish = () => {
        if (settled) return;
        settled = true;
        cleanup();
        resolve();
      };
      const poll = () => {
        if (settled || pollInFlight) return;
        attempts += 1;
        if (attempts >= 25) {
          finish();
          return;
        }
        pollInFlight = true;
        void currentWindow.outerPosition()
          .then((next) => {
            if (settled) return;
            const stable = Math.abs(next.x - previous.x) <= 1 && Math.abs(next.y - previous.y) <= 1;
            stableTicks = stable ? stableTicks + 1 : 0;
            previous = next;
            if (stableTicks >= 3) finish();
          })
          .catch(finish)
          .finally(() => { pollInFlight = false; });
      };
      // A platform or WebView failure must not leave the drag state held
      // forever. The timeout is wall-clock based, independent of a stuck
      // outerPosition() request, and cleanup prevents interval accumulation.
      timeoutId = window.setTimeout(finish, 2_500);
      intervalId = window.setInterval(poll, 80);
      poll();
    });
  } finally {
    // begin_widget_drag records the mode used to update the anchor. Always
    // clear it after a partially failed native drag as well as a normal one.
    if (began) await invoke("finish_widget_drag").catch(() => undefined);
  }
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

export function beginWidgetResize(mode: WidgetMode, edge: ResizeEdge): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return enqueueWidgetTransition(async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("begin_widget_resize", { mode, edge, workArea: await currentWorkArea() });
  });
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
        await invoke("preview_widget_resize", { size });
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
  return enqueueWidgetTransition(async () => {
    await drainWidgetResizePreviews();
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<WidgetPreferences>("finish_widget_resize", { size });
  });
}

export async function resetWidgetSize(mode: WidgetMode, current: WidgetPreferences = defaultPreferences): Promise<WidgetPreferences | undefined> {
  if (!isTauri()) {
    const compactSize = mode === "compact" ? 72 : current.compactSize;
    const expandedSize = mode === "expanded" ? 306 : current.expandedSize;
    return {
      ...current,
      widgetMode: mode,
      compactSize,
      expandedSize,
      widgetSize: widgetSizeMarker(compactSize, expandedSize),
    };
  }
  return enqueueWidgetTransition(async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<WidgetPreferences>("reset_widget_size", { mode });
  });
}

export function cancelWidgetResize(): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return enqueueWidgetTransition(async () => {
    resizePreviewLatest = null;
    await drainWidgetResizePreviews();
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("cancel_widget_resize");
  });
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
