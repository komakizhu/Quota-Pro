import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { QuotaCard, QuotaOrb } from "./components/QuotaCard";
import { beginWidgetResize, cancelWidgetResize, fetchSnapshots, finishWidgetResize, getPreferences, getSupporterStatus, listenDesktopEvents, previewWidgetResize, resetWidgetSize, setAlwaysOnTop, setWidgetMode, startDragging, syncWidgetAppearance, updatePreferences } from "./lib/bridge";
import { needsFastRefresh, quotaTier } from "./lib/format";
import { checkForAppUpdate, openReleasePage } from "./lib/appUpdate";
import { copy, normalizeLanguage } from "./lib/i18n";
import { mergeSnapshots } from "./lib/snapshots";
import { DESKTOP_PALETTES } from "./lib/desktopPalette";
import type { ResizeEdge } from "./lib/resize";
import type { ProviderSnapshot, ToggleCorner, WidgetMode, WidgetPreferences, WidgetSize, WidgetSkin, WidgetTheme } from "./types";

const DEFAULT_PREFS: WidgetPreferences = { locked: false, alwaysOnTop: true, widgetMode: "compact", widgetSize: "medium", compactSize: 72, expandedSize: 306, toggleCorner: "ne", pinnedProvider: null, autoRotateSeconds: 12, language: "zh-CN", appearance: "light", license: null, licenses: [], unlockedSkin: null, unlockedSkins: [], selectedSkin: "default" };
const DEFAULT_COMPACT_SIZE = 72;
const DEFAULT_EXPANDED_SIZE = 306;
const COMPACT_MIN_SIZE = 48;
const COMPACT_MAX_SIZE = 144;
const EXPANDED_MIN_SIZE = 220;
const EXPANDED_MAX_SIZE = 460;
const PRESET_FACTOR: Record<Exclude<WidgetSize, "custom">, number> = { small: 0.84, medium: 1, large: 1.16 };
const INITIAL_SNAPSHOT: ProviderSnapshot = {
  provider: "codex",
  displayName: "CODEX",
  plan: null,
  shortWindow: null,
  weeklyWindow: null,
  resetCredits: null,
  resetCreditExpiresAt: [],
  updatedAt: new Date().toISOString(),
  status: "unavailable",
  message: "Quota is loading.",
};

export default function App() {
  const [snapshots, setSnapshots] = useState<ProviderSnapshot[]>([]);
  const [preferences, setPreferences] = useState(DEFAULT_PREFS);
  const [activeIndex, setActiveIndex] = useState(0);
  const [consumingProviders, setConsumingProviders] = useState<Set<string>>(() => new Set());
  const [operationError, setOperationError] = useState<string | null>(null);
  const [pendingWidgetMode, setPendingWidgetMode] = useState<WidgetMode | null>(null);
  const [showUpdateFallback, setShowUpdateFallback] = useState(false);
  const [systemDark, setSystemDark] = useState(() => window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? false);
  const failures = useRef(0);
  const previousPrimary = useRef(new Map<string, number>());
  const consumptionTimers = useRef(new Map<string, number>());
  const language = normalizeLanguage(preferences.language);
  const t = copy[language];
  const operation = language === "zh-CN" ? {
    listenerFailed: "桌面事件监听启动失败。",
    settingsFailed: "设置保存失败，已恢复之前的状态。",
    alwaysOnTopFailed: "置顶状态切换失败。",
    expandFailed: "组件展开失败。",
    collapseFailed: "组件收起失败。",
    resizeFailed: "组件尺寸保存失败，已恢复之前的大小。",
    releaseOpenFailed: "无法打开 GitHub Releases。",
  } : {
    listenerFailed: "Desktop event listener failed to start.",
    settingsFailed: "Settings could not be saved. Previous state restored.",
    alwaysOnTopFailed: "Always-on-top toggle failed.",
    expandFailed: "Widget expand failed.",
    collapseFailed: "Widget collapse failed.",
    resizeFailed: "Widget size could not be saved. Previous size restored.",
    releaseOpenFailed: "Could not open GitHub Releases.",
  };
  const theme: WidgetTheme = preferences.appearance === "system" ? (systemDark ? "dark" : "light") : preferences.appearance;
  const skin: WidgetSkin = preferences.unlockedSkins.includes(preferences.selectedSkin as Exclude<WidgetSkin, "default">)
    && (preferences.selectedSkin === "blur" || preferences.selectedSkin === "computer")
    ? preferences.selectedSkin
    : "default";

  useEffect(() => {
    // This only reconciles the transparent-window safety inset after a theme
    // change. A platform refusal is non-fatal: the current widget geometry is
    // still usable and users should never see an internal resize error.
    void syncWidgetAppearance(theme).catch(() => undefined);
  }, [theme]);

  useEffect(() => {
    const media = window.matchMedia?.("(prefers-color-scheme: dark)");
    if (!media) return;
    const onChange = () => setSystemDark(media.matches);
    onChange();
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, []);

  const checkUpdate = useCallback((manual = false) => {
    setShowUpdateFallback(false);
    void checkForAppUpdate(language, {
      checking: t.updateChecking,
      current: t.updateCurrent,
      downloading: t.updateDownloading,
      installing: t.updateInstalling,
      availableWindows: t.updateAvailableWindows,
      availableMac: t.updateAvailableMac,
      failed: t.updateFailed,
    }, (message) => {
      setOperationError(message);
      if (message === t.updateFailed) setShowUpdateFallback(true);
    }, manual);
  }, [language, t]);

  const refresh = useCallback(async (force = false) => {
    try {
      const values = await fetchSnapshots(force);
      const hasFailure = values.some((item) => item.status !== "ok");
      if (hasFailure) failures.current += 1;
      else failures.current = 0;
      for (const item of values) {
        const nextPrimary = item.shortWindow?.remainingPercent;
        const previous = previousPrimary.current.get(item.provider);
        if (nextPrimary !== undefined && previous !== undefined && nextPrimary < previous) {
          setConsumingProviders((current) => new Set(current).add(item.provider));
          const oldTimer = consumptionTimers.current.get(item.provider);
          if (oldTimer !== undefined) window.clearTimeout(oldTimer);
          const timer = window.setTimeout(() => {
            setConsumingProviders((current) => { const next = new Set(current); next.delete(item.provider); return next; });
            consumptionTimers.current.delete(item.provider);
          }, 5 * 60_000);
          consumptionTimers.current.set(item.provider, timer);
        }
        if (nextPrimary !== undefined) previousPrimary.current.set(item.provider, nextPrimary);
      }
      setSnapshots((current) => mergeSnapshots(current, values));
    } catch {
      failures.current += 1;
      setSnapshots((current) => current.length > 0
        ? current.map((item) => ({ ...item, status: "stale", message: "Refresh failed. Please try again later." }))
        : [{ provider: "codex", displayName: "CODEX", plan: null, shortWindow: null, weeklyWindow: null, resetCredits: null, resetCreditExpiresAt: [], updatedAt: new Date().toISOString(), status: "unavailable", message: "Quota is temporarily unavailable. It will retry automatically." }]);
    }
  }, []);

  const normalizePreferences = useCallback((value: Partial<WidgetPreferences> & { stayExpanded?: boolean }): WidgetPreferences => {
    const widgetSize = value.widgetSize === "small" || value.widgetSize === "large" || value.widgetSize === "custom" || value.widgetSize === "medium" ? value.widgetSize : "medium";
    const factor = widgetSize === "custom" ? 1 : PRESET_FACTOR[widgetSize];
    const compactSize = Number.isFinite(value.compactSize) && (value.compactSize ?? 0) > 0
      ? value.compactSize!
      : DEFAULT_COMPACT_SIZE * factor;
    const expandedSize = Number.isFinite(value.expandedSize) && (value.expandedSize ?? 0) > 0
      ? value.expandedSize!
      : DEFAULT_EXPANDED_SIZE * factor;
    return {
      ...DEFAULT_PREFS,
      ...value,
      widgetMode: value.widgetMode === "expanded" || value.widgetMode === "compact" ? value.widgetMode : (value.stayExpanded ? "expanded" : "compact"),
      widgetSize,
      compactSize: Math.min(COMPACT_MAX_SIZE, Math.max(COMPACT_MIN_SIZE, compactSize)),
      expandedSize: Math.min(EXPANDED_MAX_SIZE, Math.max(EXPANDED_MIN_SIZE, expandedSize)),
      toggleCorner: (value.toggleCorner === "nw" || value.toggleCorner === "ne" || value.toggleCorner === "sw" || value.toggleCorner === "se" ? value.toggleCorner : "ne") as ToggleCorner,
      language: normalizeLanguage(value.language),
    };
  }, []);

  useEffect(() => {
    void refresh(true);
    void (async () => {
      // Validate and normalize stored supporter state before allowing it to
      // affect rendering, avoiding a stale preference response re-enabling it.
      await getSupporterStatus().catch(() => undefined);
      const value = await getPreferences().catch(async () => {
        // A WebView can occasionally issue its first invoke while it is
        // resuming. Retry once, then retain the already-safe defaults without
        // showing a persistent warning on the quota card.
        await new Promise((resolve) => window.setTimeout(resolve, 120));
        return getPreferences().catch(() => DEFAULT_PREFS);
      });
      const normalized = normalizePreferences(value);
      setPreferences(normalized);
      await setWidgetMode(normalized.widgetMode);
    })().catch(() => setPreferences(DEFAULT_PREFS));
    return () => {
      for (const timer of consumptionTimers.current.values()) window.clearTimeout(timer);
      consumptionTimers.current.clear();
    };
  }, [normalizePreferences, refresh]);

  useEffect(() => {
    let cancelled = false;
    let cleanup: () => void = () => {};
    void listenDesktopEvents({ onPreferences: (value) => setPreferences(normalizePreferences(value)), onRefresh: () => void refresh(true), onUpdate: () => checkUpdate(true) }).then((value) => {
      if (cancelled) value(); else cleanup = value;
    }).catch(() => setOperationError(operation.listenerFailed));
    return () => { cancelled = true; cleanup(); };
  }, [checkUpdate, normalizePreferences, operation.listenerFailed, refresh]);

  useEffect(() => {
    const timer = window.setTimeout(() => checkUpdate(false), 12_000);
    return () => window.clearTimeout(timer);
  }, [checkUpdate]);

  const refreshMs = useMemo(() => {
    const backoff = failures.current === 0 ? 5 * 60_000 : Math.min(30 * 60_000, 30_000 * 2 ** (failures.current - 1));
    if (failures.current === 0 && snapshots.some((item) => item.status === "ok" && needsFastRefresh(item))) return 60_000;
    return backoff;
  }, [snapshots]);

  useEffect(() => {
    const id = window.setInterval(() => void refresh(), refreshMs);
    return () => window.clearInterval(id);
  }, [refresh, refreshMs]);

  useEffect(() => {
    const refreshWhenActive = () => { if (document.visibilityState === "visible") void refresh(true); };
    window.addEventListener("focus", refreshWhenActive);
    document.addEventListener("visibilitychange", refreshWhenActive);
    return () => {
      window.removeEventListener("focus", refreshWhenActive);
      document.removeEventListener("visibilitychange", refreshWhenActive);
    };
  }, [refresh]);

  useEffect(() => {
    if (preferences.pinnedProvider || snapshots.length < 2) return;
    const id = window.setInterval(() => setActiveIndex((value) => (value + 1) % snapshots.length), preferences.autoRotateSeconds * 1000);
    return () => window.clearInterval(id);
  }, [preferences.autoRotateSeconds, preferences.pinnedProvider, snapshots.length]);

  const current = preferences.pinnedProvider
    ? snapshots.find((item) => item.provider === preferences.pinnedProvider) ?? snapshots[0] ?? INITIAL_SNAPSHOT
    : snapshots[activeIndex % Math.max(1, snapshots.length)] ?? INITIAL_SNAPSHOT;

  const primaryPercent = current?.shortWindow?.remainingPercent ?? current?.weeklyWindow?.remainingPercent ?? null;
  const tier = quotaTier(primaryPercent);
  const paletteName = current.status === "unavailable" || current.status === "stale" || current.status === "signed_out"
    ? current.status
    : tier === "healthy" || tier === "caution" || tier === "critical" ? tier : null;
  // The production widget and design workbench share one explicit palette
  // source. Theme records are independent so light and dark cannot leak into
  // one another through CSS defaults or preview state.
  const cardStyle = {
    ...(paletteName ? DESKTOP_PALETTES[theme][paletteName] : {}),
    "--widget-scale": String((preferences.widgetMode === "compact" ? preferences.compactSize : preferences.expandedSize) / (preferences.widgetMode === "compact" ? DEFAULT_COMPACT_SIZE : DEFAULT_EXPANDED_SIZE)),
  } as CSSProperties;

  const savePreferences = useCallback((next: WidgetPreferences) => {
    const previous = preferences;
    setPreferences(next);
    setOperationError(null);
    void updatePreferences(next).catch(() => { setPreferences(previous); setOperationError(operation.settingsFailed); });
  }, [operation.settingsFailed, preferences]);

  const changeWidgetMode = useCallback(async (mode: WidgetMode) => {
    if (pendingWidgetMode || mode === preferences.widgetMode) return;
    const previous = preferences;
    setPendingWidgetMode(mode);
    setOperationError(null);
    try {
      const saved = await setWidgetMode(mode);
      if (saved) setPreferences(normalizePreferences(saved));
    } catch {
      setPreferences(previous);
      setOperationError(mode === "expanded" ? operation.expandFailed : operation.collapseFailed);
    } finally {
      setPendingWidgetMode(null);
    }
  }, [normalizePreferences, operation.collapseFailed, operation.expandFailed, pendingWidgetMode, preferences]);

  const beginResize = useCallback((mode: WidgetMode, edge: ResizeEdge) => beginWidgetResize(mode, edge), []);
  const previewResize = useCallback((size: number) => previewWidgetResize(size), []);
  const commitResize = useCallback(async (mode: WidgetMode, size: number) => {
    const previous = preferences;
    try {
      const saved = await finishWidgetResize(mode, size);
      if (saved) setPreferences(normalizePreferences(saved));
      setOperationError(null);
    } catch (error) {
      await cancelWidgetResize().catch(() => undefined);
      setPreferences(previous);
      setOperationError(operation.resizeFailed);
      throw error;
    }
  }, [normalizePreferences, operation.resizeFailed, preferences]);

  const resetResize = useCallback(async (mode: WidgetMode) => {
    const previous = preferences;
    try {
      const saved = await resetWidgetSize(mode, previous);
      if (saved) setPreferences(normalizePreferences(saved));
      setOperationError(null);
    } catch (error) {
      setPreferences(previous);
      setOperationError(operation.resizeFailed);
      throw error;
    }
  }, [normalizePreferences, operation.resizeFailed, preferences]);

  if (preferences.widgetMode === "compact") {
    return <QuotaOrb snapshot={current} language={language} onExpand={() => { if (!pendingWidgetMode) { void refresh(true); void changeWidgetMode("expanded"); } }} onDrag={() => startDragging()} onResizeStart={(edge) => beginResize("compact", edge)} onResizePreview={previewResize} onResizeCommit={(size) => commitResize("compact", size)} onResizeCancel={cancelWidgetResize} onResizeReset={() => resetResize("compact")} resizeSize={preferences.compactSize} theme={theme} skin={skin} style={cardStyle} />;
  }

  return (
    <QuotaCard
      snapshot={current}
      preferences={preferences}
      providerCount={snapshots.length}
      onPrevious={() => setActiveIndex((value) => (value - 1 + snapshots.length) % snapshots.length)}
      onNext={() => setActiveIndex((value) => (value + 1) % snapshots.length)}
      onTogglePin={() => savePreferences({ ...preferences, pinnedProvider: preferences.pinnedProvider ? null : current.provider })}
      onCollapse={() => { void changeWidgetMode("compact"); }}
      toggleCorner={preferences.toggleCorner}
      onLock={() => { setOperationError(null); void setAlwaysOnTop(!preferences.alwaysOnTop).then((value) => setPreferences({ ...DEFAULT_PREFS, ...value, language: normalizeLanguage(value.language) })).catch(() => setOperationError(operation.alwaysOnTopFailed)); }}
      onDrag={() => startDragging()}
      onResizeStart={(edge) => beginResize("expanded", edge)}
      onResizePreview={previewResize}
      onResizeCommit={(size) => commitResize("expanded", size)}
      onResizeCancel={cancelWidgetResize}
      onResizeReset={() => resetResize("expanded")}
      resizeSize={preferences.expandedSize}
      onRefresh={() => refresh(true)}
      isConsuming={consumingProviders.has(current.provider)}
      theme={theme}
      skin={skin}
      style={cardStyle}
      notice={showUpdateFallback && operationError ? <><span>{operationError}</span><button type="button" onMouseDown={(event) => event.stopPropagation()} onClick={() => void openReleasePage().catch(() => setOperationError(operation.releaseOpenFailed))}>GitHub Releases</button></> : operationError}
    />
  );
}
