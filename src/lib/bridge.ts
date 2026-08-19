import type { CustomSkinAsset, CustomSkinMetadata, PlatformCapabilities, ProviderSnapshot, SkinTextTone, WidgetMode, WidgetPreferences, WidgetSize } from "../types";
import type { ResizeEdge } from "./resize";
import { invoke as nativeInvoke } from "@tauri-apps/api/core";

export type { CustomSkinAsset, CustomSkinMetadata, SkinTextTone } from "../types";

const defaultPreferences: WidgetPreferences = { locked: false, alwaysOnTop: true, widgetMode: "compact", widgetSize: "medium", compactSize: 72, expandedSize: 306, toggleCorner: "ne", pinnedProvider: null, autoRotateSeconds: 12, autoCheckUpdates: true, language: "zh-CN", appearance: "light", selectedSkin: "glass", glassStyle: "dock", customSkins: [] };

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
let resizePreviewLastDispatched: number | null = null;
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

export async function getPlatformCapabilities(): Promise<PlatformCapabilities> {
  if (!isTauri()) return { nativeGlass: false, supportsLiquidGlass: false };
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<PlatformCapabilities>("get_platform_capabilities");
}

export async function getAppVersion(): Promise<string> {
  if (!isTauri()) return "1.0.0";
  const { getVersion } = await import("@tauri-apps/api/app");
  return getVersion();
}

export async function updatePreferences(value: WidgetPreferences): Promise<WidgetPreferences | undefined> {
  if (!isTauri()) return undefined;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<WidgetPreferences>("set_preferences", { preferences: value });
}

/** Native menu/window ownership lands in Task 7; these commands intentionally
 * keep deterministic browser fallbacks so the settings UI is previewable now. */
export async function showSettings(): Promise<void> {
  if (!isTauri()) return;
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("show_settings");
}

export async function getLaunchAtLogin(): Promise<boolean> {
  if (!isTauri()) return false;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<boolean>("get_launch_at_login");
}

export async function setLaunchAtLogin(enabled: boolean): Promise<boolean> {
  if (!isTauri()) return enabled;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<boolean>("set_launch_at_login", { enabled });
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

export async function selectSkin(id: string): Promise<WidgetPreferences> {
  if (!isTauri()) {
    const selectedSkin = id === "computer" || id === "glass" ? id : "glass";
    return { ...defaultPreferences, selectedSkin };
  }
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<WidgetPreferences>("select_skin", { id });
}

export async function importCustomSkin(name: string, bytes: Uint8Array): Promise<CustomSkinMetadata | null> {
  if (!isTauri()) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<CustomSkinMetadata>("import_custom_skin", { name, bytes: Array.from(bytes) });
}

export async function getCustomSkinAsset(id: string): Promise<CustomSkinAsset | null> {
  if (!isTauri()) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<CustomSkinAsset>("get_custom_skin_asset", { id });
}

export async function updateCustomSkin(
  id: string,
  name: string,
  textTone: SkinTextTone,
  accentColor: string,
): Promise<WidgetPreferences> {
  if (!isTauri()) return defaultPreferences;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<WidgetPreferences>("update_custom_skin", { id, name, textTone, accentColor });
}

export async function deleteCustomSkin(id: string): Promise<WidgetPreferences> {
  if (!isTauri()) return defaultPreferences;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<WidgetPreferences>("delete_custom_skin", { id });
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

export function setWidgetMode(mode: WidgetMode, southwestWeeklyPrimary = false): Promise<WidgetPreferences | undefined> {
  if (!isTauri()) return Promise.resolve({ ...defaultPreferences, widgetMode: mode });
  return enqueueWidgetTransition(async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    const workArea = await currentWorkArea();
    return invoke<WidgetPreferences>("set_widget_mode", { mode, workArea, southwestWeeklyPrimary });
  });
}

export function syncWidgetLayout(southwestWeeklyPrimary: boolean): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return enqueueWidgetTransition(async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("sync_widget_layout", { southwestWeeklyPrimary });
  });
}

export function setWidgetSize(size: WidgetSize): Promise<WidgetPreferences | undefined> {
  if (!isTauri()) {
    const factor = size === "small" ? 0.84 : size === "large" ? 1.16 : 1;
    return Promise.resolve({ ...defaultPreferences, widgetSize: size, compactSize: 72 * factor, expandedSize: 306 * factor });
  }
  return enqueueWidgetTransition(async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    // The native layer resolves the widget's own monitor. Passing the
    // settings window work area here can incorrectly clamp a widget on a
    // different display.
    return invoke<WidgetPreferences>("set_widget_size", { size });
  });
}

export function setWidgetDimensions(compactSize: number, expandedSize: number): Promise<WidgetPreferences | undefined> {
  const compact = Math.min(144, Math.max(48, compactSize));
  const expanded = Math.min(460, Math.max(220, expandedSize));
  if (!isTauri()) return Promise.resolve({ ...defaultPreferences, widgetSize: widgetSizeMarker(compact, expanded), compactSize: compact, expandedSize: expanded });
  return enqueueWidgetTransition(async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    // The native command resolves the widget's own monitor. The settings
    // window may be on another display, so never use its monitor as geometry
    // input for the floating widget.
    return invoke<WidgetPreferences>("set_widget_dimensions", { compactSize: compact, expandedSize: expanded });
  });
}

export function beginWidgetResize(mode: WidgetMode, edge: ResizeEdge): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return enqueueWidgetTransition(async () => {
    resizePreviewLatest = null;
    resizePreviewLastDispatched = null;
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
      while (resizePreviewLatest !== null) {
        const size = resizePreviewLatest;
        resizePreviewLatest = null;
        if (size === resizePreviewLastDispatched) continue;
        await nativeInvoke("preview_widget_resize", { size });
        resizePreviewLastDispatched = size;
      }
    } finally {
      resizePreviewRunning = false;
    }
  })();
  return resizePreviewDrain;
}

export function previewWidgetResize(size: number): void {
  if (!isTauri()) return;
  resizePreviewLatest = Math.round(size);
  void drainWidgetResizePreviews().catch(() => undefined);
}

export async function finishWidgetResize(mode: WidgetMode, size: number): Promise<WidgetPreferences | undefined> {
  const roundedSize = Math.round(size);
  if (!isTauri()) return { ...defaultPreferences, [mode === "compact" ? "compactSize" : "expandedSize"]: roundedSize, widgetSize: "custom", widgetMode: mode };
  return enqueueWidgetTransition(async () => {
    previewWidgetResize(roundedSize);
    await drainWidgetResizePreviews();
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<WidgetPreferences>("finish_widget_resize", { size: roundedSize });
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
    await drainWidgetResizePreviews().catch(() => undefined);
    resizePreviewLastDispatched = null;
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("cancel_widget_resize");
  });
}

export async function listenDesktopEvents(handlers: {
  onPreferences: (value: WidgetPreferences) => void;
  onRefresh: () => void;
  onUpdate: () => void;
  onLaunchAtLogin?: (enabled: boolean) => void;
}): Promise<() => void> {
  if (!isTauri()) return () => undefined;
  const { listen } = await import("@tauri-apps/api/event");
  const unlistenPreferences = await listen<WidgetPreferences>("preferences-changed", (event) => handlers.onPreferences(event.payload));
  const unlistenRefresh = await listen("refresh-requested", handlers.onRefresh);
  const unlistenUpdate = await listen("update-check-requested", handlers.onUpdate);
  const unlistenLaunchAtLogin = handlers.onLaunchAtLogin
    ? await listen<boolean>("launch-at-login-changed", (event) => handlers.onLaunchAtLogin?.(event.payload))
    : () => undefined;
  return () => { unlistenPreferences(); unlistenRefresh(); unlistenUpdate(); unlistenLaunchAtLogin(); };
}
