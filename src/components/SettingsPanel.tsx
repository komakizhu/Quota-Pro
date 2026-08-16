import { ArrowClockwise, Check, GearSix, GithubLogo, Image, Monitor, PaintBrush, Trash } from "@phosphor-icons/react";
import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties, type ChangeEvent } from "react";
import {
  deleteCustomSkin,
  getAppVersion,
  getCustomSkinAsset,
  getLaunchAtLogin,
  getPlatformCapabilities,
  getPreferences,
  importCustomSkin,
  listenDesktopEvents,
  selectSkin,
  setClickThrough,
  setLaunchAtLogin,
  setWidgetDimensions,
  setWidgetSize,
  updateCustomSkin,
  updatePreferences,
} from "../lib/bridge";
import { checkForAppUpdate, openProjectPage } from "../lib/appUpdate";
import { normalizeGlassStyle } from "../lib/glass";
import { copy } from "../lib/i18n";
import type { AppearancePreference, CustomSkinMetadata, GlassStyle, Language, PlatformCapabilities, SkinTextTone, WidgetPreferences, WidgetSize } from "../types";

const DEFAULT_PREFERENCES: WidgetPreferences = {
  locked: false,
  alwaysOnTop: true,
  widgetMode: "compact",
  widgetSize: "medium",
  compactSize: 72,
  expandedSize: 306,
  toggleCorner: "ne",
  pinnedProvider: null,
  autoRotateSeconds: 12,
  autoCheckUpdates: true,
  language: "en",
  appearance: "system",
  selectedSkin: "glass",
  glassStyle: "dock",
  customSkins: [],
};

type Section = "general" | "widget" | "appearance" | "updates";
type Feedback = { kind: "success" | "error"; message: string } | null;

const localized = {
  en: {
    title: "Settings",
    nav: "Settings sections",
    sections: { general: "General", widget: "Widget", appearance: "Appearance", updates: "Version & Updates", skins: "Skins" },
    themeSection: "Theme",
    language: "Language",
    english: "English",
    chinese: "简体中文",
    launch: "Launch at login",
    launchHint: "Open Quota Pro automatically when you sign in.",
    rotation: "Auto-rotation interval",
    seconds: "seconds",
    version: "Current version",
    autoCheckUpdates: "Automatically check for updates",
    autoCheckUpdatesHint: "Check for a new version when Quota Pro starts.",
    checkNow: "Check for updates now",
    checking: "Checking…",
    projectAddress: "Project address",
    clickThrough: "Click-through",
    compactSize: "Compact size",
    expandedSize: "Expanded size",
    pixels: "px",
    presets: "Size presets",
    small: "Small",
    medium: "Medium",
    large: "Large",
    system: "System",
    light: "Light",
    dark: "Dark",
    builtin: "Built-in skins",
    custom: "Custom skins",
    default: "Soft Light",
    computer: "Computer",
    glass: "Default",
    glassStyle: "Glass material",
    glassTransparent: "Transparent",
    glassDock: "Dock frosted glass",
    glassLiquid: "Liquid Glass",
    liquidUnavailable: "Requires macOS 26",
    import: "Import custom skin",
    importButton: "Import image",
    noCustom: "Import a PNG, JPEG, or WebP image to create a custom skin.",
    skinName: "Skin name",
    textTone: "Text tone",
    auto: "Auto",
    accent: "Accent color",
    delete: (name: string) => `Delete ${name}`,
    confirmDelete: (name: string) => `Delete “${name}”? This cannot be undone.`,
    saved: "Updated",
    imported: "Custom skin imported",
    deleted: "Custom skin deleted",
    failed: "The change could not be applied.",
    importUnavailable: "Custom skin import is unavailable in browser preview.",
    importTooLarge: "Image must be 10 MiB or smaller.",
  },
  "zh-CN": {
    title: "设置",
    nav: "设置分类",
    sections: { general: "通用", widget: "悬浮窗", appearance: "外观", updates: "版本与更新", skins: "皮肤" },
    themeSection: "主题",
    language: "语言",
    english: "English",
    chinese: "简体中文",
    launch: "登录时启动",
    launchHint: "登录系统后自动打开 Quota Pro。",
    rotation: "自动轮换间隔",
    seconds: "秒",
    version: "当前版本",
    autoCheckUpdates: "自动检测更新",
    autoCheckUpdatesHint: "启动 Quota Pro 时检查是否有新版本。",
    checkNow: "立即检查更新",
    checking: "检查中…",
    projectAddress: "项目地址",
    clickThrough: "鼠标穿透",
    compactSize: "收起尺寸",
    expandedSize: "展开尺寸",
    pixels: "像素",
    presets: "尺寸预设",
    small: "小",
    medium: "中",
    large: "大",
    system: "跟随系统",
    light: "浅色",
    dark: "深色",
    builtin: "内置皮肤",
    custom: "自定义皮肤",
    default: "柔光",
    computer: "电脑",
    glass: "默认",
    glassStyle: "玻璃材质",
    glassTransparent: "透明",
    glassDock: "Dock 毛玻璃",
    glassLiquid: "Liquid Glass",
    liquidUnavailable: "需要 macOS 26",
    import: "导入自定义皮肤",
    importButton: "导入图片",
    noCustom: "导入 PNG、JPEG 或 WebP 图片来创建自定义皮肤。",
    skinName: "皮肤名称",
    textTone: "文字色调",
    auto: "自动",
    accent: "强调色",
    delete: (name: string) => `删除 ${name}`,
    confirmDelete: (name: string) => `删除“${name}”？此操作无法撤销。`,
    saved: "已更新",
    imported: "自定义皮肤已导入",
    deleted: "自定义皮肤已删除",
    failed: "无法应用此更改。",
    importUnavailable: "浏览器预览中无法导入自定义皮肤。",
    importTooLarge: "图片大小不能超过 10 MiB。",
  },
} as const;

function errorMessage(error: unknown, fallback: string) {
  return error instanceof Error && error.message ? error.message : fallback;
}

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

function preferencesEqual(left: WidgetPreferences, right: WidgetPreferences) {
  return JSON.stringify(left) === JSON.stringify(right);
}

function normalizeReceivedPreferences(value: Partial<WidgetPreferences> & { glassBlur?: unknown }): WidgetPreferences {
  const normalized = {
    ...DEFAULT_PREFERENCES,
    ...value,
    autoCheckUpdates: typeof value.autoCheckUpdates === "boolean" ? value.autoCheckUpdates : true,
    glassStyle: normalizeGlassStyle(value),
    customSkins: value.customSkins ?? [],
  };
  delete (normalized as WidgetPreferences & { glassBlur?: unknown }).glassBlur;
  return normalized;
}

function CustomSkinEditor({
  skin,
  selected,
  assetUrl,
  labels,
  onSelect,
  onUpdate,
  onDelete,
}: {
  skin: CustomSkinMetadata;
  selected: boolean;
  assetUrl?: string;
  labels: typeof localized.en;
  onSelect: () => void;
  onUpdate: (patch: { name?: string; textTone?: SkinTextTone; accentColor?: string }) => void;
  onDelete: () => void;
}) {
  const [name, setName] = useState(skin.name);
  useEffect(() => setName(skin.name), [skin.name]);
  const effectiveTone = skin.textTone === "auto" ? skin.detectedTone : skin.textTone;
  const previewStyle = assetUrl ? {
    backgroundImage: `url(${assetUrl})`,
    backgroundSize: "cover",
    backgroundPosition: "center",
    "--skin-preview-accent": skin.accentColor,
  } as CSSProperties : { "--skin-preview-accent": skin.accentColor } as CSSProperties;

  return <article className="custom-skin-editor">
    <label className="skin-choice skin-choice--custom">
      <input type="radio" name="skin" checked={selected} onChange={onSelect} aria-label={skin.name} />
      <span
        className={`skin-preview skin-preview--custom skin-preview--text-${effectiveTone}`}
        data-testid={`skin-preview-${skin.id}`}
        style={previewStyle}
      >
        <span>{selected ? <Check weight="bold" /> : null}</span>
      </span>
      <span>{skin.name}</span>
    </label>
    <div className="custom-skin-fields">
      <label>{labels.skinName}<input aria-label={labels.skinName} value={name} onChange={(event) => setName(event.target.value)} onBlur={() => {
        const trimmed = name.trim();
        if (trimmed && trimmed !== skin.name) onUpdate({ name: trimmed });
        else setName(skin.name);
      }} /></label>
      <label>{labels.textTone}<select aria-label={labels.textTone} value={skin.textTone} onChange={(event) => onUpdate({ textTone: event.target.value as SkinTextTone })}>
        <option value="auto">{labels.auto}</option><option value="light">{labels.light}</option><option value="dark">{labels.dark}</option>
      </select></label>
      <label>{labels.accent}<input type="color" aria-label={labels.accent} value={skin.accentColor} onChange={(event) => onUpdate({ accentColor: event.target.value.toUpperCase() })} /></label>
      <button type="button" className="settings-danger" aria-label={labels.delete(skin.name)} onClick={onDelete}><Trash weight="bold" /></button>
    </div>
  </article>;
}

export function SettingsPanel() {
  const [preferences, setPreferences] = useState(DEFAULT_PREFERENCES);
  const [section, setSection] = useState<Section>("general");
  const [launchAtLogin, setLaunchState] = useState<boolean | null>(null);
  const [appVersion, setAppVersion] = useState("—");
  const [updateChecking, setUpdateChecking] = useState(false);
  const [assets, setAssets] = useState<Record<string, string>>({});
  const [feedback, setFeedback] = useState<Feedback>(null);
  const [loaded, setLoaded] = useState(false);
  const [preferencesReady, setPreferencesReady] = useState(false);
  const [platformCapabilities, setPlatformCapabilities] = useState<PlatformCapabilities>({ nativeGlass: false, supportsLiquidGlass: false });
  const preferencesRef = useRef(preferences);
  const committedPreferencesRef = useRef(preferences);
  const deferredPreferencesRef = useRef<WidgetPreferences | null>(null);
  const customSkinDraftsRef = useRef(new Map<string, CustomSkinMetadata>());
  const pendingPreferenceMutations = useRef(0);
  const preferenceWriteEpoch = useRef(0);
  const sizeDraftRef = useRef({ compactSize: preferences.compactSize, expandedSize: preferences.expandedSize });
  const preferenceMutationQueue = useRef(Promise.resolve());
  const [systemDark, setSystemDark] = useState(() => window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? false);
  const language: Language = preferences.language === "zh-CN" ? "zh-CN" : "en";
  const t = localized[language];
  const theme = preferences.appearance === "system" ? (systemDark ? "dark" : "light") : preferences.appearance;

  useEffect(() => {
    preferencesRef.current = preferences;
    sizeDraftRef.current = { compactSize: preferences.compactSize, expandedSize: preferences.expandedSize };
    const currentIds = new Set(preferences.customSkins.map((skin) => skin.id));
    preferences.customSkins.forEach((skin) => customSkinDraftsRef.current.set(skin.id, skin));
    for (const id of customSkinDraftsRef.current.keys()) if (!currentIds.has(id)) customSkinDraftsRef.current.delete(id);
  }, [preferences]);

  const enqueuePreferenceMutation = useCallback(<T,>(operation: () => Promise<T>): Promise<T> => {
    pendingPreferenceMutations.current += 1;
    const execute = async () => {
      try { return await operation(); }
      finally { pendingPreferenceMutations.current = Math.max(0, pendingPreferenceMutations.current - 1); }
    };
    const run = preferenceMutationQueue.current.then(execute, execute);
    preferenceMutationQueue.current = run.then(() => undefined, () => undefined);
    return run;
  }, []);

  useEffect(() => {
    let cancelled = false;
    const preferencesRequest = getPreferences().then((next) => {
      if (cancelled) return false;
      const normalized = normalizeReceivedPreferences(next);
      preferencesRef.current = normalized;
      committedPreferencesRef.current = normalized;
      setPreferences(normalized);
      setPreferencesReady(true);
      return true;
    }).catch((error) => {
      if (!cancelled) setFeedback({ kind: "error", message: errorMessage(error, localized.en.failed) });
      return false;
    });
    const launchRequest = getLaunchAtLogin().then((launch) => {
      if (!cancelled) setLaunchState(launch);
    }).catch((error) => {
      if (!cancelled) { setLaunchState(null); setFeedback({ kind: "error", message: errorMessage(error, localized.en.failed) }); }
    });
    const capabilitiesRequest = getPlatformCapabilities().then((capabilities) => {
      if (!cancelled) setPlatformCapabilities(capabilities);
    }).catch(() => undefined);
    void Promise.all([preferencesRequest, launchRequest, capabilitiesRequest]).then(([ready]) => {
      if (!cancelled) { setPreferencesReady((current) => current || ready); setLoaded(true); }
    });
    let cleanup: () => void = () => undefined;
    void listenDesktopEvents({
      onPreferences: (next) => {
        const normalized = normalizeReceivedPreferences(next);
        committedPreferencesRef.current = normalized;
        setPreferencesReady(true);
        // A native command emits this event while its promise is still
        // settling. Keep newer optimistic edits visible until the queued
        // command completions have reconciled them.
        if (pendingPreferenceMutations.current === 0) {
          preferenceWriteEpoch.current += 1;
          preferencesRef.current = normalized;
          setPreferences(normalized);
        } else {
          deferredPreferencesRef.current = normalized;
        }
      },
      onRefresh: () => undefined,
      onUpdate: () => undefined,
      onLaunchAtLogin: setLaunchState,
    }).then((dispose) => { if (cancelled) dispose(); else cleanup = dispose; }).catch(() => undefined);
    return () => { cancelled = true; cleanup(); };
  }, []);

  useEffect(() => {
    const media = window.matchMedia?.("(prefers-color-scheme: dark)");
    if (!media) return;
    const change = () => setSystemDark(media.matches);
    media.addEventListener?.("change", change);
    return () => media.removeEventListener?.("change", change);
  }, []);

  useEffect(() => {
    let cancelled = false;
    void getAppVersion().then((version) => {
      if (!cancelled) setAppVersion(version);
    }).catch(() => undefined);
    return () => { cancelled = true; };
  }, []);

  useEffect(() => {
    let cancelled = false;
    void Promise.all(preferences.customSkins.map(async (skin) => {
      try { return [skin.id, (await getCustomSkinAsset(skin.id))?.dataUrl] as const; }
      catch { return [skin.id, undefined] as const; }
    })).then((values) => {
      if (cancelled) return;
      setAssets(Object.fromEntries(values.filter((entry): entry is readonly [string, string] => Boolean(entry[1]))));
    });
    return () => { cancelled = true; };
  }, [preferences.customSkins]);

  const reconcileDeferredPreferences = (localResult?: WidgetPreferences) => {
    const deferred = deferredPreferencesRef.current;
    if (!deferred) return;
    if (localResult && preferencesEqual(deferred, localResult)) {
      deferredPreferencesRef.current = null;
      return;
    }
    if (pendingPreferenceMutations.current === 0) {
      deferredPreferencesRef.current = null;
      preferenceWriteEpoch.current += 1;
      committedPreferencesRef.current = deferred;
      preferencesRef.current = deferred;
      setPreferences(deferred);
    }
  };

  const applyPreferences = useCallback(async (patch: Partial<Pick<WidgetPreferences, "language" | "autoRotateSeconds" | "autoCheckUpdates" | "appearance" | "glassStyle">>) => {
    const epoch = ++preferenceWriteEpoch.current;
    const optimistic = { ...preferencesRef.current, ...patch };
    preferencesRef.current = optimistic;
    setPreferences(optimistic);
    setFeedback(null);
    try {
      const next = await enqueuePreferenceMutation(async () => {
        const merged = { ...committedPreferencesRef.current, ...patch };
        return (await updatePreferences(merged)) ?? merged;
      });
      reconcileDeferredPreferences(next);
      if (epoch === preferenceWriteEpoch.current) {
        committedPreferencesRef.current = next;
        preferencesRef.current = next;
        setPreferences(next);
        setFeedback({ kind: "success", message: localized[next.language].saved });
      }
    } catch (error) {
      if (epoch !== preferenceWriteEpoch.current) return;
      const rollback = committedPreferencesRef.current;
      preferencesRef.current = rollback;
      setPreferences(rollback);
      setFeedback({ kind: "error", message: errorMessage(error, localized[rollback.language].failed) });
    }
  }, [enqueuePreferenceMutation]);

  const checkNow = useCallback(async () => {
    if (updateChecking) return;
    setUpdateChecking(true);
    setFeedback(null);
    try {
      const messages = {
        checking: copy[language].updateChecking,
        current: copy[language].updateCurrent,
        downloading: copy[language].updateDownloading,
        installing: copy[language].updateInstalling,
        availableWindows: copy[language].updateAvailableWindows,
        availableMac: copy[language].updateAvailableMac,
        failed: copy[language].updateFailed,
      };
      await checkForAppUpdate(language, messages, (message) => {
        if (message) setFeedback({ kind: message === messages.failed ? "error" : "success", message });
      }, true);
    } finally {
      setUpdateChecking(false);
    }
  }, [language, updateChecking]);

  const applyNativePreferences = useCallback(async (
    operation: () => Promise<WidgetPreferences | undefined>,
  ) => {
    const epoch = ++preferenceWriteEpoch.current;
    setFeedback(null);
    try {
      const next = await enqueuePreferenceMutation(operation);
      reconcileDeferredPreferences(next);
      if (next && epoch === preferenceWriteEpoch.current) {
        const normalized = normalizeReceivedPreferences(next);
        committedPreferencesRef.current = normalized;
        preferencesRef.current = normalized;
        setPreferences(normalized);
      }
      if (epoch === preferenceWriteEpoch.current) setFeedback({ kind: "success", message: t.saved });
    } catch (error) {
      if (epoch !== preferenceWriteEpoch.current) return;
      const rollback = committedPreferencesRef.current;
      preferencesRef.current = rollback;
      sizeDraftRef.current = { compactSize: rollback.compactSize, expandedSize: rollback.expandedSize };
      setPreferences(rollback);
      setFeedback({ kind: "error", message: errorMessage(error, localized[rollback.language].failed) });
    }
  }, [enqueuePreferenceMutation, t]);

  const changeSize = (field: "compactSize" | "expandedSize", raw: string) => {
    const parsed = Number(raw);
    if (!Number.isFinite(parsed)) return;
    const next = field === "compactSize" ? clamp(parsed, 48, 144) : clamp(parsed, 220, 460);
    const compactSize = field === "compactSize" ? next : sizeDraftRef.current.compactSize;
    const expandedSize = field === "expandedSize" ? next : sizeDraftRef.current.expandedSize;
    sizeDraftRef.current = { compactSize, expandedSize };
    const optimistic = { ...preferencesRef.current, compactSize, expandedSize, widgetSize: "custom" as const };
    preferencesRef.current = optimistic;
    setPreferences(optimistic);
    void applyNativePreferences(() => setWidgetDimensions(compactSize, expandedSize));
  };

  const importSkin = async (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) return;
    if (file.size > 10 * 1024 * 1024) {
      setFeedback({ kind: "error", message: t.importTooLarge });
      return;
    }
    setFeedback(null);
    try {
      const bytes = new Uint8Array(await file.arrayBuffer());
      const epoch = ++preferenceWriteEpoch.current;
      const metadata = await enqueuePreferenceMutation(() => importCustomSkin(file.name, bytes));
      if (!metadata) throw new Error(t.importUnavailable);
      const optimistic = { ...preferencesRef.current, customSkins: [...preferencesRef.current.customSkins.filter((skin) => skin.id !== metadata.id), metadata] };
      const committed = { ...committedPreferencesRef.current, customSkins: [...committedPreferencesRef.current.customSkins.filter((skin) => skin.id !== metadata.id), metadata] };
      committedPreferencesRef.current = committed;
      reconcileDeferredPreferences(committed);
      customSkinDraftsRef.current.set(metadata.id, metadata);
      if (epoch === preferenceWriteEpoch.current) {
        preferencesRef.current = optimistic;
        setPreferences(optimistic);
      }
      const asset = await getCustomSkinAsset(metadata.id).catch(() => null);
      if (asset) setAssets((current) => ({ ...current, [metadata.id]: asset.dataUrl }));
      if (epoch === preferenceWriteEpoch.current) setFeedback({ kind: "success", message: t.imported });
    } catch (error) { setFeedback({ kind: "error", message: errorMessage(error, t.failed) }); }
  };

  const updateSkin = (skin: CustomSkinMetadata, patch: { name?: string; textTone?: SkinTextTone; accentColor?: string }) => {
    const previous = preferencesRef.current;
    const latest = customSkinDraftsRef.current.get(skin.id) ?? preferencesRef.current.customSkins.find((item) => item.id === skin.id) ?? skin;
    const nextSkin = { ...latest, ...patch };
    customSkinDraftsRef.current.set(skin.id, nextSkin);
    const optimistic = { ...previous, customSkins: previous.customSkins.map((item) => item.id === skin.id ? nextSkin : item) };
    preferencesRef.current = optimistic;
    setPreferences(optimistic);
    void applyNativePreferences(() => updateCustomSkin(skin.id, nextSkin.name, nextSkin.textTone, nextSkin.accentColor));
  };

  const deleteSkin = (skin: CustomSkinMetadata) => {
    if (!window.confirm(t.confirmDelete(skin.name))) return;
    const epoch = ++preferenceWriteEpoch.current;
    setFeedback(null);
    void enqueuePreferenceMutation(() => deleteCustomSkin(skin.id)).then((next) => {
      const normalized = normalizeReceivedPreferences(next);
      committedPreferencesRef.current = normalized;
      reconcileDeferredPreferences(normalized);
      if (epoch !== preferenceWriteEpoch.current) return;
      preferencesRef.current = normalized;
      customSkinDraftsRef.current.delete(skin.id);
      setPreferences(normalized);
      setAssets((current) => { const copy = { ...current }; delete copy[skin.id]; return copy; });
      setFeedback({ kind: "success", message: t.deleted });
    }).catch((error) => setFeedback({ kind: "error", message: errorMessage(error, t.failed) }));
  };

  const ready = loaded && preferencesReady;
  const effectiveGlassStyle: GlassStyle = preferences.glassStyle === "liquid" && !platformCapabilities.supportsLiquidGlass
    ? "dock"
    : preferences.glassStyle;

  const sections = useMemo(() => ([
    ["general", t.sections.general, GearSix],
    ["widget", t.sections.widget, Monitor],
    ["appearance", t.sections.appearance, PaintBrush],
    ["updates", t.sections.updates, ArrowClockwise],
  ] as const), [t]);

  return <main className={`settings-panel settings-panel--${theme}`} data-testid="settings-panel" aria-busy={!loaded}>
    <aside className="settings-sidebar">
      <h1>{t.title}</h1>
      <nav aria-label={t.nav}>{sections.map(([id, label, Icon]) => <button key={id} type="button" className={section === id ? "is-active" : ""} aria-current={section === id ? "page" : undefined} onClick={() => setSection(id)}><Icon weight={section === id ? "fill" : "regular"} /><span>{label}</span></button>)}</nav>
    </aside>
    <section className="settings-content">
      <header><h2>{t.sections[section]}</h2>{feedback ? <p className={`settings-feedback settings-feedback--${feedback.kind}`} role={feedback.kind === "error" ? "alert" : "status"}>{feedback.message}</p> : null}</header>

      {section === "general" ? <div className="settings-group">
        <label className="settings-row"><span>{t.language}</span><select disabled={!ready} aria-label={t.language} value={language} onChange={(event) => void applyPreferences({ language: event.target.value as Language })}><option value="en">{t.english}</option><option value="zh-CN">{t.chinese}</option></select></label>
        <label className="settings-row settings-row--switch"><span><strong>{t.launch}</strong><small>{t.launchHint}</small></span><input type="checkbox" aria-label={t.launch} checked={launchAtLogin === true} disabled={launchAtLogin === null || !ready} onChange={(event) => {
          const next = event.target.checked; setLaunchState(next); setFeedback(null);
          void setLaunchAtLogin(next).then((actual) => { setLaunchState(actual); setFeedback({ kind: "success", message: t.saved }); }).catch(async (error) => {
            const actual = await getLaunchAtLogin().catch(() => null);
            setLaunchState(actual);
            setFeedback({ kind: "error", message: errorMessage(error, t.failed) });
          });
        }} /></label>
        <label className="settings-row"><span>{t.rotation}</span><span className="settings-number"><input disabled={!ready} type="number" min={5} max={300} aria-label={t.rotation} value={preferences.autoRotateSeconds} onChange={(event) => void applyPreferences({ autoRotateSeconds: clamp(Number(event.target.value), 5, 300) })} /><small>{t.seconds}</small></span></label>
      </div> : null}

      {section === "widget" ? <>
        <div className="settings-group">
          <label className="settings-row settings-row--switch"><span>{t.clickThrough}</span><input disabled={!ready} type="checkbox" aria-label={t.clickThrough} checked={preferences.locked} onChange={(event) => { const next = event.currentTarget.checked; void applyNativePreferences(() => setClickThrough(next)); }} /></label>
          <label className="settings-row"><span>{t.compactSize}</span><span className="settings-number"><input disabled={!ready} type="number" min={48} max={144} aria-label={t.compactSize} value={preferences.compactSize} onChange={(event) => changeSize("compactSize", event.target.value)} /><small>{t.pixels}</small></span></label>
          <label className="settings-row"><span>{t.expandedSize}</span><span className="settings-number"><input disabled={!ready} type="number" min={220} max={460} aria-label={t.expandedSize} value={preferences.expandedSize} onChange={(event) => changeSize("expandedSize", event.target.value)} /><small>{t.pixels}</small></span></label>
        </div>
        <fieldset className="settings-segmented" disabled={!ready}><legend>{t.presets}</legend>{(["small", "medium", "large"] as WidgetSize[]).map((size) => <button key={size} type="button" className={preferences.widgetSize === size ? "is-active" : ""} onClick={() => void applyNativePreferences(() => setWidgetSize(size))}>{t[size]}</button>)}</fieldset>
      </> : null}

      {section === "updates" ? <div className="settings-updates">
        <div className="settings-group">
          <div className="settings-row"><span>{t.version}</span><strong>v{appVersion}</strong></div>
          <label className="settings-row settings-row--switch"><span><strong>{t.autoCheckUpdates}</strong><small>{t.autoCheckUpdatesHint}</small></span><input disabled={!ready} type="checkbox" aria-label={t.autoCheckUpdates} checked={preferences.autoCheckUpdates} onChange={(event) => void applyPreferences({ autoCheckUpdates: event.currentTarget.checked })} /></label>
        </div>
        <div className="settings-update-actions">
          <button type="button" className="settings-update-button" disabled={!ready || updateChecking} onClick={() => void checkNow()}><ArrowClockwise weight="bold" />{updateChecking ? t.checking : t.checkNow}</button>
          <button type="button" className="settings-update-button settings-update-button--secondary" onClick={() => void openProjectPage()}><GithubLogo weight="bold" />{t.projectAddress}</button>
        </div>
      </div> : null}

      {section === "appearance" ? <div className="appearance-settings">
        <section className="appearance-settings-group appearance-settings-group--theme">
          <h3>{t.themeSection}</h3>
          <fieldset className="appearance-options" disabled={!ready}><legend className="sr-only">{t.themeSection}</legend>{(["system", "light", "dark"] as AppearancePreference[]).map((appearance) => <label key={appearance} className={preferences.appearance === appearance ? "is-active" : ""}><input type="radio" name="appearance" checked={preferences.appearance === appearance} onChange={() => void applyPreferences({ appearance })} /><span className={`appearance-swatch appearance-swatch--${appearance}`} /><strong>{t[appearance]}</strong></label>)}</fieldset>
        </section>
        <section className="appearance-settings-group">
          <h3>{t.sections.skins}</h3>
          <h4>{t.builtin}</h4>
          <div className="skin-grid">{(["glass", "default", "computer"] as const).map((id) => <label className="skin-choice" key={id}><input disabled={!ready} type="radio" name="skin" checked={preferences.selectedSkin === id} onChange={() => void applyNativePreferences(() => selectSkin(id))} aria-label={t[id]} /><span className={`skin-preview skin-preview--${id}`}>{preferences.selectedSkin === id ? <Check weight="bold" /> : null}</span><span>{t[id]}</span></label>)}</div>
        {preferences.selectedSkin === "glass" ? <fieldset className="glass-style-options" disabled={!ready}><legend>{t.glassStyle}</legend>{(["transparent", "dock", "liquid"] as GlassStyle[]).map((style) => {
          const disabled = style === "liquid" && !platformCapabilities.supportsLiquidGlass;
          const label = style === "transparent" ? t.glassTransparent : style === "dock" ? t.glassDock : t.glassLiquid;
          return <label key={style} className={effectiveGlassStyle === style ? "is-active" : ""}><input type="radio" name="glass-style" aria-label={label} checked={effectiveGlassStyle === style} disabled={disabled || !ready} onChange={() => void applyPreferences({ glassStyle: style })} /><span>{label}</span>{disabled ? <small>{t.liquidUnavailable}</small> : null}</label>;
        })}</fieldset> : null}
        </section>
        <div className="custom-skins-heading"><h3>{t.custom}</h3><label className="settings-import"><Image weight="bold" />{t.importButton}<input disabled={!ready} type="file" aria-label={t.import} accept=".png,.jpg,.jpeg,.webp,image/png,image/jpeg,image/webp" onChange={(event) => void importSkin(event)} /></label></div>
        {preferences.customSkins.length ? <div className="custom-skins-list">{preferences.customSkins.map((skin) => <CustomSkinEditor key={skin.id} skin={skin} selected={preferences.selectedSkin === `custom:${skin.id}`} assetUrl={assets[skin.id]} labels={t as typeof localized.en} onSelect={() => void applyNativePreferences(() => selectSkin(`custom:${skin.id}`))} onUpdate={(patch) => updateSkin(skin, patch)} onDelete={() => deleteSkin(skin)} />)}</div> : <p className="settings-empty">{t.noCustom}</p>}
      </div> : null}
    </section>
  </main>;
}
