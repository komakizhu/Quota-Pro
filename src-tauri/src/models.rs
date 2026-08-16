use serde::{Deserialize, Serialize};
use std::collections::HashSet;

const DEFAULT_CUSTOM_SKIN_ACCENT: &str = "#5A90D6";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    pub remaining_percent: f64,
    pub resets_at: Option<String>,
    pub window_seconds: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSnapshot {
    pub provider: String,
    pub display_name: String,
    pub plan: Option<String>,
    pub short_window: Option<UsageWindow>,
    pub weekly_window: Option<UsageWindow>,
    pub reset_credits: Option<u64>,
    pub reset_credit_expires_at: Vec<String>,
    pub updated_at: String,
    pub status: String,
    pub message: Option<String>,
}

impl ProviderSnapshot {
    pub fn failure(status: &str, message: &str) -> Self {
        Self {
            provider: "codex".into(),
            display_name: "CODEX".into(),
            plan: None,
            short_window: None,
            weekly_window: None,
            reset_credits: None,
            reset_credit_expires_at: Vec::new(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            status: status.into(),
            message: Some(message.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomSkinMetadata {
    pub id: String,
    pub name: String,
    pub file_name: String,
    pub detected_tone: String,
    pub text_tone: String,
    pub accent_color: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetPreferences {
    pub locked: bool,
    #[serde(default = "default_always_on_top")]
    pub always_on_top: bool,
    #[serde(default)]
    pub widget_mode: String,
    #[serde(default = "default_widget_size")]
    pub widget_size: String,
    #[serde(default = "default_missing_size")]
    pub compact_size: f64,
    #[serde(default = "default_missing_size")]
    pub expanded_size: f64,
    #[serde(default = "default_toggle_corner")]
    pub toggle_corner: String,
    #[serde(default, skip_serializing)]
    pub stay_expanded: bool,
    pub pinned_provider: Option<String>,
    pub auto_rotate_seconds: u64,
    #[serde(default = "default_auto_check_updates")]
    pub auto_check_updates: bool,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_appearance")]
    pub appearance: String,
    #[serde(default = "default_skin")]
    pub selected_skin: String,
    #[serde(default = "missing_glass_style")]
    pub glass_style: String,
    #[serde(default, skip_serializing)]
    pub glass_blur: Option<String>,
    #[serde(default)]
    pub custom_skins: Vec<CustomSkinMetadata>,
}

fn default_always_on_top() -> bool {
    true
}
fn default_language() -> String {
    "zh-CN".into()
}
fn default_auto_check_updates() -> bool {
    true
}
fn default_appearance() -> String {
    "light".into()
}
fn default_skin() -> String {
    "glass".into()
}
fn default_glass_style() -> String {
    "dock".into()
}
fn missing_glass_style() -> String {
    String::new()
}
fn default_widget_size() -> String {
    "medium".into()
}
fn default_missing_size() -> f64 {
    0.0
}
fn default_compact_size() -> f64 {
    72.0
}
fn default_expanded_size() -> f64 {
    306.0
}
fn default_toggle_corner() -> String {
    "ne".into()
}

fn normalize_accent_color(value: &str) -> String {
    let value = value.trim();
    if value.len() == 7
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        value.to_ascii_uppercase()
    } else {
        DEFAULT_CUSTOM_SKIN_ACCENT.into()
    }
}

pub(crate) fn valid_custom_skin_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 96
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

pub(crate) fn custom_skin_file_name(id: &str) -> String {
    format!("{id}.png")
}

fn preset_factor(widget_size: &str) -> f64 {
    match widget_size {
        "small" => 0.84,
        "large" => 1.16,
        _ => 1.0,
    }
}

impl Default for WidgetPreferences {
    fn default() -> Self {
        Self {
            locked: false,
            always_on_top: true,
            widget_mode: "compact".into(),
            widget_size: default_widget_size(),
            compact_size: default_compact_size(),
            expanded_size: default_expanded_size(),
            toggle_corner: default_toggle_corner(),
            stay_expanded: false,
            pinned_provider: None,
            auto_rotate_seconds: 12,
            auto_check_updates: default_auto_check_updates(),
            language: default_language(),
            appearance: default_appearance(),
            selected_skin: default_skin(),
            glass_style: default_glass_style(),
            glass_blur: None,
            custom_skins: Vec::new(),
        }
    }
}

impl WidgetPreferences {
    pub fn normalized(mut self) -> Self {
        if self.widget_mode != "compact" && self.widget_mode != "expanded" {
            self.widget_mode = if self.stay_expanded {
                "expanded"
            } else {
                "compact"
            }
            .into();
        }
        if !matches!(
            self.widget_size.as_str(),
            "small" | "medium" | "large" | "custom"
        ) {
            self.widget_size = default_widget_size();
        }
        let factor = preset_factor(&self.widget_size);
        if !self.compact_size.is_finite() || self.compact_size <= 0.0 {
            self.compact_size = default_compact_size() * factor;
        }
        if !self.expanded_size.is_finite() || self.expanded_size <= 0.0 {
            self.expanded_size = default_expanded_size() * factor;
        }
        self.compact_size = self.compact_size.clamp(48.0, 144.0);
        self.expanded_size = self.expanded_size.clamp(220.0, 460.0);
        if !matches!(self.toggle_corner.as_str(), "nw" | "ne" | "sw" | "se") {
            self.toggle_corner = default_toggle_corner();
        }
        self.stay_expanded = false;
        self.auto_rotate_seconds = self.auto_rotate_seconds.clamp(5, 300);
        if self.pinned_provider.as_deref() != Some("codex") {
            self.pinned_provider = None;
        }
        if self.language != "en" && self.language != "zh-CN" {
            self.language = default_language();
        }
        if self.appearance != "system" && self.appearance != "light" && self.appearance != "dark" {
            self.appearance = default_appearance();
        }
        if self.glass_style.is_empty() {
            self.glass_style = match self.glass_blur.as_deref() {
                Some("light") => "transparent".into(),
                Some("medium" | "heavy") => "dock".into(),
                _ => default_glass_style(),
            };
        } else if !matches!(self.glass_style.as_str(), "transparent" | "dock" | "liquid") {
            self.glass_style = default_glass_style();
        }
        self.glass_blur = None;
        let mut known_custom_ids = HashSet::new();
        self.custom_skins.retain_mut(|metadata| {
            metadata.id = metadata.id.trim().into();
            if !valid_custom_skin_id(&metadata.id)
                || metadata.file_name != custom_skin_file_name(&metadata.id)
                || !known_custom_ids.insert(metadata.id.clone())
            {
                return false;
            }
            if !matches!(metadata.detected_tone.as_str(), "light" | "dark") {
                metadata.detected_tone = "dark".into();
            }
            if !matches!(metadata.text_tone.as_str(), "auto" | "light" | "dark") {
                metadata.text_tone = "auto".into();
            }
            metadata.accent_color = normalize_accent_color(&metadata.accent_color);
            true
        });
        if self.selected_skin == "blur" {
            self.selected_skin = "default".into();
        }
        let selected_custom_id = self.selected_skin.strip_prefix("custom:");
        let selected_skin_is_valid = matches!(
            self.selected_skin.as_str(),
            "default" | "computer" | "glass"
        ) || selected_custom_id.is_some_and(|selected_id| {
            !selected_id.is_empty()
                && self
                    .custom_skins
                    .iter()
                    .any(|metadata| metadata.id == selected_id)
        });
        if !selected_skin_is_valid {
            self.selected_skin = default_skin();
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::WidgetPreferences;
    use serde_json::json;

    fn legacy_preferences(selected_skin: &str) -> serde_json::Value {
        json!({
            "locked": false,
            "pinnedProvider": null,
            "autoRotateSeconds": 12,
            "selectedSkin": selected_skin
        })
    }

    #[test]
    fn legacy_builtin_skin_selections_remain_selected_without_unlocks() {
        for selected_skin in ["computer", "glass"] {
            let parsed: WidgetPreferences =
                serde_json::from_value(legacy_preferences(selected_skin))
                    .expect("legacy preferences should deserialize");
            assert_eq!(parsed.normalized().selected_skin, selected_skin);
        }
    }

    #[test]
    fn obsolete_supporter_fields_are_ignored_and_not_serialized() {
        let mut raw = legacy_preferences("blur");
        let object = raw
            .as_object_mut()
            .expect("test fixture should be an object");
        object.insert("license".into(), json!("legacy-license"));
        object.insert("licenses".into(), json!(["legacy-license"]));
        object.insert("unlockedSkin".into(), json!("blur"));
        object.insert("unlockedSkins".into(), json!(["blur", "computer"]));
        object.insert(
            "supporterPromptFirstSeenAt".into(),
            json!("2026-01-01T00:00:00Z"),
        );
        object.insert(
            "supporterPromptShownAt".into(),
            json!("2026-01-04T00:00:00Z"),
        );

        let parsed: WidgetPreferences =
            serde_json::from_value(raw).expect("obsolete fields should deserialize harmlessly");
        let saved =
            serde_json::to_value(parsed.normalized()).expect("preferences should serialize");

        for key in [
            "license",
            "licenses",
            "unlockedSkin",
            "unlockedSkins",
            "supporterPromptFirstSeenAt",
            "supporterPromptShownAt",
        ] {
            assert!(
                saved.get(key).is_none(),
                "obsolete field {key} was serialized"
            );
        }
        assert_eq!(saved["selectedSkin"], "default");
    }

    #[test]
    fn custom_skin_selection_requires_matching_metadata() {
        let mut valid = legacy_preferences("custom:lake");
        valid
            .as_object_mut()
            .expect("test fixture should be an object")
            .insert(
                "customSkins".into(),
                json!([{
                    "id": "lake",
                    "name": "Lake",
                    "fileName": "lake.png",
                    "detectedTone": "dark",
                    "textTone": "auto",
                    "accentColor": "#3677c8"
                }]),
            );
        let parsed: WidgetPreferences =
            serde_json::from_value(valid).expect("custom metadata should deserialize");
        assert_eq!(parsed.normalized().selected_skin, "custom:lake");

        for selected_skin in ["custom:missing", "custom:", "unknown"] {
            let parsed: WidgetPreferences =
                serde_json::from_value(legacy_preferences(selected_skin))
                    .expect("invalid selection should deserialize");
            assert_eq!(parsed.normalized().selected_skin, "glass");
        }

        let parsed: WidgetPreferences = serde_json::from_value(json!({
            "locked": false,
            "pinnedProvider": null,
            "autoRotateSeconds": 12
        }))
        .expect("missing skin selection should deserialize");
        assert_eq!(parsed.normalized().selected_skin, "glass");
    }

    #[test]
    fn obsolete_blur_skin_selection_migrates_to_default_skin() {
        let parsed: WidgetPreferences = serde_json::from_value(legacy_preferences("blur"))
            .expect("legacy preferences should deserialize");
        assert_eq!(parsed.normalized().selected_skin, "default");
    }

    #[test]
    fn legacy_glass_blur_migrates_to_the_new_material_styles() {
        for (legacy, expected) in [
            ("light", "transparent"),
            ("medium", "dock"),
            ("heavy", "dock"),
            ("unknown", "dock"),
        ] {
            let mut raw = legacy_preferences("glass");
            raw.as_object_mut()
                .expect("test fixture should be an object")
                .insert("glassBlur".into(), json!(legacy));
            let parsed: WidgetPreferences = serde_json::from_value(raw).unwrap();
            assert_eq!(parsed.normalized().glass_style, expected);
        }
    }

    #[test]
    fn glass_style_defaults_to_dock_and_rejects_unknown_values() {
        let parsed: WidgetPreferences =
            serde_json::from_value(legacy_preferences("glass")).unwrap();
        let saved = serde_json::to_value(parsed.normalized()).unwrap();
        assert_eq!(saved["glassStyle"], "dock");
        assert!(saved.get("glassBlur").is_none());

        for (requested, expected) in [
            ("transparent", "transparent"),
            ("dock", "dock"),
            ("liquid", "liquid"),
            ("unknown", "dock"),
        ] {
            let mut raw = legacy_preferences("glass");
            raw.as_object_mut()
                .expect("test fixture should be an object")
                .insert("glassStyle".into(), json!(requested));
            let parsed: WidgetPreferences = serde_json::from_value(raw).unwrap();
            assert_eq!(parsed.normalized().glass_style, expected);
        }
    }

    #[test]
    fn custom_skin_metadata_is_sanitized_and_deduplicated() {
        let raw = json!({
            "locked": false,
            "pinnedProvider": null,
            "autoRotateSeconds": 12,
            "selectedSkin": "custom:lake",
            "customSkins": [
                {
                    "id": " lake ",
                    "name": "Lake",
                    "fileName": "lake.png",
                    "detectedTone": "unknown",
                    "textTone": "neon",
                    "accentColor": "blue"
                },
                {
                    "id": "lake",
                    "name": "Duplicate",
                    "fileName": "duplicate.png",
                    "detectedTone": "light",
                    "textTone": "dark",
                    "accentColor": "#112233"
                },
                {
                    "id": "",
                    "name": "Empty",
                    "fileName": "empty.png",
                    "detectedTone": "light",
                    "textTone": "auto",
                    "accentColor": "#112233"
                },
                {
                    "id": "custom:nested",
                    "name": "Nested",
                    "fileName": "nested.png",
                    "detectedTone": "dark",
                    "textTone": "light",
                    "accentColor": "#445566"
                },
                {
                    "id": "../escape",
                    "name": "Escape",
                    "fileName": "escape.png",
                    "detectedTone": "dark",
                    "textTone": "auto",
                    "accentColor": "#778899"
                },
                {
                    "id": "folder\\escape",
                    "name": "Escape",
                    "fileName": "escape.png",
                    "detectedTone": "dark",
                    "textTone": "auto",
                    "accentColor": "#778899"
                },
                {
                    "id": "lake.v2",
                    "name": "Noncanonical",
                    "fileName": "lake.v2.png",
                    "detectedTone": "dark",
                    "textTone": "auto",
                    "accentColor": "#778899"
                }
            ]
        });
        let parsed: WidgetPreferences =
            serde_json::from_value(raw).expect("custom metadata should deserialize");
        let normalized = parsed.normalized();

        assert_eq!(normalized.selected_skin, "custom:lake");
        assert_eq!(normalized.custom_skins.len(), 1);
        let metadata = &normalized.custom_skins[0];
        assert_eq!(metadata.id, "lake");
        assert_eq!(metadata.name, "Lake");
        assert_eq!(metadata.detected_tone, "dark");
        assert_eq!(metadata.text_tone, "auto");
        assert_eq!(metadata.accent_color, "#5A90D6");
    }

    #[test]
    fn selected_custom_skin_falls_back_when_its_metadata_is_removed() {
        let mut raw = legacy_preferences("custom:custom-123-abc");
        raw.as_object_mut()
            .expect("test fixture should be an object")
            .insert(
                "customSkins".into(),
                json!([{
                    "id": "custom-123-abc",
                    "name": "Mismatched",
                    "fileName": "another-file.png",
                    "detectedTone": "dark",
                    "textTone": "auto",
                    "accentColor": "#112233"
                }]),
            );
        let parsed: WidgetPreferences =
            serde_json::from_value(raw).expect("invalid metadata should deserialize harmlessly");
        let normalized = parsed.normalized();
        assert!(normalized.custom_skins.is_empty());
        assert_eq!(normalized.selected_skin, "glass");
    }

    #[test]
    fn legacy_persistent_expansion_migrates_to_widget_mode() {
        let mut legacy = WidgetPreferences::default();
        legacy.widget_mode.clear();
        legacy.stay_expanded = true;
        let normalized = legacy.normalized();
        assert_eq!(normalized.widget_mode, "expanded");
        assert!(!normalized.stay_expanded);
    }

    #[test]
    fn invalid_widget_mode_defaults_to_compact() {
        let mut preferences = WidgetPreferences::default();
        preferences.widget_mode = "invalid".into();
        assert_eq!(preferences.normalized().widget_mode, "compact");
    }

    #[test]
    fn invalid_widget_size_defaults_to_medium() {
        let mut preferences = WidgetPreferences::default();
        preferences.widget_size = "invalid".into();
        assert_eq!(preferences.normalized().widget_size, "medium");
    }

    #[test]
    fn legacy_widget_size_migrates_to_independent_dimensions() {
        let mut preferences = WidgetPreferences::default();
        preferences.widget_size = "large".into();
        preferences.compact_size = 0.0;
        preferences.expanded_size = 0.0;
        let normalized = preferences.normalized();
        assert_eq!(normalized.compact_size, 72.0 * 1.16);
        assert_eq!(normalized.expanded_size, 306.0 * 1.16);
    }

    #[test]
    fn serde_migration_fills_dimensions_for_old_preferences() {
        let raw = json!({
            "locked": false,
            "widgetSize": "small",
            "pinnedProvider": null,
            "autoRotateSeconds": 12
        });
        let parsed: WidgetPreferences =
            serde_json::from_value(raw).expect("legacy preferences should deserialize");
        let normalized = parsed.normalized();
        assert_eq!(normalized.compact_size, 72.0 * 0.84);
        assert_eq!(normalized.expanded_size, 306.0 * 0.84);
    }

    #[test]
    fn custom_dimensions_are_clamped_and_custom_marker_is_preserved() {
        let mut preferences = WidgetPreferences::default();
        preferences.widget_size = "custom".into();
        preferences.compact_size = 2.0;
        preferences.expanded_size = 999.0;
        let normalized = preferences.normalized();
        assert_eq!(normalized.widget_size, "custom");
        assert_eq!(normalized.compact_size, 48.0);
        assert_eq!(normalized.expanded_size, 460.0);
    }
}
