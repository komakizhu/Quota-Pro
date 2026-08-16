export type ProviderId = "codex" | "claude";
export type SnapshotStatus = "ok" | "stale" | "loading" | "unavailable" | "signed_out";
export type Language = "zh-CN" | "en";
export type WidgetTheme = "light" | "dark";
export type AppearancePreference = "system" | WidgetTheme;
export type BuiltinSkin = "default" | "computer" | "glass";
export type WidgetSkin = BuiltinSkin;
export type GlassStyle = "transparent" | "dock" | "liquid";
export type SkinTextTone = "auto" | "light" | "dark";
export type WidgetMode = "compact" | "expanded";
export type ToggleCorner = "nw" | "ne" | "sw" | "se";
export type WidgetSize = "small" | "medium" | "large" | "custom";

export interface CustomSkinMetadata {
  id: string;
  name: string;
  fileName: string;
  detectedTone: "light" | "dark";
  textTone: SkinTextTone;
  accentColor: string;
}

export interface CustomSkinAsset {
  id: string;
  dataUrl: string;
}

export interface PlatformCapabilities {
  nativeGlass: boolean;
  supportsLiquidGlass: boolean;
}

export interface UsageWindow {
  remainingPercent: number;
  resetsAt: string | null;
  windowSeconds: number;
}

export interface ProviderSnapshot {
  provider: ProviderId;
  displayName: string;
  plan: string | null;
  shortWindow: UsageWindow | null;
  weeklyWindow: UsageWindow | null;
  resetCredits: number | null;
  resetCreditExpiresAt?: string[];
  updatedAt: string;
  status: SnapshotStatus;
  message: string | null;
}

export interface WidgetPreferences {
  locked: boolean;
  alwaysOnTop: boolean;
  widgetMode: WidgetMode;
  widgetSize: WidgetSize;
  compactSize: number;
  expandedSize: number;
  toggleCorner: ToggleCorner;
  pinnedProvider: ProviderId | null;
  autoRotateSeconds: number;
  autoCheckUpdates: boolean;
  language: Language;
  appearance: AppearancePreference;
  selectedSkin: string;
  glassStyle: GlassStyle;
  customSkins: CustomSkinMetadata[];
}
