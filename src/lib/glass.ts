import type { GlassStyle } from "../types";

type LegacyGlassPreferences = {
  glassStyle?: unknown;
  glassBlur?: unknown;
};

export function normalizeGlassStyle(value: LegacyGlassPreferences): GlassStyle {
  if (value.glassStyle === "transparent" || value.glassStyle === "dock" || value.glassStyle === "liquid") {
    return value.glassStyle;
  }
  if (value.glassStyle !== undefined) return "dock";
  return value.glassBlur === "light" ? "transparent" : "dock";
}
