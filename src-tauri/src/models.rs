use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetPreferences {
    pub locked: bool,
    #[serde(default = "default_always_on_top")]
    pub always_on_top: bool,
    #[serde(default)]
    pub widget_mode: String,
    #[serde(default = "default_widget_size")]
    pub widget_size: String,
    #[serde(default, skip_serializing)]
    pub stay_expanded: bool,
    pub pinned_provider: Option<String>,
    pub auto_rotate_seconds: u64,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_appearance")]
    pub appearance: String,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub licenses: Vec<String>,
    #[serde(default)]
    pub unlocked_skin: Option<String>,
    #[serde(default)]
    pub unlocked_skins: Vec<String>,
    #[serde(default = "default_skin")]
    pub selected_skin: String,
    #[serde(default)]
    pub supporter_prompt_first_seen_at: Option<String>,
    #[serde(default)]
    pub supporter_prompt_shown_at: Option<String>,
}

fn default_always_on_top() -> bool {
    true
}
fn default_language() -> String {
    "zh-CN".into()
}
fn default_appearance() -> String {
    "light".into()
}
fn default_skin() -> String {
    "default".into()
}
fn default_widget_size() -> String {
    "medium".into()
}

impl Default for WidgetPreferences {
    fn default() -> Self {
        Self {
            locked: false,
            always_on_top: true,
            widget_mode: "compact".into(),
            widget_size: default_widget_size(),
            stay_expanded: false,
            pinned_provider: None,
            auto_rotate_seconds: 12,
            language: default_language(),
            appearance: default_appearance(),
            license: None,
            licenses: Vec::new(),
            unlocked_skin: None,
            unlocked_skins: Vec::new(),
            selected_skin: default_skin(),
            supporter_prompt_first_seen_at: None,
            supporter_prompt_shown_at: None,
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
        if !matches!(self.widget_size.as_str(), "small" | "medium" | "large") {
            self.widget_size = default_widget_size();
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
        if self.licenses.is_empty() {
            if let Some(legacy) = self.license.take() {
                self.licenses.push(legacy);
            }
        }
        self.licenses.retain(|license| !license.trim().is_empty());
        self.licenses.sort();
        self.licenses.dedup();
        if self.unlocked_skins.is_empty() {
            if let Some(legacy) = self.unlocked_skin.take() {
                self.unlocked_skins.push(legacy);
            }
        }
        self.unlocked_skins
            .retain(|skin| matches!(skin.as_str(), "blur" | "computer"));
        self.unlocked_skins.sort();
        self.unlocked_skins.dedup();
        if !matches!(self.selected_skin.as_str(), "default" | "blur" | "computer") {
            self.selected_skin = default_skin();
        }
        if self.selected_skin != "default"
            && !self
                .unlocked_skins
                .iter()
                .any(|skin| skin == &self.selected_skin)
        {
            self.selected_skin = default_skin();
        }
        // Keep the legacy fields populated for pre-migration renderer payloads.
        self.license = self.licenses.first().cloned();
        self.unlocked_skin = self.unlocked_skins.first().cloned();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::WidgetPreferences;

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
}
