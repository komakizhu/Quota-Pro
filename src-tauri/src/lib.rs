mod codex;
mod custom_skins;
mod models;

use std::{
    fs,
    io::Write,
    path::PathBuf,
    sync::{Mutex, MutexGuard},
    time::{Duration, Instant},
};

#[cfg(debug_assertions)]
use models::UsageWindow;
use models::{CustomSkinMetadata, ProviderSnapshot, WidgetPreferences};
use serde::{Deserialize, Serialize};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use tauri::menu::{MenuItemKind, PredefinedMenuItem};
use tauri::{
    image::Image as TauriImage,
    menu::{CheckMenuItem, Menu, MenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    window::Color,
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, State, WindowEvent,
};
use tauri_plugin_autostart::{AutoLaunchManager, MacosLauncher, ManagerExt};
use tauri_plugin_window_state::Builder as WindowStateBuilder;

// These are the default visual dimensions. User resizing stores compact and
// expanded values independently; tray presets derive from these defaults.
const COLLAPSED_LOGICAL_SIZE: f64 = 72.0;
const EXPANDED_LOGICAL_SIZE: f64 = 306.0;
const COMPACT_MIN_LOGICAL_SIZE: f64 = 48.0;
const COMPACT_MAX_LOGICAL_SIZE: f64 = 144.0;
const EXPANDED_MIN_LOGICAL_SIZE: f64 = 220.0;
const EXPANDED_MAX_LOGICAL_SIZE: f64 = 460.0;
const EDGE_SAFE_INSET_LOGICAL: f64 = 4.0;
const POSITION_EPSILON: u32 = 2;
// The dedicated mode button is 25px square at the default 306px visual size.
// Its center inset is kept in native geometry so the button and compact orb
// share one anchor even when the card is resized. The southwest button has a
// larger bottom inset so it clears the fallback `--` metric in the footer.
const TOGGLE_BUTTON_EDGE_INSET_LOGICAL: f64 = 24.0;
const TOGGLE_BUTTON_TOP_EDGE_INSET_LOGICAL: f64 = 30.0;
const TOGGLE_BUTTON_SIZE_LOGICAL: f64 = 25.0;
const TOGGLE_BUTTON_SW_BOTTOM_INSET_LOGICAL: f64 = 56.0;
const TOGGLE_BUTTON_SW_WEEKLY_PRIMARY_BOTTOM_INSET_LOGICAL: f64 = 24.0;
const APP_SETTINGS_MENU_ID: &str = "app-show-settings";
const TRAY_SETTINGS_MENU_ID: &str = "tray-show-settings";
const LAUNCH_AT_LOGIN_CHANGED_EVENT: &str = "launch-at-login-changed";

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlatformCapabilities {
    native_glass: bool,
    supports_liquid_glass: bool,
}

#[cfg(target_os = "macos")]
fn supports_liquid_glass_runtime() -> bool {
    use objc2::runtime::AnyClass;
    use objc2_foundation::{NSOperatingSystemVersion, NSProcessInfo};

    let required = NSOperatingSystemVersion {
        majorVersion: 26,
        minorVersion: 0,
        patchVersion: 0,
    };
    NSProcessInfo::processInfo().isOperatingSystemAtLeastVersion(required)
        && AnyClass::get(c"NSGlassEffectView").is_some()
}

#[cfg(not(target_os = "macos"))]
fn supports_liquid_glass_runtime() -> bool {
    false
}

/// The Dock material is available on every supported macOS release. Keep this
/// separate from the macOS 26-only Liquid Glass capability: the former is the
/// actual backdrop sampler we need on macOS 15, while the latter is an
/// optional newer AppKit view.
#[cfg(target_os = "macos")]
fn supports_native_dock_runtime() -> bool {
    true
}

#[cfg(not(target_os = "macos"))]
fn supports_native_dock_runtime() -> bool {
    false
}

#[tauri::command]
fn get_platform_capabilities() -> PlatformCapabilities {
    PlatformCapabilities {
        // Dock vibrancy is available on all supported macOS releases. Liquid
        // Glass remains separately gated to macOS 26+.
        native_glass: supports_native_dock_runtime(),
        supports_liquid_glass: supports_liquid_glass_runtime(),
    }
}

#[derive(Clone, Copy)]
struct WidgetRect {
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct AppKitFrame {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

fn appkit_frame_from_physical(
    current: WidgetRect,
    current_frame: AppKitFrame,
    target_position: PhysicalPosition<i32>,
    target_size: PhysicalSize<u32>,
    scale_factor: f64,
) -> AppKitFrame {
    let delta_x = (f64::from(target_position.x) - f64::from(current.position.x)) / scale_factor;
    let delta_y = (f64::from(target_position.y) - f64::from(current.position.y)) / scale_factor;
    let width = f64::from(target_size.width) / scale_factor;
    let height = f64::from(target_size.height) / scale_factor;

    AppKitFrame {
        x: current_frame.x + delta_x,
        y: current_frame.y + current_frame.height - delta_y - height,
        width,
        height,
    }
}

#[derive(Clone, Copy, Deserialize)]
struct WorkAreaPoint {
    x: i32,
    y: i32,
}

#[derive(Clone, Copy, Deserialize)]
struct WorkAreaSize {
    width: u32,
    height: u32,
}

#[derive(Clone, Copy, Deserialize)]
struct WorkAreaPayload {
    position: WorkAreaPoint,
    size: WorkAreaSize,
}

#[derive(Clone, Copy)]
enum WidgetMode {
    Collapsed,
    Expanded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToggleCorner {
    NorthWest,
    NorthEast,
    SouthWest,
    SouthEast,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WidgetSize {
    Small,
    Medium,
    Large,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ResizeEdge {
    North,
    South,
    East,
    West,
    NorthEast,
    NorthWest,
    SouthEast,
    SouthWest,
}

#[derive(Clone, Copy)]
struct WidgetGeometryState {
    mode: WidgetMode,
    collapsed_rect: WidgetRect,
    toggle_corner: ToggleCorner,
    southwest_weekly_primary: bool,
}

#[derive(Clone, Copy)]
struct WidgetResizeState {
    mode: WidgetMode,
    edge: ResizeEdge,
    start_rect: WidgetRect,
    start_collapsed_rect: WidgetRect,
    toggle_corner: ToggleCorner,
    southwest_weekly_primary: bool,
    scale_factor: f64,
    safe_inset: u32,
    bounds: Option<(PhysicalPosition<i32>, PhysicalSize<u32>)>,
    #[cfg(target_os = "macos")]
    start_native_frame: AppKitFrame,
}

struct AppState {
    client: reqwest::Client,
    preferences: Mutex<WidgetPreferences>,
    preferences_path: PathBuf,
    fetch_lock: tokio::sync::Mutex<()>,
    snapshot_cache: Mutex<Option<(Instant, Vec<ProviderSnapshot>)>>,
    #[cfg(debug_assertions)]
    simulate_short_window_for_testing: Mutex<bool>,
    geometry: Mutex<Option<WidgetGeometryState>>,
    drag_mode: Mutex<Option<WidgetMode>>,
    resize_state: Mutex<Option<WidgetResizeState>>,
}

struct TrayMenuState {
    autostart: CheckMenuItem<tauri::Wry>,
    size_small: CheckMenuItem<tauri::Wry>,
    size_medium: CheckMenuItem<tauri::Wry>,
    size_large: CheckMenuItem<tauri::Wry>,
    theme_system: CheckMenuItem<tauri::Wry>,
    theme_dark: CheckMenuItem<tauri::Wry>,
    theme_light: CheckMenuItem<tauri::Wry>,
    skin_default: CheckMenuItem<tauri::Wry>,
    skin_computer: CheckMenuItem<tauri::Wry>,
    skin_glass: CheckMenuItem<tauri::Wry>,
}

fn sync_tray_preferences(app: &AppHandle, preferences: &WidgetPreferences) {
    let Some(menu) = app.try_state::<TrayMenuState>() else {
        return;
    };
    let _ = menu
        .size_small
        .set_checked(preferences.widget_size == "small");
    let _ = menu
        .size_medium
        .set_checked(preferences.widget_size == "medium");
    let _ = menu
        .size_large
        .set_checked(preferences.widget_size == "large");
    let _ = menu
        .theme_system
        .set_checked(preferences.appearance == "system");
    let _ = menu
        .theme_dark
        .set_checked(preferences.appearance == "dark");
    let _ = menu
        .theme_light
        .set_checked(preferences.appearance == "light");
    let _ = menu
        .skin_default
        .set_checked(preferences.selected_skin == "default");
    let _ = menu
        .skin_computer
        .set_checked(preferences.selected_skin == "computer");
    let _ = menu
        .skin_glass
        .set_checked(preferences.selected_skin == "glass");
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsMenuRoute {
    Application,
    Tray,
}

fn settings_menu_route(id: &str) -> Option<SettingsMenuRoute> {
    match id {
        APP_SETTINGS_MENU_ID => Some(SettingsMenuRoute::Application),
        TRAY_SETTINGS_MENU_ID => Some(SettingsMenuRoute::Tray),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CloseBehavior {
    Hide,
    Allow,
}

fn close_behavior(window_label: &str) -> CloseBehavior {
    match window_label {
        "widget" | "settings" => CloseBehavior::Hide,
        _ => CloseBehavior::Allow,
    }
}

trait SettingsWindowController {
    fn show_window(&self) -> Result<(), String>;
    fn center_window(&self) -> Result<(), String>;
    fn focus_window(&self) -> Result<(), String>;
}

impl SettingsWindowController for tauri::WebviewWindow {
    fn show_window(&self) -> Result<(), String> {
        self.show()
            .map_err(|error| format!("failed to show settings window: {error}"))
    }

    fn center_window(&self) -> Result<(), String> {
        self.center()
            .map_err(|error| format!("failed to center settings window: {error}"))
    }

    fn focus_window(&self) -> Result<(), String> {
        self.set_focus()
            .map_err(|error| format!("failed to focus settings window: {error}"))
    }
}

fn activate_settings_window(window: &impl SettingsWindowController) -> Result<(), String> {
    window.show_window()?;
    window.center_window()?;
    window.focus_window()
}

trait LaunchAtLoginBackend {
    fn is_enabled(&self) -> Result<bool, String>;
    fn enable(&self) -> Result<(), String>;
    fn disable(&self) -> Result<(), String>;
}

impl LaunchAtLoginBackend for AutoLaunchManager {
    fn is_enabled(&self) -> Result<bool, String> {
        AutoLaunchManager::is_enabled(self)
            .map_err(|error| format!("failed to read launch-at-login state: {error}"))
    }

    fn enable(&self) -> Result<(), String> {
        AutoLaunchManager::enable(self)
            .map_err(|error| format!("failed to enable launch at login: {error}"))
    }

    fn disable(&self) -> Result<(), String> {
        AutoLaunchManager::disable(self)
            .map_err(|error| format!("failed to disable launch at login: {error}"))
    }
}

fn read_launch_at_login(backend: &impl LaunchAtLoginBackend) -> Result<bool, String> {
    backend.is_enabled()
}

fn write_launch_at_login(
    backend: &impl LaunchAtLoginBackend,
    enabled: bool,
) -> Result<bool, String> {
    let current = backend.is_enabled()?;
    if current != enabled {
        if enabled {
            backend.enable()?;
        } else {
            backend.disable()?;
        }
    }
    let actual = backend.is_enabled()?;
    if actual != enabled {
        return Err(format!(
            "launch-at-login state remained {} after requesting {}",
            actual, enabled
        ));
    }
    Ok(actual)
}

#[cfg(test)]
mod native_settings_tests {
    use super::*;
    use std::{cell::Cell, sync::Mutex};

    #[derive(Default)]
    struct FakeLaunchBackend {
        enabled: Cell<bool>,
        fail_read: Cell<bool>,
        fail_write: Cell<bool>,
        ignore_write: Cell<bool>,
    }

    impl LaunchAtLoginBackend for FakeLaunchBackend {
        fn is_enabled(&self) -> Result<bool, String> {
            if self.fail_read.get() {
                Err("read failed".into())
            } else {
                Ok(self.enabled.get())
            }
        }

        fn enable(&self) -> Result<(), String> {
            if self.fail_write.get() {
                return Err("enable failed".into());
            }
            if !self.ignore_write.get() {
                self.enabled.set(true);
            }
            Ok(())
        }

        fn disable(&self) -> Result<(), String> {
            if self.fail_write.get() {
                return Err("disable failed".into());
            }
            if !self.ignore_write.get() {
                self.enabled.set(false);
            }
            Ok(())
        }
    }

    #[derive(Default)]
    struct RecordingSettingsWindow {
        calls: Mutex<Vec<&'static str>>,
        renderer_state: Cell<u32>,
    }

    impl SettingsWindowController for RecordingSettingsWindow {
        fn show_window(&self) -> Result<(), String> {
            self.calls.lock().unwrap().push("show");
            Ok(())
        }

        fn center_window(&self) -> Result<(), String> {
            self.calls.lock().unwrap().push("center");
            Ok(())
        }

        fn focus_window(&self) -> Result<(), String> {
            self.calls.lock().unwrap().push("focus");
            Ok(())
        }
    }

    #[test]
    fn settings_menu_ids_route_only_the_two_native_entrypoints() {
        assert_eq!(
            settings_menu_route(APP_SETTINGS_MENU_ID),
            Some(SettingsMenuRoute::Application)
        );
        assert_eq!(
            settings_menu_route(TRAY_SETTINGS_MENU_ID),
            Some(SettingsMenuRoute::Tray)
        );
        assert_eq!(settings_menu_route("show"), None);
    }

    #[test]
    fn settings_activation_reuses_the_same_window_and_preserves_renderer_state() {
        let window = RecordingSettingsWindow::default();
        window.renderer_state.set(7);

        activate_settings_window(&window).unwrap();
        activate_settings_window(&window).unwrap();

        assert_eq!(window.renderer_state.get(), 7);
        assert_eq!(
            *window.calls.lock().unwrap(),
            ["show", "center", "focus", "show", "center", "focus"]
        );
    }

    #[test]
    fn settings_window_is_precreated_once_and_starts_hidden() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let settings: Vec<_> = config["app"]["windows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|window| window["label"] == "settings")
            .collect();

        assert_eq!(settings.len(), 1);
        assert_eq!(settings[0]["visible"], false);
        assert_eq!(settings[0]["url"], "index.html?settings");
    }

    #[test]
    fn close_policy_hides_widget_and_settings_but_does_not_swallow_other_windows() {
        assert_eq!(close_behavior("settings"), CloseBehavior::Hide);
        assert_eq!(close_behavior("widget"), CloseBehavior::Hide);
        assert_eq!(close_behavior("diagnostics"), CloseBehavior::Allow);
    }

    #[test]
    fn launch_state_helpers_return_the_verified_native_state() {
        let backend = FakeLaunchBackend::default();

        assert!(!read_launch_at_login(&backend).unwrap());
        assert!(write_launch_at_login(&backend, true).unwrap());
        assert!(read_launch_at_login(&backend).unwrap());
        assert!(!write_launch_at_login(&backend, false).unwrap());
    }

    #[test]
    fn launch_state_failures_never_report_the_requested_value_as_applied() {
        let backend = FakeLaunchBackend::default();
        backend.fail_write.set(true);
        assert!(write_launch_at_login(&backend, true).is_err());
        assert!(!backend.enabled.get());

        backend.fail_write.set(false);
        backend.ignore_write.set(true);
        assert!(write_launch_at_login(&backend, true).is_err());
        assert!(!backend.enabled.get());

        backend.fail_read.set(true);
        assert!(read_launch_at_login(&backend).is_err());
    }
}

fn apply_short_window_test_override(
    _state: &AppState,
    #[allow(unused_mut)] mut snapshots: Vec<ProviderSnapshot>,
) -> Vec<ProviderSnapshot> {
    #[cfg(debug_assertions)]
    if _state
        .simulate_short_window_for_testing
        .lock()
        .map(|value| *value)
        .unwrap_or(false)
    {
        for snapshot in &mut snapshots {
            if snapshot.status == "ok" {
                snapshot.short_window = Some(UsageWindow {
                    remaining_percent: 88.0,
                    resets_at: Some((chrono::Utc::now() + chrono::Duration::hours(3)).to_rfc3339()),
                    window_seconds: 18_000,
                });
            }
        }
    }
    snapshots
}

async fn fetch_snapshots_uncached(state: &State<'_, AppState>) -> Vec<ProviderSnapshot> {
    let _guard = state.fetch_lock.lock().await;
    let values = vec![codex::fetch_snapshot(&state.client).await];
    if let Ok(mut cache) = state.snapshot_cache.lock() {
        *cache = Some((Instant::now(), values.clone()));
    }
    apply_short_window_test_override(state.inner(), values)
}

fn load_preferences(path: &PathBuf) -> WidgetPreferences {
    let parse = |candidate: &PathBuf| {
        fs::read_to_string(candidate)
            .ok()
            .and_then(|raw| serde_json::from_str::<WidgetPreferences>(&raw).ok())
    };
    if let Some(value) = parse(path) {
        return value.normalized();
    }
    let backup = path.with_extension("json.bak");
    if let Some(value) = parse(&backup) {
        eprintln!("preferences recovered from backup");
        return value.normalized();
    }
    WidgetPreferences::default()
}

fn preferences_lock(state: &AppState) -> MutexGuard<'_, WidgetPreferences> {
    state.preferences.lock().unwrap_or_else(|poisoned| {
        eprintln!("preferences lock was poisoned; recovering the last in-memory settings");
        poisoned.into_inner()
    })
}

fn persist_preferences(path: &PathBuf, value: &WidgetPreferences) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|_| "failed to create settings directory".to_string())?;
    }
    let serialized =
        serde_json::to_vec_pretty(value).map_err(|_| "failed to serialize settings".to_string())?;
    let temporary = path.with_extension("json.tmp");
    let backup = path.with_extension("json.bak");
    let mut file = fs::File::create(&temporary)
        .map_err(|_| "failed to create temporary settings file".to_string())?;
    file.write_all(&serialized)
        .and_then(|_| file.sync_all())
        .map_err(|_| "failed to write settings".to_string())?;
    if path.exists() {
        let _ = fs::remove_file(&backup);
        fs::rename(path, &backup).map_err(|_| "failed to back up settings".to_string())?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::rename(&backup, path);
        return Err(format!("failed to commit settings: {error}"));
    }
    Ok(())
}

fn load_preferences_with_skin_reconciliation(
    config_dir: &std::path::Path,
    preferences_path: &PathBuf,
) -> WidgetPreferences {
    let backup_path = preferences_path.with_extension("json.bak");
    let needs_canonical_persist = fs::read_to_string(preferences_path)
        .ok()
        .and_then(|raw| serde_json::from_str::<WidgetPreferences>(&raw).ok())
        .map(|raw| raw.clone().normalized() != raw)
        .unwrap_or_else(|| preferences_path.exists() || backup_path.exists());
    let preferences = load_preferences(preferences_path);
    match custom_skins::reconcile_skin_storage(config_dir, &preferences) {
        Ok((reconciled, changed)) => {
            if changed || needs_canonical_persist {
                if let Err(error) = persist_preferences(preferences_path, &reconciled) {
                    eprintln!("failed to persist reconciled custom skins: {error}");
                }
            }
            reconciled
        }
        Err(error) => {
            eprintln!("failed to reconcile custom skin storage: {error}");
            preferences
        }
    }
}

#[cfg(test)]
mod startup_custom_skin_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let suffix = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "quota-float-startup-skins-{}-{suffix}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(path.join("skins")).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn startup_persists_catalog_cleanup_when_a_managed_asset_is_missing() {
        let directory = TestDirectory::new();
        let preferences_path = directory.0.join("preferences.json");
        let id = "custom-100-00000007";
        let preferences = WidgetPreferences {
            selected_skin: format!("custom:{id}"),
            custom_skins: vec![CustomSkinMetadata {
                id: id.into(),
                name: "Missing".into(),
                file_name: models::custom_skin_file_name(id),
                detected_tone: "dark".into(),
                text_tone: "auto".into(),
                accent_color: "#5A90D6".into(),
            }],
            ..WidgetPreferences::default()
        };
        persist_preferences(&preferences_path, &preferences).unwrap();

        let loaded = load_preferences_with_skin_reconciliation(&directory.0, &preferences_path);

        assert_eq!(loaded.selected_skin, "glass");
        assert!(loaded.custom_skins.is_empty());
        let persisted: WidgetPreferences =
            serde_json::from_slice(&fs::read(&preferences_path).unwrap()).unwrap();
        assert_eq!(persisted.selected_skin, "glass");
        assert!(persisted.custom_skins.is_empty());
    }

    #[test]
    fn startup_persists_catalog_entries_rejected_by_canonical_normalization() {
        let directory = TestDirectory::new();
        let preferences_path = directory.0.join("preferences.json");
        let id = "custom-100-00000008";
        let mut raw = serde_json::to_value(WidgetPreferences::default()).unwrap();
        let object = raw.as_object_mut().unwrap();
        object.insert(
            "selectedSkin".into(),
            serde_json::json!(format!("custom:{id}")),
        );
        object.insert(
            "customSkins".into(),
            serde_json::json!([{
                "id": id,
                "name": "Mismatch",
                "fileName": "wrong.png",
                "detectedTone": "dark",
                "textTone": "auto",
                "accentColor": "#5A90D6"
            }]),
        );
        fs::write(&preferences_path, serde_json::to_vec_pretty(&raw).unwrap()).unwrap();
        fs::write(
            directory
                .0
                .join("skins")
                .join(models::custom_skin_file_name(id)),
            b"orphaned by normalization",
        )
        .unwrap();

        let loaded = load_preferences_with_skin_reconciliation(&directory.0, &preferences_path);

        assert_eq!(loaded.selected_skin, "glass");
        assert!(loaded.custom_skins.is_empty());
        let persisted: WidgetPreferences =
            serde_json::from_slice(&fs::read(&preferences_path).unwrap()).unwrap();
        assert_eq!(persisted.selected_skin, "glass");
        assert!(persisted.custom_skins.is_empty());
    }
}

#[tauri::command]
async fn get_snapshots(state: State<'_, AppState>) -> Result<Vec<ProviderSnapshot>, String> {
    const CACHE_TTL: Duration = Duration::from_secs(30);
    if let Ok(cache) = state.snapshot_cache.lock() {
        if let Some((time, values)) = &*cache {
            if time.elapsed() < CACHE_TTL {
                return Ok(apply_short_window_test_override(&state, values.clone()));
            }
        }
    }
    let _guard = match state.fetch_lock.try_lock() {
        Ok(guard) => guard,
        Err(_) => {
            if let Ok(cache) = state.snapshot_cache.lock() {
                if let Some((_, values)) = &*cache {
                    return Ok(apply_short_window_test_override(&state, values.clone()));
                }
            }
            return Ok(vec![ProviderSnapshot::failure(
                "unavailable",
                "Quota refresh is already running.",
            )]);
        }
    };
    if let Ok(cache) = state.snapshot_cache.lock() {
        if let Some((time, values)) = &*cache {
            if time.elapsed() < CACHE_TTL {
                return Ok(apply_short_window_test_override(&state, values.clone()));
            }
        }
    }
    let values = vec![codex::fetch_snapshot(&state.client).await];
    if let Ok(mut cache) = state.snapshot_cache.lock() {
        *cache = Some((Instant::now(), values.clone()));
    }
    Ok(apply_short_window_test_override(&state, values))
}

#[tauri::command]
async fn refresh_snapshots(state: State<'_, AppState>) -> Result<Vec<ProviderSnapshot>, String> {
    Ok(fetch_snapshots_uncached(&state).await)
}

fn logical_to_physical(value: f64, scale_factor: f64) -> u32 {
    (value * scale_factor).round().max(1.0) as u32
}

fn safe_inset_for_current_appearance(state: &AppState, scale_factor: f64) -> u32 {
    let _ = state;
    logical_to_physical(EDGE_SAFE_INSET_LOGICAL, scale_factor)
}

fn window_size_for_visual_size(visual_size: u32, safe_inset: u32) -> u32 {
    visual_size + safe_inset * 2
}

fn widget_window_size(logical_visual_size: f64, scale_factor: f64, safe_inset: u32) -> u32 {
    window_size_for_visual_size(
        logical_to_physical(logical_visual_size, scale_factor),
        safe_inset,
    )
}

fn visual_size(window_size: PhysicalSize<u32>, safe_inset: u32) -> f64 {
    window_size
        .width
        .saturating_sub(safe_inset.saturating_mul(2)) as f64
}

fn compact_center_offset(
    compact_size: PhysicalSize<u32>,
    safe_inset: u32,
) -> PhysicalPosition<i32> {
    let visual = visual_size(compact_size, safe_inset);
    PhysicalPosition::new(
        (safe_inset as f64 + visual / 2.0).round() as i32,
        (safe_inset as f64 + visual / 2.0).round() as i32,
    )
}

fn toggle_corner_from_preference(value: &str) -> ToggleCorner {
    match value {
        "nw" => ToggleCorner::NorthWest,
        "sw" => ToggleCorner::SouthWest,
        "se" => ToggleCorner::SouthEast,
        _ => ToggleCorner::NorthEast,
    }
}

fn toggle_corner_preference(corner: ToggleCorner) -> &'static str {
    match corner {
        ToggleCorner::NorthWest => "nw",
        ToggleCorner::NorthEast => "ne",
        ToggleCorner::SouthWest => "sw",
        ToggleCorner::SouthEast => "se",
    }
}

#[cfg(test)]
fn collapse_button_center_offset(
    expanded_size: PhysicalSize<u32>,
    safe_inset: u32,
    corner: ToggleCorner,
) -> PhysicalPosition<i32> {
    collapse_button_center_offset_for_layout(expanded_size, safe_inset, corner, false)
}

fn collapse_button_center_offset_for_layout(
    expanded_size: PhysicalSize<u32>,
    safe_inset: u32,
    corner: ToggleCorner,
    southwest_weekly_primary: bool,
) -> PhysicalPosition<i32> {
    let visual = visual_size(expanded_size, safe_inset);
    let scale = visual / EXPANDED_LOGICAL_SIZE;
    let button_half = TOGGLE_BUTTON_SIZE_LOGICAL * scale / 2.0;
    let (horizontal_edge_inset, vertical_edge_inset) = match corner {
        ToggleCorner::NorthWest | ToggleCorner::NorthEast => (
            TOGGLE_BUTTON_TOP_EDGE_INSET_LOGICAL,
            TOGGLE_BUTTON_TOP_EDGE_INSET_LOGICAL,
        ),
        ToggleCorner::SouthWest => (
            TOGGLE_BUTTON_EDGE_INSET_LOGICAL,
            if southwest_weekly_primary {
                TOGGLE_BUTTON_SW_WEEKLY_PRIMARY_BOTTOM_INSET_LOGICAL
            } else {
                TOGGLE_BUTTON_SW_BOTTOM_INSET_LOGICAL
            },
        ),
        ToggleCorner::SouthEast => (
            TOGGLE_BUTTON_EDGE_INSET_LOGICAL,
            TOGGLE_BUTTON_EDGE_INSET_LOGICAL,
        ),
    };
    let horizontal_inset = horizontal_edge_inset * scale + button_half;
    let vertical_inset = vertical_edge_inset * scale + button_half;
    let x = match corner {
        ToggleCorner::NorthWest | ToggleCorner::SouthWest => horizontal_inset,
        ToggleCorner::NorthEast | ToggleCorner::SouthEast => visual - horizontal_inset,
    };
    let y = match corner {
        ToggleCorner::NorthWest | ToggleCorner::NorthEast => vertical_inset,
        ToggleCorner::SouthWest | ToggleCorner::SouthEast => visual - vertical_inset,
    };
    PhysicalPosition::new(
        (safe_inset as f64 + x).round() as i32,
        (safe_inset as f64 + y).round() as i32,
    )
}

fn compact_anchor_from_expanded_for_layout(
    expanded: WidgetRect,
    compact_size: PhysicalSize<u32>,
    safe_inset: u32,
    toggle_corner: ToggleCorner,
    southwest_weekly_primary: bool,
) -> WidgetRect {
    let toggle_offset = collapse_button_center_offset_for_layout(
        expanded.size,
        safe_inset,
        toggle_corner,
        southwest_weekly_primary,
    );
    let compact_offset = compact_center_offset(compact_size, safe_inset);
    WidgetRect {
        position: PhysicalPosition::new(
            expanded.position.x + toggle_offset.x - compact_offset.x,
            expanded.position.y + toggle_offset.y - compact_offset.y,
        ),
        size: compact_size,
    }
}

fn compact_anchor_for_current_for_layout(
    current: WidgetRect,
    compact_size: PhysicalSize<u32>,
    safe_inset: u32,
    toggle_corner: ToggleCorner,
    southwest_weekly_primary: bool,
) -> WidgetRect {
    if current.size.width > compact_size.width + POSITION_EPSILON {
        compact_anchor_from_expanded_for_layout(
            current,
            compact_size,
            safe_inset,
            toggle_corner,
            southwest_weekly_primary,
        )
    } else {
        WidgetRect {
            position: current.position,
            size: compact_size,
        }
    }
}

#[cfg(test)]
fn expanded_position_from_anchor(
    collapsed: WidgetRect,
    expanded_size: PhysicalSize<u32>,
    safe_inset: u32,
    toggle_corner: ToggleCorner,
) -> PhysicalPosition<i32> {
    expanded_position_from_anchor_for_layout(
        collapsed,
        expanded_size,
        safe_inset,
        toggle_corner,
        false,
    )
}

fn expanded_position_from_anchor_for_layout(
    collapsed: WidgetRect,
    expanded_size: PhysicalSize<u32>,
    safe_inset: u32,
    toggle_corner: ToggleCorner,
    southwest_weekly_primary: bool,
) -> PhysicalPosition<i32> {
    let compact_offset = compact_center_offset(collapsed.size, safe_inset);
    let toggle_offset = collapse_button_center_offset_for_layout(
        expanded_size,
        safe_inset,
        toggle_corner,
        southwest_weekly_primary,
    );
    PhysicalPosition::new(
        collapsed.position.x + compact_offset.x - toggle_offset.x,
        collapsed.position.y + compact_offset.y - toggle_offset.y,
    )
}

fn rect_fully_in_bounds(
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    bounds_position: PhysicalPosition<i32>,
    bounds_size: PhysicalSize<u32>,
    safe_inset: i32,
) -> bool {
    let right = bounds_position.x + bounds_size.width as i32;
    let bottom = bounds_position.y + bounds_size.height as i32;
    position.x >= bounds_position.x - safe_inset
        && position.y >= bounds_position.y - safe_inset
        && position.x + size.width as i32 <= right + safe_inset
        && position.y + size.height as i32 <= bottom + safe_inset
}

fn visible_area(
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    bounds_position: PhysicalPosition<i32>,
    bounds_size: PhysicalSize<u32>,
) -> i64 {
    let left = position.x.max(bounds_position.x) as i64;
    let top = position.y.max(bounds_position.y) as i64;
    let right =
        (position.x + size.width as i32).min(bounds_position.x + bounds_size.width as i32) as i64;
    let bottom =
        (position.y + size.height as i32).min(bounds_position.y + bounds_size.height as i32) as i64;
    (right - left).max(0) * (bottom - top).max(0)
}

#[cfg(test)]
fn expanded_layout_from_anchor(
    collapsed: WidgetRect,
    expanded_size: PhysicalSize<u32>,
    bounds: Option<(PhysicalPosition<i32>, PhysicalSize<u32>)>,
    safe_inset: i32,
    preferred: ToggleCorner,
) -> (PhysicalPosition<i32>, ToggleCorner) {
    expanded_layout_from_anchor_for_layout(
        collapsed,
        expanded_size,
        bounds,
        safe_inset,
        preferred,
        false,
    )
}

fn expanded_layout_from_anchor_for_layout(
    collapsed: WidgetRect,
    expanded_size: PhysicalSize<u32>,
    bounds: Option<(PhysicalPosition<i32>, PhysicalSize<u32>)>,
    safe_inset: i32,
    preferred: ToggleCorner,
    southwest_weekly_primary: bool,
) -> (PhysicalPosition<i32>, ToggleCorner) {
    let desired_center = {
        let offset = compact_center_offset(collapsed.size, safe_inset.max(0) as u32);
        PhysicalPosition::new(
            collapsed.position.x + offset.x,
            collapsed.position.y + offset.y,
        )
    };
    let Some((bounds_position, bounds_size)) = bounds else {
        return (
            expanded_position_from_anchor_for_layout(
                collapsed,
                expanded_size,
                safe_inset.max(0) as u32,
                preferred,
                southwest_weekly_primary,
            ),
            preferred,
        );
    };
    let mid_x = bounds_position.x + bounds_size.width as i32 / 2;
    let mid_y = bounds_position.y + bounds_size.height as i32 / 2;
    let horizontal_left = desired_center.x <= mid_x;
    let vertical_top = desired_center.y <= mid_y;
    let quadrant_corner = match (horizontal_left, vertical_top) {
        (true, true) => ToggleCorner::NorthWest,
        (true, false) => ToggleCorner::SouthWest,
        (false, true) => ToggleCorner::NorthEast,
        (false, false) => ToggleCorner::SouthEast,
    };
    // The compact center's screen quadrant is the normal trigger. Only try a
    // neighboring corner when that quadrant's card cannot remain fully visible.
    let directional = match quadrant_corner {
        ToggleCorner::NorthWest => [
            ToggleCorner::NorthWest,
            ToggleCorner::SouthWest,
            ToggleCorner::NorthEast,
            ToggleCorner::SouthEast,
        ],
        ToggleCorner::SouthWest => [
            ToggleCorner::SouthWest,
            ToggleCorner::NorthWest,
            ToggleCorner::SouthEast,
            ToggleCorner::NorthEast,
        ],
        ToggleCorner::NorthEast => [
            ToggleCorner::NorthEast,
            ToggleCorner::NorthWest,
            ToggleCorner::SouthEast,
            ToggleCorner::SouthWest,
        ],
        ToggleCorner::SouthEast => [
            ToggleCorner::SouthEast,
            ToggleCorner::NorthEast,
            ToggleCorner::SouthWest,
            ToggleCorner::NorthWest,
        ],
    };
    for corner in directional {
        let position = expanded_position_from_anchor_for_layout(
            collapsed,
            expanded_size,
            safe_inset.max(0) as u32,
            corner,
            southwest_weekly_primary,
        );
        if rect_fully_in_bounds(
            position,
            expanded_size,
            bounds_position,
            bounds_size,
            safe_inset,
        ) {
            return (position, corner);
        }
    }
    // A card larger than the work area cannot be made fully visible. Keep the
    // toggle button anchored under the pointer and choose the most visible
    // candidate as a deterministic fallback.
    let mut best = (
        expanded_position_from_anchor_for_layout(
            collapsed,
            expanded_size,
            safe_inset.max(0) as u32,
            quadrant_corner,
            southwest_weekly_primary,
        ),
        quadrant_corner,
        i64::MIN,
    );
    for corner in [
        ToggleCorner::NorthWest,
        ToggleCorner::NorthEast,
        ToggleCorner::SouthWest,
        ToggleCorner::SouthEast,
    ] {
        let position = expanded_position_from_anchor_for_layout(
            collapsed,
            expanded_size,
            safe_inset.max(0) as u32,
            corner,
            southwest_weekly_primary,
        );
        let area = visible_area(position, expanded_size, bounds_position, bounds_size);
        if area > best.2 {
            best = (position, corner, area);
        }
    }
    (best.0, best.1)
}

#[cfg(test)]
fn expanded_position_in_bounds(
    collapsed: WidgetRect,
    expanded_size: PhysicalSize<u32>,
    bounds_position: PhysicalPosition<i32>,
    bounds_size: PhysicalSize<u32>,
    safe_inset: i32,
) -> PhysicalPosition<i32> {
    expanded_layout_from_anchor(
        collapsed,
        expanded_size,
        Some((bounds_position, bounds_size)),
        safe_inset,
        ToggleCorner::NorthEast,
    )
    .0
}

fn bounds_for_resize(
    monitor: Option<&tauri::Monitor>,
    work_area: Option<WorkAreaPayload>,
) -> Option<(PhysicalPosition<i32>, PhysicalSize<u32>)> {
    work_area
        .map(|area| {
            (
                PhysicalPosition::new(area.position.x, area.position.y),
                PhysicalSize::new(area.size.width, area.size.height),
            )
        })
        .or_else(|| {
            monitor.map(|item| {
                let area = item.work_area();
                (
                    PhysicalPosition::new(area.position.x, area.position.y),
                    PhysicalSize::new(area.size.width, area.size.height),
                )
            })
        })
}

fn clamp_position_in_bounds(
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    bounds: Option<(PhysicalPosition<i32>, PhysicalSize<u32>)>,
    safe_inset: i32,
) -> PhysicalPosition<i32> {
    let Some((bounds_position, bounds_size)) = bounds else {
        return position;
    };
    let right = bounds_position.x + bounds_size.width as i32;
    let bottom = bounds_position.y + bounds_size.height as i32;
    let min_x = bounds_position.x - safe_inset;
    let min_y = bounds_position.y - safe_inset;
    let max_x = (right - size.width as i32 + safe_inset).max(min_x);
    let max_y = (bottom - size.height as i32 + safe_inset).max(min_y);
    PhysicalPosition::new(
        position.x.clamp(min_x, max_x),
        position.y.clamp(min_y, max_y),
    )
}

fn safety_clamp_position_in_bounds(
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    bounds: Option<(PhysicalPosition<i32>, PhysicalSize<u32>)>,
    safe_inset: i32,
) -> PhysicalPosition<i32> {
    if bounds
        .map(|value| rect_intersects_bounds(position, size, value))
        .unwrap_or(true)
    {
        position
    } else {
        clamp_position_in_bounds(position, size, bounds, safe_inset)
    }
}

fn full_monitor_bounds(
    monitor: Option<&tauri::Monitor>,
) -> Option<(PhysicalPosition<i32>, PhysicalSize<u32>)> {
    monitor.map(|item| (*item.position(), *item.size()))
}

fn select_widget_bounds(
    monitor_bounds: Option<(PhysicalPosition<i32>, PhysicalSize<u32>)>,
    work_area_bounds: Option<(PhysicalPosition<i32>, PhysicalSize<u32>)>,
) -> Option<(PhysicalPosition<i32>, PhysicalSize<u32>)> {
    #[cfg(target_os = "macos")]
    {
        monitor_bounds.or(work_area_bounds)
    }
    #[cfg(not(target_os = "macos"))]
    {
        work_area_bounds.or(monitor_bounds)
    }
}

/// Return the bounds used for widget geometry corrections.
///
/// macOS reports a work area that excludes a side-mounted Dock. Treating that
/// rectangle as a hard window boundary makes a widget sitting over the Dock
/// look completely off-screen; the next resize then snaps it to the Dock's
/// left edge. macOS transparent widgets are intentionally allowed to cover
/// the full display, while Windows/Linux continue to respect taskbars and
/// panels through the work-area payload.
fn bounds_for_widget_geometry(
    monitor: Option<&tauri::Monitor>,
    work_area: Option<WorkAreaPayload>,
) -> Option<(PhysicalPosition<i32>, PhysicalSize<u32>)> {
    select_widget_bounds(
        full_monitor_bounds(monitor),
        // When a command originates from the settings window, no work-area
        // payload is available for the widget. Fall back to the widget's own
        // monitor work area instead of allowing Windows taskbars/panels to be
        // covered by a preset resize.
        bounds_for_resize(monitor, work_area),
    )
}

fn current_widget_rect(window: &tauri::WebviewWindow) -> Result<WidgetRect, String> {
    Ok(WidgetRect {
        position: window
            .outer_position()
            .map_err(|_| "failed to read widget position".to_string())?,
        size: window
            .outer_size()
            .map_err(|_| "failed to read widget size".to_string())?,
    })
}

fn monitor_and_scale(
    window: &tauri::WebviewWindow,
) -> Result<(Option<tauri::Monitor>, f64), String> {
    let monitor = window
        .current_monitor()
        .map_err(|_| "failed to read monitor".to_string())?;
    let scale_factor = monitor
        .as_ref()
        .map(|item| item.scale_factor())
        .unwrap_or(1.0);
    Ok((monitor, scale_factor))
}

#[cfg(target_os = "macos")]
fn set_native_window_frame_from_base(
    window: &tauri::WebviewWindow,
    _main_thread: objc2::MainThreadMarker,
    current: WidgetRect,
    current_frame: AppKitFrame,
    target_position: PhysicalPosition<i32>,
    target_size: PhysicalSize<u32>,
    scale_factor: f64,
    display: bool,
    glass_corner_radius: Option<f64>,
) -> Result<(), String> {
    use objc2_app_kit::{NSColor, NSWindow};
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    let native_window = window
        .ns_window()
        .map_err(|_| "failed to access native widget window".to_string())?;
    // SAFETY: Tauri's `ns_window` is an AppKit `NSWindow` owned by the live
    // webview window. The marker proves all native access is on the main thread.
    let ns_window = unsafe { &*(native_window as *const NSWindow) };
    let target_frame = appkit_frame_from_physical(
        current,
        current_frame,
        target_position,
        target_size,
        scale_factor,
    );
    ns_window.setFrame_display(
        NSRect::new(
            NSPoint::new(target_frame.x, target_frame.y),
            NSSize::new(target_frame.width, target_frame.height),
        ),
        display,
    );
    // AppKit can recreate the backing surface during a frame update. Keep the
    // native margins transparent in the same transaction as the resize so the
    // desktop never flashes through an opaque white/gray layer.
    ns_window.setOpaque(false);
    let clear = NSColor::clearColor();
    ns_window.setBackgroundColor(Some(&clear));
    // Keep the AppKit material aligned for both resize previews and ordinary
    // compact/expanded mode changes. The latter used to rely on autoresizing,
    // leaving a stale rounded material frame outside the CSS card.
    update_native_glass_geometry_on_main(ns_window, glass_corner_radius);
    Ok(())
}

#[cfg(target_os = "macos")]
fn current_native_window_frame_on_main(
    window: &tauri::WebviewWindow,
    _main_thread: objc2::MainThreadMarker,
) -> Result<AppKitFrame, String> {
    use objc2_app_kit::NSWindow;

    let native_window = window
        .ns_window()
        .map_err(|_| "failed to access native widget window".to_string())?;
    // SAFETY: Tauri's `ns_window` is an AppKit `NSWindow` owned by the live
    // webview window. The marker proves all native access is on the main thread.
    let ns_window = unsafe { &*(native_window as *const NSWindow) };
    let frame = ns_window.frame();
    Ok(AppKitFrame {
        x: frame.origin.x,
        y: frame.origin.y,
        width: frame.size.width,
        height: frame.size.height,
    })
}

#[cfg(target_os = "macos")]
fn current_native_window_frame(window: &tauri::WebviewWindow) -> Result<AppKitFrame, String> {
    if let Some(main_thread) = objc2::MainThreadMarker::new() {
        return current_native_window_frame_on_main(window, main_thread);
    }

    let main_thread_window = window.clone();
    let (frame_tx, frame_rx) = std::sync::mpsc::sync_channel(1);
    window
        .run_on_main_thread(move || {
            let result = objc2::MainThreadMarker::new()
                .ok_or_else(|| "failed to enter AppKit main thread".to_string())
                .and_then(|marker| {
                    current_native_window_frame_on_main(&main_thread_window, marker)
                });
            let _ = frame_tx.send(result);
        })
        .map_err(|_| "failed to schedule native frame read".to_string())?;
    frame_rx
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| "native frame read did not complete".to_string())?
}

#[cfg(target_os = "macos")]
fn set_native_window_frame(
    window: &tauri::WebviewWindow,
    main_thread: objc2::MainThreadMarker,
    current: WidgetRect,
    target_position: PhysicalPosition<i32>,
    target_size: PhysicalSize<u32>,
    scale_factor: f64,
) -> Result<(), String> {
    let current_frame = current_native_window_frame_on_main(window, main_thread)?;
    let visual_width =
        (f64::from(target_size.width) / scale_factor - EDGE_SAFE_INSET_LOGICAL * 2.0).max(1.0);
    // Compact and expanded sizes have disjoint ranges, so the visual size is
    // enough to select the matching radius when a mode switch goes through
    // the ordinary (non-resize-session) geometry path.
    let glass_corner_radius = if visual_width >= 180.0 {
        38.0 * (visual_width / EXPANDED_LOGICAL_SIZE)
    } else {
        (visual_width * 0.25).clamp(12.0, 36.0)
    };
    set_native_window_frame_from_base(
        window,
        main_thread,
        current,
        current_frame,
        target_position,
        target_size,
        scale_factor,
        true,
        Some(glass_corner_radius),
    )
}

#[cfg(target_os = "macos")]
fn apply_window_frame(
    window: &tauri::WebviewWindow,
    current: WidgetRect,
    target_position: PhysicalPosition<i32>,
    target_size: PhysicalSize<u32>,
    scale_factor: f64,
) -> Result<(), String> {
    if !scale_factor.is_finite() || scale_factor <= 0.0 {
        return Err("invalid widget scale factor".to_string());
    }

    if let Some(main_thread) = objc2::MainThreadMarker::new() {
        return set_native_window_frame(
            window,
            main_thread,
            current,
            target_position,
            target_size,
            scale_factor,
        );
    }

    // Wait for the AppKit update to complete so the next resize preview reads
    // a current Tauri rect that matches the NSWindow frame used as its delta
    // base. Without this handshake, queued previews can accumulate offsets.
    let frame_update_pending = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let frame_update_pending_on_main = frame_update_pending.clone();
    let main_thread_window = window.clone();
    let (completed_tx, completed_rx) = std::sync::mpsc::sync_channel(1);
    window
        .run_on_main_thread(move || {
            let result =
                if frame_update_pending_on_main.swap(false, std::sync::atomic::Ordering::AcqRel) {
                    let main_thread = objc2::MainThreadMarker::new()
                        .expect("run_on_main_thread closure must execute on the main thread");
                    set_native_window_frame(
                        &main_thread_window,
                        main_thread,
                        current,
                        target_position,
                        target_size,
                        scale_factor,
                    )
                } else {
                    Ok(())
                };
            let _ = completed_tx.send(result);
        })
        .map_err(|_| "failed to schedule widget frame update".to_string())?;
    match completed_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(result) => result,
        Err(_) => {
            // Cancel a task that is still queued, so an update reported as
            // failed cannot surprise the caller by applying much later.
            frame_update_pending.store(false, std::sync::atomic::Ordering::Release);
            Err("widget frame update did not complete".to_string())
        }
    }
}

#[cfg(target_os = "macos")]
fn apply_window_frame_from_base(
    window: &tauri::WebviewWindow,
    current: WidgetRect,
    base_frame: AppKitFrame,
    target_position: PhysicalPosition<i32>,
    target_size: PhysicalSize<u32>,
    scale_factor: f64,
    display: bool,
    glass_corner_radius: Option<f64>,
) -> Result<(), String> {
    if !scale_factor.is_finite() || scale_factor <= 0.0 {
        return Err("invalid widget scale factor".to_string());
    }

    if let Some(main_thread) = objc2::MainThreadMarker::new() {
        return set_native_window_frame_from_base(
            window,
            main_thread,
            current,
            base_frame,
            target_position,
            target_size,
            scale_factor,
            display,
            glass_corner_radius,
        );
    }

    let main_thread_window = window.clone();
    let (completed_tx, completed_rx) = std::sync::mpsc::sync_channel(1);
    window
        .run_on_main_thread(move || {
            let result = objc2::MainThreadMarker::new()
                .ok_or_else(|| "failed to enter AppKit main thread".to_string())
                .and_then(|marker| {
                    set_native_window_frame_from_base(
                        &main_thread_window,
                        marker,
                        current,
                        base_frame,
                        target_position,
                        target_size,
                        scale_factor,
                        display,
                        glass_corner_radius,
                    )
                });
            let _ = completed_tx.send(result);
        })
        .map_err(|_| "failed to schedule widget frame update".to_string())?;
    completed_rx
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| "widget frame update did not complete".to_string())?
}

#[cfg(not(target_os = "macos"))]
fn apply_window_frame_from_base(
    window: &tauri::WebviewWindow,
    current: WidgetRect,
    _base_frame: (),
    target_position: PhysicalPosition<i32>,
    target_size: PhysicalSize<u32>,
    scale_factor: f64,
    _display: bool,
    _glass_corner_radius: Option<f64>,
) -> Result<(), String> {
    apply_window_frame(window, current, target_position, target_size, scale_factor)
}

fn apply_resize_window_frame(
    window: &tauri::WebviewWindow,
    session: &WidgetResizeState,
    target_position: PhysicalPosition<i32>,
    target_size: PhysicalSize<u32>,
    display: bool,
) -> Result<(), String> {
    let visual_width = (f64::from(target_size.width.saturating_sub(session.safe_inset * 2))
        / session.scale_factor)
        .max(1.0);
    let glass_corner_radius = if matches!(session.mode, WidgetMode::Expanded) {
        38.0 * (visual_width / EXPANDED_LOGICAL_SIZE)
    } else {
        (visual_width * 0.25).clamp(12.0, 36.0)
    };
    #[cfg(target_os = "macos")]
    {
        return apply_window_frame_from_base(
            window,
            session.start_rect,
            session.start_native_frame,
            target_position,
            target_size,
            session.scale_factor,
            display,
            Some(glass_corner_radius),
        );
    }
    #[cfg(not(target_os = "macos"))]
    {
        return apply_window_frame_from_base(
            window,
            session.start_rect,
            (),
            target_position,
            target_size,
            session.scale_factor,
            display,
            Some(glass_corner_radius),
        );
    }
}

#[cfg(not(target_os = "macos"))]
fn apply_window_frame(
    window: &tauri::WebviewWindow,
    current: WidgetRect,
    target_position: PhysicalPosition<i32>,
    target_size: PhysicalSize<u32>,
    _scale_factor: f64,
) -> Result<(), String> {
    window
        .set_size(target_size)
        .map_err(|_| "failed to resize widget".to_string())?;
    if target_position != current.position {
        window
            .set_position(target_position)
            .map_err(|_| "failed to position widget".to_string())?;
    }
    Ok(())
}

fn infer_mode(rect: WidgetRect, collapsed_size: PhysicalSize<u32>) -> WidgetMode {
    if rect.size.width <= collapsed_size.width + POSITION_EPSILON
        && rect.size.height <= collapsed_size.height + POSITION_EPSILON
    {
        WidgetMode::Collapsed
    } else {
        WidgetMode::Expanded
    }
}

fn mode_from_preference(value: &str) -> Result<WidgetMode, String> {
    match value {
        "compact" => Ok(WidgetMode::Collapsed),
        "expanded" => Ok(WidgetMode::Expanded),
        _ => Err("invalid widget mode".to_string()),
    }
}

fn mode_preference(mode: WidgetMode) -> &'static str {
    match mode {
        WidgetMode::Collapsed => "compact",
        WidgetMode::Expanded => "expanded",
    }
}

fn resize_edge_from_preference(value: &str) -> Result<ResizeEdge, String> {
    match value {
        "n" => Ok(ResizeEdge::North),
        "s" => Ok(ResizeEdge::South),
        "e" => Ok(ResizeEdge::East),
        "w" => Ok(ResizeEdge::West),
        "ne" => Ok(ResizeEdge::NorthEast),
        "nw" => Ok(ResizeEdge::NorthWest),
        "se" => Ok(ResizeEdge::SouthEast),
        "sw" => Ok(ResizeEdge::SouthWest),
        _ => Err("invalid resize edge".to_string()),
    }
}

fn widget_size_preference(size: WidgetSize) -> &'static str {
    match size {
        WidgetSize::Small => "small",
        WidgetSize::Medium => "medium",
        WidgetSize::Large => "large",
    }
}

fn widget_size_factor(size: WidgetSize) -> f64 {
    match size {
        WidgetSize::Small => 0.84,
        WidgetSize::Medium => 1.0,
        WidgetSize::Large => 1.16,
    }
}

fn widget_size_marker(compact_size: f64, expanded_size: f64) -> &'static str {
    const EPSILON: f64 = 0.01;
    for (size, name) in [
        (WidgetSize::Small, "small"),
        (WidgetSize::Medium, "medium"),
        (WidgetSize::Large, "large"),
    ] {
        let factor = widget_size_factor(size);
        if (compact_size - COLLAPSED_LOGICAL_SIZE * factor).abs() <= EPSILON
            && (expanded_size - EXPANDED_LOGICAL_SIZE * factor).abs() <= EPSILON
        {
            return name;
        }
    }
    "custom"
}

fn clamp_logical_size(mode: WidgetMode, size: f64) -> f64 {
    let (min, max) = match mode {
        WidgetMode::Collapsed => (COMPACT_MIN_LOGICAL_SIZE, COMPACT_MAX_LOGICAL_SIZE),
        WidgetMode::Expanded => (EXPANDED_MIN_LOGICAL_SIZE, EXPANDED_MAX_LOGICAL_SIZE),
    };
    if size.is_finite() {
        size.clamp(min, max)
    } else {
        min
    }
}

fn widget_dimensions(
    preferences: &WidgetPreferences,
    scale_factor: f64,
    safe_inset: u32,
) -> (PhysicalSize<u32>, PhysicalSize<u32>) {
    let collapsed = widget_window_size(
        clamp_logical_size(WidgetMode::Collapsed, preferences.compact_size),
        scale_factor,
        safe_inset,
    );
    let expanded = widget_window_size(
        clamp_logical_size(WidgetMode::Expanded, preferences.expanded_size),
        scale_factor,
        safe_inset,
    );
    (
        PhysicalSize::new(collapsed, collapsed),
        PhysicalSize::new(expanded, expanded),
    )
}

#[cfg(test)]
fn widget_sizes(
    size: WidgetSize,
    scale_factor: f64,
    safe_inset: u32,
) -> (PhysicalSize<u32>, PhysicalSize<u32>) {
    let factor = widget_size_factor(size);
    let collapsed = widget_window_size(COLLAPSED_LOGICAL_SIZE * factor, scale_factor, safe_inset);
    let expanded = widget_window_size(EXPANDED_LOGICAL_SIZE * factor, scale_factor, safe_inset);
    (
        PhysicalSize::new(collapsed, collapsed),
        PhysicalSize::new(expanded, expanded),
    )
}

fn set_widget_mode_internal(
    mode: WidgetMode,
    work_area: Option<WorkAreaPayload>,
    app: &AppHandle,
    state: &AppState,
    southwest_weekly_primary: Option<bool>,
) -> Result<WidgetPreferences, String> {
    let window = app
        .get_webview_window("widget")
        .ok_or_else(|| "widget window missing".to_string())?;
    let current = current_widget_rect(&window)?;
    let (monitor, scale_factor) = monitor_and_scale(&window)?;
    let safe_inset = safe_inset_for_current_appearance(state, scale_factor) as i32;
    let preferences_snapshot = preferences_lock_value(state).clone();
    let (collapsed_size, expanded_size) =
        widget_dimensions(&preferences_snapshot, scale_factor, safe_inset as u32);
    let previous = state.geometry.lock().ok().and_then(|value| *value);
    let southwest_weekly_primary = southwest_weekly_primary
        .or_else(|| previous.map(|value| value.southwest_weekly_primary))
        .unwrap_or(false);
    let preferred_corner = previous
        .map(|value| value.toggle_corner)
        .unwrap_or_else(|| toggle_corner_from_preference(&preferences_snapshot.toggle_corner));
    let anchor = previous
        .map(|value| value.collapsed_rect.position)
        .unwrap_or_else(|| {
            compact_anchor_for_current_for_layout(
                current,
                collapsed_size,
                safe_inset as u32,
                preferred_corner,
                southwest_weekly_primary,
            )
            .position
        });
    let Some(monitor) = monitor else {
        let size = if matches!(mode, WidgetMode::Collapsed) {
            collapsed_size
        } else {
            expanded_size
        };
        let (target_position, selected_corner) = if matches!(mode, WidgetMode::Collapsed) {
            (anchor, preferred_corner)
        } else {
            (
                expanded_position_from_anchor_for_layout(
                    WidgetRect {
                        position: anchor,
                        size: collapsed_size,
                    },
                    expanded_size,
                    safe_inset as u32,
                    preferred_corner,
                    southwest_weekly_primary,
                ),
                preferred_corner,
            )
        };
        apply_window_frame(&window, current, target_position, size, scale_factor)?;
        if let Ok(mut geometry) = state.geometry.lock() {
            *geometry = Some(WidgetGeometryState {
                mode,
                collapsed_rect: WidgetRect {
                    position: anchor,
                    size: collapsed_size,
                },
                toggle_corner: selected_corner,
                southwest_weekly_primary,
            });
        }
        let mut preferences = preferences_lock_value(state).clone();
        preferences.widget_mode = mode_preference(mode).into();
        preferences.toggle_corner = toggle_corner_preference(selected_corner).into();
        persist_preferences(&state.preferences_path, &preferences)?;
        *preferences_lock_value(state) = preferences.clone();
        emit_preferences_changed(app, &preferences);
        return Ok(preferences);
    };
    let bounds = bounds_for_widget_geometry(Some(&monitor), work_area);
    let anchor = WidgetRect {
        position: safety_clamp_position_in_bounds(anchor, collapsed_size, bounds, safe_inset),
        size: collapsed_size,
    };
    let (target_position, target_size, selected_corner) = match mode {
        WidgetMode::Collapsed => (anchor.position, collapsed_size, preferred_corner),
        WidgetMode::Expanded => {
            let (position, corner) = expanded_layout_from_anchor_for_layout(
                anchor,
                expanded_size,
                bounds,
                safe_inset,
                preferred_corner,
                southwest_weekly_primary,
            );
            (position, expanded_size, corner)
        }
    };
    apply_window_frame(&window, current, target_position, target_size, scale_factor)?;
    if let Ok(mut geometry) = state.geometry.lock() {
        *geometry = Some(WidgetGeometryState {
            mode,
            collapsed_rect: anchor,
            toggle_corner: selected_corner,
            southwest_weekly_primary,
        });
    }
    let mut preferences = preferences_lock_value(state).clone();
    preferences.widget_mode = mode_preference(mode).into();
    preferences.toggle_corner = toggle_corner_preference(selected_corner).into();
    persist_preferences(&state.preferences_path, &preferences)?;
    *preferences_lock_value(state) = preferences.clone();
    emit_preferences_changed(&app, &preferences);
    Ok(preferences)
}

fn preferences_lock_value(state: &AppState) -> MutexGuard<'_, WidgetPreferences> {
    state
        .preferences
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[tauri::command]
fn set_widget_mode(
    mode: String,
    work_area: Option<WorkAreaPayload>,
    southwest_weekly_primary: Option<bool>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<WidgetPreferences, String> {
    set_widget_mode_internal(
        mode_from_preference(&mode)?,
        work_area,
        &app,
        state.inner(),
        southwest_weekly_primary,
    )
}

fn sync_widget_layout_internal(
    southwest_weekly_primary: bool,
    app: &AppHandle,
    state: &AppState,
) -> Result<(), String> {
    let window = app
        .get_webview_window("widget")
        .ok_or_else(|| "widget window missing".to_string())?;
    let current = current_widget_rect(&window)?;
    let (monitor, scale_factor) = monitor_and_scale(&window)?;
    let safe_inset = safe_inset_for_current_appearance(state, scale_factor) as i32;
    let preferences = preferences_lock_value(state).clone();
    let (collapsed_size, expanded_size) =
        widget_dimensions(&preferences, scale_factor, safe_inset as u32);
    let previous = state.geometry.lock().ok().and_then(|value| *value);
    let mode = previous
        .map(|value| value.mode)
        .or_else(|| mode_from_preference(&preferences.widget_mode).ok())
        .unwrap_or_else(|| infer_mode(current, collapsed_size));
    let preferred_corner = previous
        .map(|value| value.toggle_corner)
        .unwrap_or_else(|| toggle_corner_from_preference(&preferences.toggle_corner));
    let anchor_position = previous
        .map(|value| value.collapsed_rect.position)
        .unwrap_or_else(|| match mode {
            WidgetMode::Collapsed => current.position,
            WidgetMode::Expanded => {
                compact_anchor_from_expanded_for_layout(
                    current,
                    collapsed_size,
                    safe_inset as u32,
                    preferred_corner,
                    southwest_weekly_primary,
                )
                .position
            }
        });
    let bounds = monitor
        .as_ref()
        .and_then(|item| bounds_for_widget_geometry(Some(item), None));
    let anchor = WidgetRect {
        position: safety_clamp_position_in_bounds(
            anchor_position,
            collapsed_size,
            bounds,
            safe_inset,
        ),
        size: collapsed_size,
    };
    let (target_position, target_size, selected_corner) = match mode {
        WidgetMode::Collapsed => (anchor.position, collapsed_size, preferred_corner),
        WidgetMode::Expanded => {
            let (position, corner) = expanded_layout_from_anchor_for_layout(
                anchor,
                expanded_size,
                bounds,
                safe_inset,
                preferred_corner,
                southwest_weekly_primary,
            );
            (position, expanded_size, corner)
        }
    };
    apply_window_frame(&window, current, target_position, target_size, scale_factor)?;
    if let Ok(mut geometry) = state.geometry.lock() {
        *geometry = Some(WidgetGeometryState {
            mode,
            collapsed_rect: anchor,
            toggle_corner: selected_corner,
            southwest_weekly_primary,
        });
    }
    Ok(())
}

#[tauri::command]
fn sync_widget_layout(
    southwest_weekly_primary: bool,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    sync_widget_layout_internal(southwest_weekly_primary, &app, state.inner())
}

fn set_widget_dimensions_internal(
    compact_size_logical: f64,
    expanded_size_logical: f64,
    widget_size_marker: &'static str,
    work_area: Option<WorkAreaPayload>,
    app: &AppHandle,
    state: &AppState,
) -> Result<WidgetPreferences, String> {
    let window = app
        .get_webview_window("widget")
        .ok_or_else(|| "widget window missing".to_string())?;
    let current = current_widget_rect(&window)?;
    let (monitor, scale_factor) = monitor_and_scale(&window)?;
    let safe_inset = safe_inset_for_current_appearance(state, scale_factor) as i32;
    let current_preferences = preferences_lock_value(state).clone();
    let (old_collapsed_size, _) =
        widget_dimensions(&current_preferences, scale_factor, safe_inset as u32);
    let mut next_preferences = current_preferences.clone();
    next_preferences.compact_size = clamp_logical_size(WidgetMode::Collapsed, compact_size_logical);
    next_preferences.expanded_size =
        clamp_logical_size(WidgetMode::Expanded, expanded_size_logical);
    next_preferences.widget_size = widget_size_marker.into();
    let (collapsed_size, expanded_size) =
        widget_dimensions(&next_preferences, scale_factor, safe_inset as u32);
    let previous = state.geometry.lock().ok().and_then(|value| *value);
    let southwest_weekly_primary = previous
        .map(|value| value.southwest_weekly_primary)
        .unwrap_or(false);
    let mode = previous
        .map(|value| value.mode)
        .or_else(|| mode_from_preference(&current_preferences.widget_mode).ok())
        .unwrap_or_else(|| infer_mode(current, old_collapsed_size));
    let preferred_corner = previous
        .map(|value| value.toggle_corner)
        .unwrap_or_else(|| toggle_corner_from_preference(&current_preferences.toggle_corner));
    let anchor_position = previous
        .map(|value| value.collapsed_rect.position)
        .unwrap_or_else(|| match mode {
            WidgetMode::Collapsed => current.position,
            WidgetMode::Expanded => {
                compact_anchor_from_expanded_for_layout(
                    current,
                    collapsed_size,
                    safe_inset as u32,
                    preferred_corner,
                    southwest_weekly_primary,
                )
                .position
            }
        });

    let Some(monitor) = monitor else {
        let target_size = if matches!(mode, WidgetMode::Collapsed) {
            collapsed_size
        } else {
            expanded_size
        };
        let (target_position, selected_corner) = if matches!(mode, WidgetMode::Collapsed) {
            (anchor_position, preferred_corner)
        } else {
            (
                expanded_position_from_anchor_for_layout(
                    WidgetRect {
                        position: anchor_position,
                        size: collapsed_size,
                    },
                    expanded_size,
                    safe_inset as u32,
                    preferred_corner,
                    southwest_weekly_primary,
                ),
                preferred_corner,
            )
        };
        apply_window_frame(&window, current, target_position, target_size, scale_factor)?;
        if let Ok(mut geometry) = state.geometry.lock() {
            *geometry = Some(WidgetGeometryState {
                mode,
                collapsed_rect: WidgetRect {
                    position: anchor_position,
                    size: collapsed_size,
                },
                toggle_corner: selected_corner,
                southwest_weekly_primary,
            });
        }
        let mut preferences = next_preferences;
        preferences.toggle_corner = toggle_corner_preference(selected_corner).into();
        if let Err(error) = persist_preferences(&state.preferences_path, &preferences) {
            let rollback_current = current_widget_rect(&window).unwrap_or(current);
            let _ = apply_window_frame(
                &window,
                rollback_current,
                current.position,
                current.size,
                scale_factor,
            );
            if let Ok(mut geometry) = state.geometry.lock() {
                *geometry = previous;
            }
            return Err(error);
        }
        *preferences_lock_value(state) = preferences.clone();
        emit_preferences_changed(app, &preferences);
        return Ok(preferences);
    };

    let bounds = bounds_for_widget_geometry(Some(&monitor), work_area);
    let anchor = WidgetRect {
        position: safety_clamp_position_in_bounds(
            anchor_position,
            collapsed_size,
            bounds,
            safe_inset,
        ),
        size: collapsed_size,
    };
    let (target_position, target_size, selected_corner) = match mode {
        WidgetMode::Collapsed => (anchor.position, collapsed_size, preferred_corner),
        WidgetMode::Expanded => {
            let (position, corner) = expanded_layout_from_anchor_for_layout(
                anchor,
                expanded_size,
                bounds,
                safe_inset,
                preferred_corner,
                southwest_weekly_primary,
            );
            (position, expanded_size, corner)
        }
    };
    apply_window_frame(&window, current, target_position, target_size, scale_factor)?;
    if let Ok(mut geometry) = state.geometry.lock() {
        *geometry = Some(WidgetGeometryState {
            mode,
            collapsed_rect: anchor,
            toggle_corner: selected_corner,
            southwest_weekly_primary,
        });
    }
    let mut preferences = next_preferences;
    preferences.toggle_corner = toggle_corner_preference(selected_corner).into();
    if let Err(error) = persist_preferences(&state.preferences_path, &preferences) {
        let rollback_current = current_widget_rect(&window).unwrap_or(current);
        let _ = apply_window_frame(
            &window,
            rollback_current,
            current.position,
            current.size,
            scale_factor,
        );
        if let Ok(mut geometry) = state.geometry.lock() {
            *geometry = previous;
        }
        return Err(error);
    }
    *preferences_lock_value(state) = preferences.clone();
    emit_preferences_changed(app, &preferences);
    Ok(preferences)
}

fn set_widget_size_internal(
    size: WidgetSize,
    work_area: Option<WorkAreaPayload>,
    app: &AppHandle,
    state: &AppState,
) -> Result<WidgetPreferences, String> {
    let factor = widget_size_factor(size);
    set_widget_dimensions_internal(
        COLLAPSED_LOGICAL_SIZE * factor,
        EXPANDED_LOGICAL_SIZE * factor,
        widget_size_preference(size),
        work_area,
        app,
        state,
    )
}

#[tauri::command]
fn set_widget_size(
    size: String,
    work_area: Option<WorkAreaPayload>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<WidgetPreferences, String> {
    let size = match size.as_str() {
        "small" => WidgetSize::Small,
        "medium" => WidgetSize::Medium,
        "large" => WidgetSize::Large,
        _ => return Err("invalid widget size".to_string()),
    };
    set_widget_size_internal(size, work_area, &app, state.inner())
}

#[tauri::command]
fn set_widget_dimensions(
    compact_size: f64,
    expanded_size: f64,
    work_area: Option<WorkAreaPayload>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<WidgetPreferences, String> {
    let compact_size = clamp_logical_size(WidgetMode::Collapsed, compact_size);
    let expanded_size = clamp_logical_size(WidgetMode::Expanded, expanded_size);
    set_widget_dimensions_internal(
        compact_size,
        expanded_size,
        widget_size_marker(compact_size, expanded_size),
        work_area,
        &app,
        state.inner(),
    )
}

fn resize_position(
    start: WidgetRect,
    size: PhysicalSize<u32>,
    edge: ResizeEdge,
) -> PhysicalPosition<i32> {
    let move_left = matches!(
        edge,
        ResizeEdge::West | ResizeEdge::NorthWest | ResizeEdge::SouthWest
    );
    let move_top = matches!(
        edge,
        ResizeEdge::North | ResizeEdge::NorthEast | ResizeEdge::NorthWest
    );
    PhysicalPosition::new(
        if move_left {
            start.position.x + start.size.width as i32 - size.width as i32
        } else {
            start.position.x
        },
        if move_top {
            start.position.y + start.size.height as i32 - size.height as i32
        } else {
            start.position.y
        },
    )
}

fn max_outer_width_for_resize(
    start: WidgetRect,
    edge: ResizeEdge,
    bounds: (PhysicalPosition<i32>, PhysicalSize<u32>),
    safe_inset: u32,
) -> u32 {
    let (bounds_position, bounds_size) = bounds;
    let bounds_left = bounds_position.x - safe_inset as i32;
    let bounds_right = bounds_position.x + bounds_size.width as i32 + safe_inset as i32;
    if matches!(
        edge,
        ResizeEdge::West | ResizeEdge::NorthWest | ResizeEdge::SouthWest
    ) {
        (start.position.x + start.size.width as i32 - bounds_left).max(1) as u32
    } else {
        (bounds_right - start.position.x).max(1) as u32
    }
}

fn max_outer_height_for_resize(
    start: WidgetRect,
    edge: ResizeEdge,
    bounds: (PhysicalPosition<i32>, PhysicalSize<u32>),
    safe_inset: u32,
) -> u32 {
    let (bounds_position, bounds_size) = bounds;
    let bounds_top = bounds_position.y - safe_inset as i32;
    let bounds_bottom = bounds_position.y + bounds_size.height as i32 + safe_inset as i32;
    if matches!(
        edge,
        ResizeEdge::North | ResizeEdge::NorthEast | ResizeEdge::NorthWest
    ) {
        (start.position.y + start.size.height as i32 - bounds_top).max(1) as u32
    } else {
        (bounds_bottom - start.position.y).max(1) as u32
    }
}

fn max_logical_size_for_resize(session: &WidgetResizeState) -> Option<f64> {
    let bounds = session.bounds?;
    // Every resize keeps the widget square, so a horizontal edge drag can
    // also reach a vertical work-area boundary (and vice versa). Always use
    // both limits; corners naturally get the smaller of their two axes.
    let max_outer =
        max_outer_width_for_resize(session.start_rect, session.edge, bounds, session.safe_inset)
            .min(max_outer_height_for_resize(
                session.start_rect,
                session.edge,
                bounds,
                session.safe_inset,
            ));
    Some(((max_outer as f64 - (session.safe_inset * 2) as f64) / session.scale_factor).max(1.0))
}

fn rect_intersects_bounds(
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    bounds: (PhysicalPosition<i32>, PhysicalSize<u32>),
) -> bool {
    let right = position.x + size.width as i32;
    let bottom = position.y + size.height as i32;
    let bounds_right = bounds.0.x + bounds.1.width as i32;
    let bounds_bottom = bounds.0.y + bounds.1.height as i32;
    right > bounds.0.x
        && position.x < bounds_right
        && bottom > bounds.0.y
        && position.y < bounds_bottom
}

fn apply_widget_resize(
    size: f64,
    app: &AppHandle,
    state: &AppState,
    persist: bool,
) -> Result<Option<WidgetPreferences>, String> {
    let session = state
        .resize_state
        .lock()
        .map_err(|_| "resize state unavailable".to_string())?
        .ok_or_else(|| "widget resize has not started".to_string())?;
    let window = app
        .get_webview_window("widget")
        .ok_or_else(|| "widget window missing".to_string())?;
    let boundary_max = max_logical_size_for_resize(&session).unwrap_or(f64::INFINITY);
    let minimum_size = clamp_logical_size(session.mode, f64::NAN);
    let logical_size = clamp_logical_size(session.mode, size).min(boundary_max.max(minimum_size));
    let target = widget_window_size(logical_size, session.scale_factor, session.safe_inset);
    let target_size = PhysicalSize::new(target, target);
    let raw_position = resize_position(session.start_rect, target_size, session.edge);
    // Resizing should keep the opposite edge fixed, even when the dragged
    // edge reaches a work-area boundary. Only recover from a completely
    // off-screen start/target rectangle; normal edge resizing must not be
    // treated as position snapping.
    let target_position = session
        .bounds
        .filter(|bounds| !rect_intersects_bounds(raw_position, target_size, *bounds))
        .map(|bounds| {
            clamp_position_in_bounds(
                raw_position,
                target_size,
                Some(bounds),
                session.safe_inset as i32,
            )
        })
        .unwrap_or(raw_position);
    // Use the immutable resize-session frame for every preview. On macOS this
    // avoids reading an in-flight NSWindow frame and applying a second delta
    // on top of it, which is the source of both drift and visible jitter.
    // Preview frames must be displayed immediately. The renderer/bridge
    // coalesces stale requests, so deferring AppKit display here would only
    // make the shell appear to jump on the next event-loop repaint.
    apply_resize_window_frame(&window, &session, target_position, target_size, true)?;

    if let Ok(mut geometry) = state.geometry.lock() {
        let collapsed_rect = if matches!(session.mode, WidgetMode::Collapsed) {
            WidgetRect {
                position: target_position,
                size: target_size,
            }
        } else {
            compact_anchor_from_expanded_for_layout(
                WidgetRect {
                    position: target_position,
                    size: target_size,
                },
                session.start_collapsed_rect.size,
                session.safe_inset,
                session.toggle_corner,
                session.southwest_weekly_primary,
            )
        };
        *geometry = Some(WidgetGeometryState {
            mode: session.mode,
            collapsed_rect,
            toggle_corner: session.toggle_corner,
            southwest_weekly_primary: session.southwest_weekly_primary,
        });
    }

    if !persist {
        return Ok(None);
    }
    let mut preferences = preferences_lock_value(state).clone();
    match session.mode {
        WidgetMode::Collapsed => preferences.compact_size = logical_size,
        WidgetMode::Expanded => preferences.expanded_size = logical_size,
    }
    preferences.widget_size =
        widget_size_marker(preferences.compact_size, preferences.expanded_size).into();
    preferences.widget_mode = mode_preference(session.mode).into();
    preferences.toggle_corner = toggle_corner_preference(session.toggle_corner).into();
    persist_preferences(&state.preferences_path, &preferences)?;
    *preferences_lock_value(state) = preferences.clone();
    emit_preferences_changed(app, &preferences);
    Ok(Some(preferences))
}

#[tauri::command]
fn begin_widget_resize(
    mode: String,
    edge: String,
    work_area: Option<WorkAreaPayload>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let window = app
        .get_webview_window("widget")
        .ok_or_else(|| "widget window missing".to_string())?;
    let preferences = preferences_lock_value(state.inner()).clone();
    if preferences.locked {
        return Err("widget is locked".to_string());
    }
    let requested_mode = mode_from_preference(&mode)?;
    let current = current_widget_rect(&window)?;
    #[cfg(target_os = "macos")]
    let start_native_frame = current_native_window_frame(&window)?;
    let (monitor, scale_factor) = monitor_and_scale(&window)?;
    let safe_inset = safe_inset_for_current_appearance(state.inner(), scale_factor);
    let (collapsed_size, _) = widget_dimensions(&preferences, scale_factor, safe_inset);
    let geometry = state.geometry.lock().ok().and_then(|value| *value);
    let toggle_corner = geometry
        .map(|value| value.toggle_corner)
        .unwrap_or_else(|| toggle_corner_from_preference(&preferences.toggle_corner));
    let southwest_weekly_primary = geometry
        .map(|value| value.southwest_weekly_primary)
        .unwrap_or(false);
    // The renderer starts a session only for the mode it is displaying. Use
    // that request as the source of truth; geometry can briefly lag during a
    // mode transition and must not widen the compact range or vice versa.
    let actual_mode = requested_mode;
    let start_collapsed_rect = geometry
        .map(|value| value.collapsed_rect)
        .unwrap_or_else(|| match actual_mode {
            WidgetMode::Collapsed => WidgetRect {
                position: current.position,
                size: collapsed_size,
            },
            WidgetMode::Expanded => compact_anchor_from_expanded_for_layout(
                current,
                collapsed_size,
                safe_inset,
                toggle_corner,
                southwest_weekly_primary,
            ),
        });
    let bounds = bounds_for_widget_geometry(monitor.as_ref(), work_area);
    let session = WidgetResizeState {
        mode: actual_mode,
        edge: resize_edge_from_preference(&edge)?,
        start_rect: current,
        start_collapsed_rect,
        toggle_corner,
        southwest_weekly_primary,
        scale_factor,
        safe_inset,
        bounds,
        #[cfg(target_os = "macos")]
        start_native_frame,
    };
    *state
        .resize_state
        .lock()
        .map_err(|_| "resize state unavailable".to_string())? = Some(session);
    Ok(())
}

#[tauri::command]
fn preview_widget_resize(
    size: f64,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let _ = apply_widget_resize(size, &app, state.inner(), false)?;
    Ok(())
}

#[tauri::command]
fn finish_widget_resize(
    size: f64,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<WidgetPreferences, String> {
    let result = apply_widget_resize(size, &app, state.inner(), true)?
        .ok_or_else(|| "failed to commit widget resize".to_string())?;
    *state
        .resize_state
        .lock()
        .map_err(|_| "resize state unavailable".to_string())? = None;
    Ok(result)
}

#[tauri::command]
fn cancel_widget_resize(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let session = *state
        .resize_state
        .lock()
        .map_err(|_| "resize state unavailable".to_string())?;
    if let Some(session) = session {
        let window = app
            .get_webview_window("widget")
            .ok_or_else(|| "widget window missing".to_string())?;
        apply_resize_window_frame(
            &window,
            &session,
            session.start_rect.position,
            session.start_rect.size,
            true,
        )?;
        *state
            .resize_state
            .lock()
            .map_err(|_| "resize state unavailable".to_string())? = None;
        if let Ok(mut geometry) = state.geometry.lock() {
            *geometry = Some(WidgetGeometryState {
                mode: session.mode,
                collapsed_rect: session.start_collapsed_rect,
                toggle_corner: session.toggle_corner,
                southwest_weekly_primary: session.southwest_weekly_primary,
            });
        }
    }
    Ok(())
}

#[tauri::command]
fn reset_widget_size(
    mode: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<WidgetPreferences, String> {
    let mode = mode_from_preference(&mode)?;
    *state
        .resize_state
        .lock()
        .map_err(|_| "resize state unavailable".to_string())? = None;
    let window = app
        .get_webview_window("widget")
        .ok_or_else(|| "widget window missing".to_string())?;
    let current = current_widget_rect(&window)?;
    let (monitor, scale_factor) = monitor_and_scale(&window)?;
    let safe_inset = safe_inset_for_current_appearance(state.inner(), scale_factor);
    let target_logical = if matches!(mode, WidgetMode::Collapsed) {
        COLLAPSED_LOGICAL_SIZE
    } else {
        EXPANDED_LOGICAL_SIZE
    };
    let target = widget_window_size(target_logical, scale_factor, safe_inset);
    let target_size = PhysicalSize::new(target, target);
    let geometry = state.geometry.lock().ok().and_then(|value| *value);
    let current_preferences = preferences_lock_value(state.inner()).clone();
    let compact_size = widget_dimensions(&current_preferences, scale_factor, safe_inset).0;
    let preferred_corner = geometry
        .map(|value| value.toggle_corner)
        .unwrap_or_else(|| toggle_corner_from_preference(&current_preferences.toggle_corner));
    let southwest_weekly_primary = geometry
        .map(|value| value.southwest_weekly_primary)
        .unwrap_or(false);
    let anchor_position = geometry
        .map(|value| value.collapsed_rect.position)
        .unwrap_or_else(|| match mode {
            WidgetMode::Collapsed => current.position,
            WidgetMode::Expanded => {
                compact_anchor_from_expanded_for_layout(
                    current,
                    compact_size,
                    safe_inset,
                    preferred_corner,
                    southwest_weekly_primary,
                )
                .position
            }
        });
    let bounds = bounds_for_widget_geometry(monitor.as_ref(), None);
    let (target_position, selected_corner) = if matches!(mode, WidgetMode::Collapsed) {
        (
            safety_clamp_position_in_bounds(
                anchor_position,
                target_size,
                bounds,
                safe_inset as i32,
            ),
            preferred_corner,
        )
    } else {
        expanded_layout_from_anchor_for_layout(
            WidgetRect {
                position: anchor_position,
                size: compact_size,
            },
            target_size,
            bounds,
            safe_inset as i32,
            preferred_corner,
            southwest_weekly_primary,
        )
    };

    if let Err(error) =
        apply_window_frame(&window, current, target_position, target_size, scale_factor)
    {
        if let Ok(rollback_current) = current_widget_rect(&window) {
            let _ = apply_window_frame(
                &window,
                rollback_current,
                current.position,
                current.size,
                scale_factor,
            );
        }
        return Err(format!("failed to reset widget size: {error}"));
    }

    let mut preferences = preferences_lock_value(state.inner()).clone();
    if matches!(mode, WidgetMode::Collapsed) {
        preferences.compact_size = COLLAPSED_LOGICAL_SIZE;
    } else {
        preferences.expanded_size = EXPANDED_LOGICAL_SIZE;
    }
    preferences.widget_size =
        widget_size_marker(preferences.compact_size, preferences.expanded_size).into();
    preferences.widget_mode = mode_preference(mode).into();
    preferences.toggle_corner = toggle_corner_preference(selected_corner).into();
    if let Err(error) = persist_preferences(&state.preferences_path, &preferences) {
        if let Ok(rollback_current) = current_widget_rect(&window) {
            let _ = apply_window_frame(
                &window,
                rollback_current,
                current.position,
                current.size,
                scale_factor,
            );
        }
        return Err(error);
    }
    *preferences_lock_value(state.inner()) = preferences.clone();

    if let Ok(mut geometry) = state.geometry.lock() {
        *geometry = Some(WidgetGeometryState {
            mode,
            collapsed_rect: if matches!(mode, WidgetMode::Collapsed) {
                WidgetRect {
                    position: target_position,
                    size: target_size,
                }
            } else {
                WidgetRect {
                    position: anchor_position,
                    size: compact_size,
                }
            },
            toggle_corner: selected_corner,
            southwest_weekly_primary,
        });
    }
    emit_preferences_changed(&app, &preferences);
    Ok(preferences)
}

#[cfg(test)]
mod geometry_tests {
    use super::*;

    fn rect(x: i32, y: i32, size: u32) -> WidgetRect {
        WidgetRect {
            position: PhysicalPosition::new(x, y),
            size: PhysicalSize::new(size, size),
        }
    }

    #[test]
    fn appkit_frame_mapping_keeps_the_east_edge_fixed_when_resizing_west() {
        let frame = appkit_frame_from_physical(
            rect(100, 200, 100),
            AppKitFrame {
                x: 50.0,
                y: 300.0,
                width: 100.0,
                height: 100.0,
            },
            PhysicalPosition::new(60, 200),
            PhysicalSize::new(140, 140),
            1.0,
        );

        assert_eq!(
            frame,
            AppKitFrame {
                x: 10.0,
                y: 260.0,
                width: 140.0,
                height: 140.0,
            }
        );
    }

    #[test]
    fn appkit_frame_mapping_keeps_the_south_edge_fixed_when_resizing_north() {
        let frame = appkit_frame_from_physical(
            rect(100, 200, 100),
            AppKitFrame {
                x: 50.0,
                y: 300.0,
                width: 100.0,
                height: 100.0,
            },
            PhysicalPosition::new(100, 160),
            PhysicalSize::new(140, 140),
            1.0,
        );

        assert_eq!(
            frame,
            AppKitFrame {
                x: 50.0,
                y: 300.0,
                width: 140.0,
                height: 140.0,
            }
        );
    }

    #[test]
    fn appkit_frame_mapping_keeps_both_opposite_edges_fixed_when_resizing_northwest() {
        let frame = appkit_frame_from_physical(
            rect(100, 200, 100),
            AppKitFrame {
                x: 50.0,
                y: 300.0,
                width: 100.0,
                height: 100.0,
            },
            PhysicalPosition::new(60, 160),
            PhysicalSize::new(140, 140),
            1.0,
        );

        assert_eq!(
            frame,
            AppKitFrame {
                x: 10.0,
                y: 300.0,
                width: 140.0,
                height: 140.0,
            }
        );
    }

    #[test]
    fn appkit_frame_mapping_converts_retina_physical_deltas_to_points() {
        let frame = appkit_frame_from_physical(
            rect(200, 400, 200),
            AppKitFrame {
                x: 100.0,
                y: 300.0,
                width: 100.0,
                height: 100.0,
            },
            PhysicalPosition::new(120, 320),
            PhysicalSize::new(280, 280),
            2.0,
        );

        assert_eq!(
            frame,
            AppKitFrame {
                x: 60.0,
                y: 300.0,
                width: 140.0,
                height: 140.0,
            }
        );
    }

    #[test]
    fn appkit_frame_mapping_supports_negative_coordinate_monitors() {
        let frame = appkit_frame_from_physical(
            rect(-1_200, -100, 100),
            AppKitFrame {
                x: -1_200.0,
                y: 700.0,
                width: 100.0,
                height: 100.0,
            },
            PhysicalPosition::new(-1_240, -140),
            PhysicalSize::new(140, 140),
            1.0,
        );

        assert_eq!(
            frame,
            AppKitFrame {
                x: -1_240.0,
                y: 700.0,
                width: 140.0,
                height: 140.0,
            }
        );
    }

    #[test]
    fn window_size_includes_the_transparent_safe_inset() {
        assert_eq!(window_size_for_visual_size(72, 4), 80);
        assert_eq!(widget_window_size(306.0, 1.5, 6), 471);
    }

    #[test]
    fn widget_size_presets_scale_both_window_modes() {
        let (small_collapsed, small_expanded) = widget_sizes(WidgetSize::Small, 1.0, 4);
        let (medium_collapsed, medium_expanded) = widget_sizes(WidgetSize::Medium, 1.0, 4);
        let (large_collapsed, large_expanded) = widget_sizes(WidgetSize::Large, 1.0, 4);
        assert_eq!(small_collapsed.width, 68);
        assert_eq!(small_expanded.width, 265);
        assert_eq!(medium_collapsed.width, 80);
        assert_eq!(medium_expanded.width, 314);
        assert_eq!(large_collapsed.width, 92);
        assert_eq!(large_expanded.width, 363);
    }

    #[test]
    fn resize_edges_keep_the_opposite_edge_fixed() {
        let start = rect(100, 200, 100);
        let target = PhysicalSize::new(140, 140);
        assert_eq!(
            resize_position(start, target, ResizeEdge::East),
            PhysicalPosition::new(100, 200)
        );
        assert_eq!(
            resize_position(start, target, ResizeEdge::South),
            PhysicalPosition::new(100, 200)
        );
        assert_eq!(
            resize_position(start, target, ResizeEdge::West),
            PhysicalPosition::new(60, 200)
        );
        assert_eq!(
            resize_position(start, target, ResizeEdge::North),
            PhysicalPosition::new(100, 160)
        );
        assert_eq!(
            resize_position(start, target, ResizeEdge::NorthWest),
            PhysicalPosition::new(60, 160)
        );
        assert_eq!(
            resize_position(start, target, ResizeEdge::SouthEast),
            PhysicalPosition::new(100, 200)
        );
    }

    #[test]
    fn resize_boundary_limits_keep_the_fixed_edges_at_the_screen_boundary() {
        let bounds = (PhysicalPosition::new(0, 0), PhysicalSize::new(300, 300));
        let start = rect(100, 100, 80);
        let session = |edge| WidgetResizeState {
            mode: WidgetMode::Collapsed,
            edge,
            start_rect: start,
            start_collapsed_rect: start,
            toggle_corner: ToggleCorner::NorthEast,
            southwest_weekly_primary: false,
            scale_factor: 1.0,
            safe_inset: 4,
            bounds: Some(bounds),
            #[cfg(target_os = "macos")]
            start_native_frame: AppKitFrame {
                x: 100.0,
                y: 700.0,
                width: 100.0,
                height: 100.0,
            },
        };
        let expected = [
            (ResizeEdge::East, 196.0, PhysicalPosition::new(100, 100)),
            (ResizeEdge::West, 176.0, PhysicalPosition::new(-4, 100)),
            (ResizeEdge::South, 196.0, PhysicalPosition::new(100, 100)),
            (ResizeEdge::North, 176.0, PhysicalPosition::new(100, -4)),
            (
                ResizeEdge::SouthEast,
                196.0,
                PhysicalPosition::new(100, 100),
            ),
            (ResizeEdge::SouthWest, 176.0, PhysicalPosition::new(-4, 100)),
            (ResizeEdge::NorthEast, 176.0, PhysicalPosition::new(100, -4)),
            (ResizeEdge::NorthWest, 176.0, PhysicalPosition::new(-4, -4)),
        ];
        for (edge, logical_size, position) in expected {
            let resize_session = session(edge);
            assert_eq!(
                max_logical_size_for_resize(&resize_session),
                Some(logical_size)
            );
            let target = widget_window_size(logical_size, 1.0, 4);
            assert_eq!(
                resize_position(start, PhysicalSize::new(target, target), edge),
                position
            );
        }
    }

    #[test]
    fn square_resize_also_respects_the_other_axis_boundary_for_side_drags() {
        let start = rect(100, 200, 80);
        let session = WidgetResizeState {
            mode: WidgetMode::Collapsed,
            edge: ResizeEdge::East,
            start_rect: start,
            start_collapsed_rect: start,
            toggle_corner: ToggleCorner::NorthEast,
            southwest_weekly_primary: false,
            scale_factor: 1.0,
            safe_inset: 4,
            bounds: Some((PhysicalPosition::new(0, 0), PhysicalSize::new(300, 300))),
            #[cfg(target_os = "macos")]
            start_native_frame: AppKitFrame {
                x: 100.0,
                y: 700.0,
                width: 100.0,
                height: 100.0,
            },
        };
        // East drag fixes the left/top edges, but the square also grows down.
        // The bottom boundary therefore limits the side to 96 logical px.
        assert_eq!(max_logical_size_for_resize(&session), Some(96.0));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn resize_over_a_side_dock_keeps_the_fixed_edge_on_the_full_display() {
        let start = rect(1850, 100, 80);
        let target = PhysicalSize::new(120, 120);
        let raw_position = resize_position(start, target, ResizeEdge::East);
        let full_display = (PhysicalPosition::new(0, 0), PhysicalSize::new(1920, 1080));
        let dock_excluded_work_area = (PhysicalPosition::new(0, 0), PhysicalSize::new(1680, 1080));
        let selected_bounds =
            select_widget_bounds(Some(full_display), Some(dock_excluded_work_area));

        // A side-mounted Dock can make the old work-area rectangle report no
        // intersection even though the window is visibly on the display. The
        // full monitor bounds used on macOS keep the east edge stable.
        assert!(!rect_intersects_bounds(
            raw_position,
            target,
            dock_excluded_work_area
        ));
        assert_eq!(selected_bounds, Some(full_display));
        assert!(rect_intersects_bounds(
            raw_position,
            target,
            selected_bounds.unwrap()
        ));
        assert_eq!(raw_position, PhysicalPosition::new(1850, 100));
    }

    #[test]
    fn resize_position_is_safe_on_negative_origin_work_areas() {
        let position = clamp_position_in_bounds(
            PhysicalPosition::new(-900, 700),
            PhysicalSize::new(300, 300),
            Some((
                PhysicalPosition::new(-1280, -20),
                PhysicalSize::new(1280, 800),
            )),
            4,
        );
        assert_eq!(position, PhysicalPosition::new(-900, 484));
    }

    #[test]
    fn custom_dimensions_use_independent_logical_sizes() {
        let mut preferences = WidgetPreferences::default();
        preferences.widget_size = "custom".into();
        preferences.compact_size = 48.0;
        preferences.expanded_size = 460.0;
        let (compact, expanded) = widget_dimensions(&preferences, 2.0, 8);
        assert_eq!(compact.width, 112);
        assert_eq!(expanded.width, 936);
    }

    #[test]
    fn expansion_stays_inside_a_bottom_work_area_without_moving_the_anchor() {
        let compact = rect(1844, 964, 80);
        let (position, corner) = expanded_layout_from_anchor(
            compact,
            PhysicalSize::new(314, 314),
            Some((PhysicalPosition::new(0, 0), PhysicalSize::new(1920, 1040))),
            4,
            ToggleCorner::NorthEast,
        );
        assert_eq!(corner, ToggleCorner::SouthEast);
        let compact_center = compact_center_offset(compact.size, 4);
        let toggle_center = collapse_button_center_offset(PhysicalSize::new(314, 314), 4, corner);
        assert_eq!(
            PhysicalPosition::new(
                compact.position.x + compact_center.x,
                compact.position.y + compact_center.y
            ),
            PhysicalPosition::new(position.x + toggle_center.x, position.y + toggle_center.y)
        );
    }

    #[test]
    fn expansion_handles_negative_origin_work_areas() {
        let compact = rect(-1284, -4, 80);
        let (position, corner) = expanded_layout_from_anchor(
            compact,
            PhysicalSize::new(314, 314),
            Some((
                PhysicalPosition::new(-1280, 0),
                PhysicalSize::new(1280, 984),
            )),
            4,
            ToggleCorner::NorthEast,
        );
        assert_eq!(corner, ToggleCorner::NorthWest);
        let compact_center = compact_center_offset(compact.size, 4);
        let toggle_center = collapse_button_center_offset(PhysicalSize::new(314, 314), 4, corner);
        assert_eq!(
            PhysicalPosition::new(
                compact.position.x + compact_center.x,
                compact.position.y + compact_center.y
            ),
            PhysicalPosition::new(position.x + toggle_center.x, position.y + toggle_center.y)
        );
    }

    #[test]
    fn expansion_clamps_only_when_the_expanded_window_would_overflow() {
        let compact = rect(1750, 900, 80);
        let (position, corner) = expanded_layout_from_anchor(
            compact,
            PhysicalSize::new(314, 314),
            Some((PhysicalPosition::new(0, 0), PhysicalSize::new(1920, 1040))),
            4,
            ToggleCorner::NorthEast,
        );
        assert_eq!(corner, ToggleCorner::SouthEast);
        let compact_center = compact_center_offset(compact.size, 4);
        let toggle_center = collapse_button_center_offset(PhysicalSize::new(314, 314), 4, corner);
        assert_eq!(
            PhysicalPosition::new(
                compact.position.x + compact_center.x,
                compact.position.y + compact_center.y
            ),
            PhysicalPosition::new(position.x + toggle_center.x, position.y + toggle_center.y)
        );
    }

    #[test]
    fn expansion_aligns_the_collapse_button_with_the_compact_center() {
        let compact = rect(720, 420, 80);
        let position = expanded_position_in_bounds(
            compact,
            PhysicalSize::new(314, 314),
            PhysicalPosition::new(0, 0),
            PhysicalSize::new(1920, 1040),
            4,
        );
        assert_eq!(position, PhysicalPosition::new(713, 413));
        let compact_center = compact_center_offset(compact.size, 4);
        let collapse_button =
            collapse_button_center_offset(PhysicalSize::new(314, 314), 4, ToggleCorner::NorthWest);
        assert_eq!(
            PhysicalPosition::new(
                compact.position.x + compact_center.x,
                compact.position.y + compact_center.y,
            ),
            PhysicalPosition::new(
                position.x + collapse_button.x,
                position.y + collapse_button.y
            )
        );
    }

    #[test]
    fn expansion_selects_each_screen_quadrant_even_when_the_previous_corner_still_fits() {
        let cases = [
            (
                rect(200, 200, 80),
                ToggleCorner::SouthEast,
                ToggleCorner::NorthWest,
            ),
            (
                rect(1600, 200, 80),
                ToggleCorner::SouthWest,
                ToggleCorner::NorthEast,
            ),
            (
                rect(200, 800, 80),
                ToggleCorner::NorthEast,
                ToggleCorner::SouthWest,
            ),
            (
                rect(1600, 800, 80),
                ToggleCorner::NorthWest,
                ToggleCorner::SouthEast,
            ),
        ];

        for (compact, previous, expected) in cases {
            let (position, corner) = expanded_layout_from_anchor(
                compact,
                PhysicalSize::new(314, 314),
                Some((PhysicalPosition::new(0, 0), PhysicalSize::new(1920, 1040))),
                4,
                previous,
            );

            assert_eq!(corner, expected);
            let compact_center = compact_center_offset(compact.size, 4);
            let toggle_center =
                collapse_button_center_offset(PhysicalSize::new(314, 314), 4, corner);
            assert_eq!(
                PhysicalPosition::new(
                    compact.position.x + compact_center.x,
                    compact.position.y + compact_center.y,
                ),
                PhysicalPosition::new(position.x + toggle_center.x, position.y + toggle_center.y,)
            );
        }
    }

    #[test]
    fn top_corner_toggles_use_the_card_content_inset() {
        assert_eq!(
            collapse_button_center_offset(PhysicalSize::new(314, 314), 4, ToggleCorner::NorthWest),
            PhysicalPosition::new(47, 47)
        );
        assert_eq!(
            collapse_button_center_offset(PhysicalSize::new(314, 314), 4, ToggleCorner::NorthEast),
            PhysicalPosition::new(268, 47)
        );
        assert_eq!(
            collapse_button_center_offset(PhysicalSize::new(314, 314), 4, ToggleCorner::SouthEast),
            PhysicalPosition::new(274, 274)
        );
    }

    #[test]
    fn southwest_toggle_clears_footer_metric_and_preserves_anchor() {
        let toggle_center =
            collapse_button_center_offset(PhysicalSize::new(314, 314), 4, ToggleCorner::SouthWest);
        assert_eq!(toggle_center, PhysicalPosition::new(41, 242));

        let compact = rect(720, 420, 80);
        let expanded_position = expanded_position_from_anchor(
            compact,
            PhysicalSize::new(314, 314),
            4,
            ToggleCorner::SouthWest,
        );
        let compact_center = compact_center_offset(compact.size, 4);
        assert_eq!(
            PhysicalPosition::new(
                compact.position.x + compact_center.x,
                compact.position.y + compact_center.y,
            ),
            PhysicalPosition::new(
                expanded_position.x + toggle_center.x,
                expanded_position.y + toggle_center.y,
            )
        );
    }

    #[test]
    fn southwest_weekly_primary_toggle_uses_footer_alignment_and_preserves_anchor() {
        let regular =
            collapse_button_center_offset(PhysicalSize::new(314, 314), 4, ToggleCorner::SouthWest);
        let weekly_primary = collapse_button_center_offset_for_layout(
            PhysicalSize::new(314, 314),
            4,
            ToggleCorner::SouthWest,
            true,
        );
        assert_eq!(regular, PhysicalPosition::new(41, 242));
        assert_eq!(weekly_primary, PhysicalPosition::new(41, 274));

        let compact = rect(720, 420, 80);
        let expanded_position = expanded_position_from_anchor_for_layout(
            compact,
            PhysicalSize::new(314, 314),
            4,
            ToggleCorner::SouthWest,
            true,
        );
        let compact_center = compact_center_offset(compact.size, 4);
        assert_eq!(
            PhysicalPosition::new(
                compact.position.x + compact_center.x,
                compact.position.y + compact_center.y,
            ),
            PhysicalPosition::new(
                expanded_position.x + weekly_primary.x,
                expanded_position.y + weekly_primary.y,
            )
        );
    }

    #[test]
    fn large_compact_orb_near_left_edge_uses_a_left_toggle_corner() {
        let compact = rect(0, 420, 152);
        let (position, corner) = expanded_layout_from_anchor(
            compact,
            PhysicalSize::new(468, 468),
            Some((PhysicalPosition::new(0, 0), PhysicalSize::new(1920, 1040))),
            4,
            ToggleCorner::NorthEast,
        );
        assert_eq!(corner, ToggleCorner::NorthWest);
        let compact_center = compact_center_offset(compact.size, 4);
        let toggle_center = collapse_button_center_offset(PhysicalSize::new(468, 468), 4, corner);
        assert_eq!(
            PhysicalPosition::new(
                compact.position.x + compact_center.x,
                compact.position.y + compact_center.y
            ),
            PhysicalPosition::new(position.x + toggle_center.x, position.y + toggle_center.y)
        );
        assert!(rect_fully_in_bounds(
            position,
            PhysicalSize::new(468, 468),
            PhysicalPosition::new(0, 0),
            PhysicalSize::new(1920, 1040),
            4,
        ));
    }

    #[test]
    fn widget_size_marker_only_reports_presets_when_both_modes_match() {
        assert_eq!(widget_size_marker(72.0, 306.0), "medium");
        assert_eq!(widget_size_marker(72.0 * 0.84, 306.0 * 0.84), "small");
        assert_eq!(widget_size_marker(72.0 * 1.16, 306.0 * 1.16), "large");
        assert_eq!(widget_size_marker(72.0, 305.0), "custom");
    }
}

#[tauri::command]
fn begin_widget_drag(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let window = app
        .get_webview_window("widget")
        .ok_or_else(|| "widget window missing".to_string())?;
    let current = current_widget_rect(&window)?;
    let (_, scale_factor) = monitor_and_scale(&window)?;
    let safe_inset = safe_inset_for_current_appearance(state.inner(), scale_factor);
    let preferences = preferences_lock_value(state.inner()).clone();
    let (collapsed_size, _) = widget_dimensions(&preferences, scale_factor, safe_inset);
    let mode = state
        .geometry
        .lock()
        .ok()
        .and_then(|value| *value)
        .map(|value| value.mode)
        .or_else(|| mode_from_preference(&preferences_lock_value(state.inner()).widget_mode).ok())
        .unwrap_or_else(|| infer_mode(current, collapsed_size));
    if let Ok(mut drag_mode) = state.drag_mode.lock() {
        *drag_mode = Some(mode);
    }
    Ok(())
}

#[tauri::command]
fn finish_widget_drag(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let window = app
        .get_webview_window("widget")
        .ok_or_else(|| "widget window missing".to_string())?;
    let current = current_widget_rect(&window)?;
    let (monitor, scale_factor) = monitor_and_scale(&window)?;
    let Some(monitor) = monitor else {
        return Ok(());
    };
    let safe_inset = safe_inset_for_current_appearance(state.inner(), scale_factor);
    let preferences = preferences_lock_value(state.inner()).clone();
    let (collapsed_size, expanded_size) = widget_dimensions(&preferences, scale_factor, safe_inset);
    let geometry = state.geometry.lock().ok().and_then(|value| *value);
    let preferred_corner = geometry
        .map(|value| value.toggle_corner)
        .unwrap_or_else(|| toggle_corner_from_preference(&preferences.toggle_corner));
    let southwest_weekly_primary = geometry
        .map(|value| value.southwest_weekly_primary)
        .unwrap_or(false);
    let bounds = bounds_for_widget_geometry(Some(&monitor), None);
    let mode = state
        .drag_mode
        .lock()
        .ok()
        .and_then(|mut value| value.take())
        .or_else(|| {
            state
                .geometry
                .lock()
                .ok()
                .and_then(|value| *value)
                .map(|value| value.mode)
        })
        .unwrap_or_else(|| infer_mode(current, collapsed_size));

    match mode {
        WidgetMode::Collapsed => {
            let next_position = safety_clamp_position_in_bounds(
                current.position,
                collapsed_size,
                bounds,
                safe_inset as i32,
            );
            let collapsed_rect = WidgetRect {
                position: next_position,
                size: collapsed_size,
            };
            window
                .set_position(next_position)
                .map_err(|_| "failed to position widget".to_string())?;
            if let Ok(mut geometry) = state.geometry.lock() {
                *geometry = Some(WidgetGeometryState {
                    mode: WidgetMode::Collapsed,
                    collapsed_rect,
                    toggle_corner: preferred_corner,
                    southwest_weekly_primary,
                });
            }
        }
        WidgetMode::Expanded => {
            let current_position = safety_clamp_position_in_bounds(
                current.position,
                expanded_size,
                bounds,
                safe_inset as i32,
            );
            let collapsed_rect = compact_anchor_from_expanded_for_layout(
                WidgetRect {
                    position: current_position,
                    size: expanded_size,
                },
                collapsed_size,
                safe_inset,
                preferred_corner,
                southwest_weekly_primary,
            );
            window
                .set_position(current_position)
                .map_err(|_| "failed to position widget".to_string())?;
            if let Ok(mut geometry) = state.geometry.lock() {
                let mut value = geometry.unwrap_or(WidgetGeometryState {
                    mode: WidgetMode::Expanded,
                    collapsed_rect,
                    toggle_corner: preferred_corner,
                    southwest_weekly_primary,
                });
                value.mode = WidgetMode::Expanded;
                value.collapsed_rect = collapsed_rect;
                *geometry = Some(value);
            }
        }
    }
    Ok(())
}

#[tauri::command]
fn get_preferences(state: State<'_, AppState>) -> WidgetPreferences {
    // Preferences are always recoverable: an empty or invalid on-disk file
    // was normalized at startup, and a poisoned mutex is recovered above.
    // Do not turn a safe default into a user-facing startup error.
    preferences_lock(&state).clone()
}

fn emit_preferences_changed(app: &AppHandle, preferences: &WidgetPreferences) {
    sync_tray_preferences(app, preferences);
    let _ = app.emit_to("widget", "preferences-changed", preferences.clone());
    let _ = app.emit_to("settings", "preferences-changed", preferences.clone());
}

fn renderer_preferences(
    current: &WidgetPreferences,
    mut requested: WidgetPreferences,
) -> WidgetPreferences {
    requested.custom_skins = current.custom_skins.clone();
    requested.normalized()
}

fn app_config_directory(state: &AppState) -> Result<&std::path::Path, String> {
    state
        .preferences_path
        .parent()
        .ok_or_else(|| "app config directory unavailable".to_string())
}

#[cfg(test)]
mod renderer_preference_tests {
    use super::*;
    use crate::models::CustomSkinMetadata;

    fn native_custom_skin() -> CustomSkinMetadata {
        CustomSkinMetadata {
            id: "lake".into(),
            name: "Lake".into(),
            file_name: "lake.png".into(),
            detected_tone: "dark".into(),
            text_tone: "auto".into(),
            accent_color: "#3677C8".into(),
        }
    }

    #[test]
    fn renderer_preferences_preserve_the_native_custom_skin_catalog() {
        let current = WidgetPreferences {
            custom_skins: vec![native_custom_skin()],
            ..WidgetPreferences::default()
        };

        let partial = WidgetPreferences {
            selected_skin: "custom:lake".into(),
            custom_skins: Vec::new(),
            ..WidgetPreferences::default()
        };
        let saved = renderer_preferences(&current, partial);
        assert_eq!(saved.selected_skin, "custom:lake");
        assert_eq!(saved.custom_skins.len(), 1);
        assert_eq!(saved.custom_skins[0].id, "lake");

        let forged = WidgetPreferences {
            selected_skin: "custom:forged".into(),
            custom_skins: vec![CustomSkinMetadata {
                id: "forged".into(),
                ..native_custom_skin()
            }],
            ..WidgetPreferences::default()
        };
        let saved = renderer_preferences(&current, forged);
        assert_eq!(saved.selected_skin, "glass");
        assert_eq!(saved.custom_skins.len(), 1);
        assert_eq!(saved.custom_skins[0].id, "lake");
    }
}

#[tauri::command]
fn set_preferences(
    preferences: WidgetPreferences,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<WidgetPreferences, String> {
    let current = preferences_lock(&state).clone();
    let preferences = renderer_preferences(&current, preferences);
    persist_preferences(&state.preferences_path, &preferences)?;
    *preferences_lock(&state) = preferences.clone();
    if let Some(window) = app.get_webview_window("widget") {
        sync_native_glass_material(&window, &preferences);
    }
    emit_preferences_changed(&app, &preferences);
    Ok(preferences)
}

#[tauri::command]
fn import_custom_skin(
    name: String,
    bytes: Vec<u8>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<CustomSkinMetadata, String> {
    let config_dir = app_config_directory(&state)?;
    let mut current = preferences_lock(&state);
    let (metadata, next) =
        custom_skins::import_skin(config_dir, &state.preferences_path, &current, &name, &bytes)?;
    *current = next.clone();
    drop(current);
    emit_preferences_changed(&app, &next);
    Ok(metadata)
}

#[tauri::command]
fn get_custom_skin_asset(
    id: String,
    state: State<'_, AppState>,
) -> Result<custom_skins::CustomSkinAsset, String> {
    let config_dir = app_config_directory(&state)?;
    let preferences = preferences_lock(&state).clone();
    custom_skins::load_skin_asset(config_dir, &preferences, &id)
}

#[tauri::command]
fn update_custom_skin(
    id: String,
    name: String,
    text_tone: String,
    accent_color: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<WidgetPreferences, String> {
    let mut current = preferences_lock(&state);
    let next = custom_skins::update_skin(
        &state.preferences_path,
        &current,
        &id,
        &name,
        &text_tone,
        &accent_color,
    )?;
    *current = next.clone();
    drop(current);
    emit_preferences_changed(&app, &next);
    Ok(next)
}

#[tauri::command]
fn delete_custom_skin(
    id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<WidgetPreferences, String> {
    let config_dir = app_config_directory(&state)?;
    let mut current = preferences_lock(&state);
    let next = custom_skins::delete_skin(config_dir, &state.preferences_path, &current, &id)?;
    *current = next.clone();
    drop(current);
    emit_preferences_changed(&app, &next);
    Ok(next)
}

#[tauri::command]
fn select_skin(
    id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<WidgetPreferences, String> {
    let mut preferences = preferences_lock(&state).clone();
    preferences.selected_skin = id;
    let preferences = preferences.normalized();
    persist_preferences(&state.preferences_path, &preferences)?;
    *preferences_lock(&state) = preferences.clone();
    if let Some(window) = app.get_webview_window("widget") {
        sync_native_glass_material(&window, &preferences);
    }
    emit_preferences_changed(&app, &preferences);
    Ok(preferences)
}

fn apply_lock(app: &AppHandle, locked: bool) -> Result<(), String> {
    let window = app
        .get_webview_window("widget")
        .ok_or_else(|| "widget window missing".to_string())?;
    window
        .set_ignore_cursor_events(locked)
        .map_err(|_| "failed to toggle click-through".to_string())
}

#[tauri::command]
fn set_widget_locked(
    locked: bool,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<WidgetPreferences, String> {
    let previous = state
        .preferences
        .lock()
        .map_err(|_| "settings unavailable".to_string())?
        .clone();
    let mut next = previous.clone();
    next.locked = locked;
    persist_preferences(&state.preferences_path, &next)?;
    if let Err(error) = apply_lock(&app, locked) {
        let _ = persist_preferences(&state.preferences_path, &previous);
        return Err(error);
    }
    *state
        .preferences
        .lock()
        .map_err(|_| "settings unavailable".to_string())? = next.clone();
    emit_preferences_changed(&app, &next);
    Ok(next)
}

#[tauri::command]
fn set_widget_always_on_top(
    always_on_top: bool,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<WidgetPreferences, String> {
    let previous = state
        .preferences
        .lock()
        .map_err(|_| "settings unavailable".to_string())?
        .clone();
    let mut next = previous.clone();
    next.always_on_top = always_on_top;
    persist_preferences(&state.preferences_path, &next)?;
    let window = app
        .get_webview_window("widget")
        .ok_or_else(|| "widget window missing".to_string())?;
    if let Err(error) = window.set_always_on_top(always_on_top) {
        let _ = persist_preferences(&state.preferences_path, &previous);
        return Err(format!("failed to toggle always-on-top: {error}"));
    }
    *state
        .preferences
        .lock()
        .map_err(|_| "settings unavailable".to_string())? = next.clone();
    emit_preferences_changed(&app, &next);
    Ok(next)
}

#[tauri::command]
fn sync_widget_appearance(
    _appearance: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let window = app
        .get_webview_window("widget")
        .ok_or_else(|| "widget window missing".to_string())?;
    let current = current_widget_rect(&window)?;
    let (_, scale_factor) = monitor_and_scale(&window)?;
    let safe_inset = safe_inset_for_current_appearance(state.inner(), scale_factor);
    let preferences = preferences_lock_value(state.inner()).clone();
    let (collapsed_size, expanded_size) = widget_dimensions(&preferences, scale_factor, safe_inset);
    let mode = state
        .geometry
        .lock()
        .ok()
        .and_then(|value| *value)
        .map(|value| value.mode)
        .unwrap_or_else(|| infer_mode(current, collapsed_size));
    let target_size = if matches!(mode, WidgetMode::Expanded) {
        expanded_size
    } else {
        collapsed_size
    };
    clear_widget_window_background(&window);
    sync_native_glass_material(&window, &preferences);
    window
        .set_size(target_size)
        .map_err(|_| "failed to resize widget for appearance".to_string())?;
    // macOS may recreate the backing surface while applying a size change.
    // Re-assert the clear native surface after the resize has been queued so
    // transparent margins do not become an opaque white frame.
    clear_widget_window_background(&window);
    sync_native_glass_material(&window, &preferences);
    Ok(())
}

fn show_settings_window(app: &AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("settings")
        .ok_or_else(|| "settings window missing".to_string())?;
    activate_settings_window(&window)
}

fn clear_widget_window_background(window: &tauri::WebviewWindow) {
    // `transparent: true` disables the native window surface at creation time,
    // but WKWebView can restore its opaque white under-page background when a
    // transparent window is resized or restored by window-state. Re-assert the
    // clear color for both the native window and its WebView before revealing
    // the widget so Glass can blend with the desktop rather than a white layer.
    let _ = window.set_background_color(Some(Color(0, 0, 0, 0)));

    #[cfg(target_os = "macos")]
    {
        let native_window = window.clone();
        let native_window_for_clear = native_window.clone();
        let clear_on_main = move || -> Result<(), String> {
            use objc2_app_kit::{NSColor, NSWindow};

            let pointer = native_window_for_clear
                .ns_window()
                .map_err(|_| "failed to access native widget window".to_string())?;
            // SAFETY: `ns_window` is the live AppKit window owned by Tauri;
            // this closure is only run on AppKit's main thread.
            let ns_window = unsafe { &*(pointer as *const NSWindow) };
            ns_window.setOpaque(false);
            let clear = NSColor::clearColor();
            ns_window.setBackgroundColor(Some(&clear));
            ns_window.setHasShadow(false);
            Ok(())
        };

        if objc2::MainThreadMarker::new().is_some() {
            let _ = clear_on_main();
        } else {
            let (done_tx, done_rx) = std::sync::mpsc::sync_channel(1);
            if native_window
                .run_on_main_thread(move || {
                    let _ = done_tx.send(clear_on_main());
                })
                .is_ok()
            {
                let _ = done_rx.recv_timeout(Duration::from_secs(2));
            }
        }
    }
}

// AppKit owns background sampling for the two native Glass materials. The Dock
// material stays behind the WebView; on macOS 26+ Liquid Glass embeds the
// WebView as NSGlassEffectView's contentView so AppKit can render the material
// and its contents as one unit. Both use the existing 4px safety inset.
#[cfg(target_os = "macos")]
const NATIVE_DOCK_GLASS_ALPHA: f64 = 0.84;

#[cfg(target_os = "macos")]
fn set_native_glass_corner_radius(view: &objc2_app_kit::NSView, radius: f64) {
    // NSVisualEffectView does not expose the undocumented `setCornerRadius:`
    // selector on macOS 15. Calling it through msg_send raises an Objective-C
    // exception that crosses tao's callback boundary and aborts the process.
    // Use the supported layer-backed view path instead.
    view.setWantsLayer(true);
    if let Some(layer) = view.layer() {
        layer.setCornerRadius(radius);
        layer.setMasksToBounds(true);
    }
}

#[cfg(target_os = "macos")]
fn find_native_dock_view(
    root: &objc2_app_kit::NSView,
) -> Option<objc2::rc::Retained<objc2_app_kit::NSView>> {
    use objc2::Message;
    use objc2_app_kit::NSVisualEffectView;

    root.subviews().iter().find_map(|child| {
        child
            .downcast_ref::<NSVisualEffectView>()
            .map(|_| child.retain())
    })
}

#[cfg(target_os = "macos")]
fn update_native_glass_geometry_on_main(ns_window: &objc2_app_kit::NSWindow, radius: Option<f64>) {
    use objc2_app_kit::NSGlassEffectView;
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    let Some(root) = ns_window.contentView() else {
        return;
    };
    // The WebView's content view can keep its old bounds for one compositor
    // turn after AppKit resizes the transparent window. Recompute the native
    // material frame here instead of relying only on autoresizing; otherwise
    // the previous Dock surface remains visible as a smaller/larger rounded
    // rectangle, most noticeably at the four corners.
    let bounds = root.bounds();
    let inset = EDGE_SAFE_INSET_LOGICAL;
    let frame = NSRect::new(
        NSPoint::new(inset, inset),
        NSSize::new(
            (bounds.size.width - inset * 2.0).max(1.0),
            (bounds.size.height - inset * 2.0).max(1.0),
        ),
    );
    if let Some(dock) = find_native_dock_view(&root) {
        dock.setFrame(frame);
        if let Some(radius) = radius {
            set_native_glass_corner_radius(&dock, radius);
        }
    }
    if supports_liquid_glass_runtime() {
        for child in root.subviews().iter() {
            if let Some(glass) = child.downcast_ref::<NSGlassEffectView>() {
                glass.setFrame(frame);
                if let Some(radius) = radius {
                    glass.setCornerRadius(radius);
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn unwrap_liquid_glass_on_main(
    ns_window: &objc2_app_kit::NSWindow,
) -> Option<objc2::rc::Retained<objc2_app_kit::NSView>> {
    use objc2_app_kit::NSGlassEffectView;

    let root = ns_window.contentView()?;
    if !supports_liquid_glass_runtime() {
        return Some(root);
    }
    for child in root.subviews().iter() {
        if let Some(glass) = child.downcast_ref::<NSGlassEffectView>() {
            if let Some(webview) = glass.contentView() {
                glass.setContentView(None);
                ns_window.setContentView(Some(&webview));
                return Some(webview);
            }
        }
    }
    Some(root)
}

#[cfg(target_os = "macos")]
fn apply_native_glass_material_on_main(
    window: &tauri::WebviewWindow,
    preferences: &WidgetPreferences,
    main_thread: objc2::MainThreadMarker,
) -> Result<(), String> {
    use objc2::MainThreadOnly;
    use objc2_app_kit::{
        NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
        NSAutoresizingMaskOptions, NSGlassEffectView, NSGlassEffectViewStyle, NSView,
        NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState,
        NSVisualEffectView, NSWindow, NSWindowOrderingMode,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    let native_window = window
        .ns_window()
        .map_err(|_| "failed to access native widget window".to_string())?;
    // SAFETY: `ns_window` is the live AppKit window and `main_thread` proves
    // that all Objective-C access below runs on AppKit's main thread.
    let ns_window = unsafe { &*(native_window as *const NSWindow) };
    // Keep the native material opt-in to macOS. On Windows/Linux the CSS
    // fallback remains the only supported implementation.
    if !supports_native_dock_runtime() {
        return Ok(());
    }
    // Do not attach AppKit sibling views while the transparent WebView is
    // hidden/being created. WKWebView may replace its backing content view at
    // that point; waiting until after `show()` avoids the macOS 15 dealloc
    // assertion that occurred during startup.
    if !ns_window.isVisible() {
        return Ok(());
    }
    let Some(content_view) = unwrap_liquid_glass_on_main(ns_window) else {
        return Ok(());
    };
    let existing_vibrancy = find_native_dock_view(&content_view);
    // Transparent is intentionally the exact pre-existing 0px Glass style.
    if preferences.selected_skin != "glass" || preferences.glass_style == "transparent" {
        // Keep the AppKit view alive while WebKit may still hold a compositor
        // reference to it. Removing it during a script-message callback can
        // trip macOS 15's NSView dealloc assertion; hiding it is equivalent for
        // rendering and makes material changes safe and reversible.
        if let Some(previous) = existing_vibrancy {
            previous.setHidden(true);
        }
        return Ok(());
    }

    let bounds = content_view.bounds();
    let inset = EDGE_SAFE_INSET_LOGICAL;
    let frame = NSRect::new(
        NSPoint::new(inset, inset),
        NSSize::new(
            (bounds.size.width - inset * 2.0).max(1.0),
            (bounds.size.height - inset * 2.0).max(1.0),
        ),
    );
    let visual_width = frame.size.width;
    let radius = if preferences.widget_mode == "expanded" {
        38.0 * (visual_width / EXPANDED_LOGICAL_SIZE)
    } else {
        (visual_width * 0.25).clamp(12.0, 36.0)
    };
    // SAFETY: these are stable AppKit appearance-name constants available on
    // every macOS version supported by this application.
    let appearance = unsafe {
        match preferences.appearance.as_str() {
            "light" => NSAppearance::appearanceNamed(NSAppearanceNameAqua),
            "dark" => NSAppearance::appearanceNamed(NSAppearanceNameDarkAqua),
            _ => None,
        }
    };
    if preferences.glass_style == "liquid" && supports_liquid_glass_runtime() {
        let outer = NSView::initWithFrame(NSView::alloc(main_thread), bounds);
        outer.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        let glass = NSGlassEffectView::initWithFrame(NSGlassEffectView::alloc(main_thread), frame);
        glass.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        glass.setStyle(NSGlassEffectViewStyle::Regular);
        glass.setCornerRadius(radius);
        glass.setAppearance(appearance.as_deref());
        // Leave tintColor unset and interaction at AppKit's non-interactive
        // default: the desktop determines the color and resizing cannot press
        // or deform the whole widget.
        ns_window.setContentView(Some(&outer));
        glass.setContentView(Some(&content_view));
        outer.addSubview(&glass);
        return Ok(());
    }

    // Dock mode uses the neutral system material behind the transparent page.
    if let Some(previous) = existing_vibrancy {
        if let Some(previous_effect) = previous.downcast_ref::<NSVisualEffectView>() {
            previous_effect.setHidden(false);
            previous_effect.setFrame(frame);
            previous_effect.setMaterial(NSVisualEffectMaterial::UnderWindowBackground);
            previous_effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
            previous_effect.setState(NSVisualEffectState::Active);
            previous_effect.setAppearance(appearance.as_deref());
            previous_effect.setAlphaValue(NATIVE_DOCK_GLASS_ALPHA);
        } else {
            return Ok(());
        }
        set_native_glass_corner_radius(&previous, radius);
        return Ok(());
    }
    // Use the stock AppKit class. The older tagged subclass supplied by
    // window-vibrancy can trip an NSView dealloc assertion in macOS 15 when
    // WebKit releases a compositor surface during an invoke callback.
    let effect_view =
        NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(main_thread), frame);
    effect_view.setMaterial(NSVisualEffectMaterial::UnderWindowBackground);
    effect_view.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    effect_view.setState(NSVisualEffectState::Active);
    effect_view.setAppearance(appearance.as_deref());
    effect_view.setAlphaValue(NATIVE_DOCK_GLASS_ALPHA);
    effect_view.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    set_native_glass_corner_radius(&effect_view, radius);
    content_view.addSubview_positioned_relativeTo(&effect_view, NSWindowOrderingMode::Below, None);
    Ok(())
}

#[cfg(target_os = "macos")]
fn sync_native_glass_material(window: &tauri::WebviewWindow, preferences: &WidgetPreferences) {
    let window_for_main = window.clone();
    let preferences_for_main = preferences.clone();
    let apply = move || {
        if let Some(main_thread) = objc2::MainThreadMarker::new() {
            let _ = apply_native_glass_material_on_main(
                &window_for_main,
                &preferences_for_main,
                main_thread,
            );
        }
    };
    if objc2::MainThreadMarker::new().is_some() {
        apply();
    } else {
        let _ = window.run_on_main_thread(apply);
    }
}

#[cfg(not(target_os = "macos"))]
fn sync_native_glass_material(_window: &tauri::WebviewWindow, _preferences: &WidgetPreferences) {}

#[tauri::command]
fn show_settings(app: AppHandle) -> Result<(), String> {
    show_settings_window(&app)
}

#[tauri::command]
fn get_launch_at_login(app: AppHandle) -> Result<bool, String> {
    let manager = app.autolaunch();
    read_launch_at_login(manager.inner())
}

fn set_launch_at_login_internal(app: &AppHandle, enabled: bool) -> Result<bool, String> {
    let manager = app.autolaunch();
    let actual = match write_launch_at_login(manager.inner(), enabled) {
        Ok(actual) => actual,
        Err(error) => {
            if let Some(menu) = app.try_state::<TrayMenuState>() {
                match read_launch_at_login(manager.inner()) {
                    Ok(actual) => {
                        let _ = menu.autostart.set_enabled(true);
                        let _ = menu.autostart.set_checked(actual);
                    }
                    Err(_) => {
                        let _ = menu.autostart.set_enabled(false);
                    }
                }
            }
            return Err(error);
        }
    };
    if let Some(menu) = app.try_state::<TrayMenuState>() {
        let _ = menu.autostart.set_enabled(true);
        let _ = menu.autostart.set_checked(actual);
    }
    let _ = app.emit_to("settings", LAUNCH_AT_LOGIN_CHANGED_EVENT, actual);
    Ok(actual)
}

#[tauri::command]
fn set_launch_at_login(enabled: bool, app: AppHandle) -> Result<bool, String> {
    set_launch_at_login_internal(&app, enabled)
}

fn setup_application_menu(app: &tauri::App) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let menu = Menu::default(app.handle())?;
        let settings = MenuItem::with_id(
            app,
            APP_SETTINGS_MENU_ID,
            "Settings…",
            true,
            Some("CmdOrCtrl+,"),
        )?;
        let separator = PredefinedMenuItem::separator(app)?;
        if let Some(MenuItemKind::Submenu(application_menu)) = menu.items()?.into_iter().next() {
            application_menu.insert_items(&[&settings, &separator], 2)?;
        }
        app.set_menu(menu)?;
    }

    // Windows menus are window-scoped here so the frameless widget never
    // inherits a native menu bar and changes its client-area geometry.
    #[cfg(target_os = "windows")]
    {
        let menu = Menu::default(app.handle())?;
        let settings = MenuItem::with_id(
            app,
            APP_SETTINGS_MENU_ID,
            "Settings…",
            true,
            Some("CmdOrCtrl+,"),
        )?;
        let separator = PredefinedMenuItem::separator(app)?;
        menu.prepend_items(&[&settings, &separator])?;
        if let Some(settings_window) = app.get_webview_window("settings") {
            settings_window.set_menu(menu)?;
        }
    }

    app.on_menu_event(|app, event| {
        if settings_menu_route(event.id.as_ref()) == Some(SettingsMenuRoute::Application) {
            if let Err(error) = show_settings_window(app) {
                eprintln!("failed to open settings from application menu: {error}");
            }
        }
    });
    Ok(())
}

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let open_settings = MenuItem::with_id(
        app,
        TRAY_SETTINGS_MENU_ID,
        "Settings / 设置",
        true,
        None::<&str>,
    )?;
    let unlock = MenuItem::with_id(app, "unlock", "Unlock widget", true, None::<&str>)?;
    let pin = MenuItem::with_id(app, "pin", "Pin / Unpin Codex", true, None::<&str>)?;
    let toggle_mode =
        MenuItem::with_id(app, "toggle-mode", "Toggle widget mode", true, None::<&str>)?;
    let size_small =
        CheckMenuItem::with_id(app, "widget-size-small", "Small", true, false, None::<&str>)?;
    let size_medium = CheckMenuItem::with_id(
        app,
        "widget-size-medium",
        "Medium",
        true,
        true,
        None::<&str>,
    )?;
    let size_large =
        CheckMenuItem::with_id(app, "widget-size-large", "Large", true, false, None::<&str>)?;
    let widget_size = Submenu::with_items(
        app,
        "Widget size / 组件大小",
        true,
        &[&size_small, &size_medium, &size_large],
    )?;
    let language = MenuItem::with_id(
        app,
        "language",
        "Switch Language / 切换语言",
        true,
        None::<&str>,
    )?;
    let theme_system = CheckMenuItem::with_id(
        app,
        "theme-system",
        "Follow system",
        true,
        false,
        None::<&str>,
    )?;
    let theme_dark = CheckMenuItem::with_id(app, "theme-dark", "Dark", true, false, None::<&str>)?;
    let theme_light =
        CheckMenuItem::with_id(app, "theme-light", "Light", true, false, None::<&str>)?;
    let appearance = Submenu::with_items(
        app,
        "Appearance / 外观",
        true,
        &[&theme_system, &theme_dark, &theme_light],
    )?;
    let skin_default =
        CheckMenuItem::with_id(app, "skin-default", "Soft Light", true, false, None::<&str>)?;
    let skin_computer =
        CheckMenuItem::with_id(app, "skin-computer", "Computer", true, false, None::<&str>)?;
    let skin_glass = CheckMenuItem::with_id(app, "skin-glass", "Glass", true, false, None::<&str>)?;
    let skins = Submenu::with_items(
        app,
        "Skins / 皮肤",
        true,
        &[&skin_default, &skin_computer, &skin_glass],
    )?;
    let theme = Submenu::with_items(app, "Theme / 主题", true, &[&appearance, &skins])?;
    let (autostart_enabled, autostart_available) = {
        let manager = app.autolaunch();
        match read_launch_at_login(manager.inner()) {
            Ok(enabled) => (enabled, true),
            Err(error) => {
                eprintln!("failed to initialize autostart menu state: {error}");
                (false, false)
            }
        }
    };
    let autostart = CheckMenuItem::with_id(
        app,
        "autostart",
        "Start at login",
        autostart_available,
        autostart_enabled,
        None::<&str>,
    )?;
    #[cfg(debug_assertions)]
    let test_short_window = CheckMenuItem::with_id(
        app,
        "debug-short-window",
        "Test: simulate 5-hour quota",
        true,
        false,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let initial_language = app
        .try_state::<AppState>()
        .and_then(|state| {
            state
                .preferences
                .lock()
                .ok()
                .map(|prefs| prefs.language.clone())
        })
        .unwrap_or_else(|| "zh-CN".into());
    let initial_selected_skin = app
        .try_state::<AppState>()
        .and_then(|state| {
            state
                .preferences
                .lock()
                .ok()
                .map(|prefs| prefs.selected_skin.clone())
        })
        .unwrap_or_else(|| "glass".into());
    let initial_appearance = app
        .try_state::<AppState>()
        .and_then(|state| {
            state
                .preferences
                .lock()
                .ok()
                .map(|prefs| prefs.appearance.clone())
        })
        .unwrap_or_else(|| "system".into());
    let initial_widget_size = app
        .try_state::<AppState>()
        .and_then(|state| {
            state
                .preferences
                .lock()
                .ok()
                .map(|prefs| prefs.widget_size.clone())
        })
        .unwrap_or_else(|| "medium".into());
    let _ = skin_default.set_checked(initial_selected_skin == "default");
    let _ = skin_computer.set_checked(initial_selected_skin == "computer");
    let _ = skin_glass.set_checked(initial_selected_skin == "glass");
    let _ = theme_system.set_checked(initial_appearance == "system");
    let _ = theme_dark.set_checked(initial_appearance == "dark");
    let _ = theme_light.set_checked(initial_appearance == "light");
    let _ = size_small.set_checked(initial_widget_size == "small");
    let _ = size_medium.set_checked(initial_widget_size == "medium");
    let _ = size_large.set_checked(initial_widget_size == "large");
    if initial_language != "en" {
        let _ = unlock.set_text("解锁悬浮窗");
        let _ = pin.set_text("固定 / 取消固定 Codex");
        let _ = toggle_mode.set_text("切换展开状态");
        let _ = widget_size.set_text("组件大小");
        let _ = size_small.set_text("小");
        let _ = size_medium.set_text("中");
        let _ = size_large.set_text("大");
        let _ = language.set_text("Switch to English");
        let _ = theme.set_text("主题");
        let _ = appearance.set_text("外观");
        let _ = theme_system.set_text("跟随系统");
        let _ = theme_dark.set_text("深色");
        let _ = theme_light.set_text("浅色");
        let _ = skins.set_text("皮肤");
        let _ = skin_default.set_text("柔光");
        let _ = skin_computer.set_text("电脑");
        let _ = skin_glass.set_text("默认");
        let _ = autostart.set_text("开机启动");
        let _ = quit.set_text("退出");
    }
    if initial_language == "en" {
        let _ = theme.set_text("Theme");
        let _ = appearance.set_text("Appearance");
        let _ = theme_system.set_text("Follow system");
        let _ = theme_dark.set_text("Dark");
        let _ = theme_light.set_text("Light");
        let _ = skins.set_text("Skins");
        let _ = skin_default.set_text("Soft Light");
        let _ = skin_computer.set_text("Computer");
        let _ = skin_glass.set_text("Default");
        let _ = widget_size.set_text("Widget size");
        let _ = size_small.set_text("Small");
        let _ = size_medium.set_text("Medium");
        let _ = size_large.set_text("Large");
        let _ = autostart.set_text("Start at login");
    }
    #[cfg(debug_assertions)]
    // Keep Settings as a direct tray entry. The previous expandable Settings
    // submenu is intentionally omitted; its controls live in the settings window.
    let menu = Menu::with_items(app, &[&open_settings, &theme, &test_short_window, &quit])?;
    #[cfg(not(debug_assertions))]
    let menu = Menu::with_items(app, &[&open_settings, &theme, &quit])?;
    let mut builder = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .tooltip("Quota Pro")
        // Use the transparent waveform as a native macOS template image so
        // AppKit supplies the same sizing, antialiasing, and light/dark tint
        // treatment used by other menu-bar applications.
        .icon_as_template(true);
    let tray_icon = image::load_from_memory(include_bytes!("../icons/tray-white.png"))
        .ok()
        .map(|icon| {
            let icon = icon.to_rgba8();
            let (width, height) = icon.dimensions();
            TauriImage::new_owned(icon.into_raw(), width, height)
        });
    if let Some(icon) = tray_icon {
        builder = builder.icon(icon);
    } else if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    let theme_system_state = theme_system.clone();
    let theme_dark_state = theme_dark.clone();
    let theme_light_state = theme_light.clone();
    let skin_default_menu = skin_default.clone();
    let skin_computer_menu = skin_computer.clone();
    let skin_glass_menu = skin_glass.clone();
    let autostart_menu = autostart.clone();
    #[cfg(debug_assertions)]
    let test_short_window_menu = test_short_window.clone();
    builder
        .on_menu_event(move |app, event| match event.id.as_ref() {
            id if settings_menu_route(id) == Some(SettingsMenuRoute::Tray) => {
                if let Err(error) = show_settings_window(app) {
                    eprintln!("failed to open settings from tray menu: {error}");
                }
            }
            "skin-default" | "skin-computer" | "skin-glass" => {
                let requested_skin = event.id.as_ref().trim_start_matches("skin-");
                if let Some(state) = app.try_state::<AppState>() {
                    if let Ok(mut preferences) = state.preferences.lock() {
                        let mut requested = preferences.clone();
                        requested.selected_skin = requested_skin.into();
                        let normalized = requested.normalized();
                        if persist_preferences(&state.preferences_path, &normalized).is_ok() {
                            *preferences = normalized.clone();
                            let _ = skin_default_menu
                                .set_checked(normalized.selected_skin == "default");
                            let _ = skin_computer_menu
                                .set_checked(normalized.selected_skin == "computer");
                            let _ =
                                skin_glass_menu.set_checked(normalized.selected_skin == "glass");
                            emit_preferences_changed(app, &normalized);
                        }
                    }
                }
            }
            "debug-short-window" =>
            {
                #[cfg(debug_assertions)]
                if let Some(state) = app.try_state::<AppState>() {
                    if let Ok(mut enabled) = state.simulate_short_window_for_testing.lock() {
                        *enabled = !*enabled;
                        let _ = test_short_window_menu.set_checked(*enabled);
                        let _ = app.emit_to("widget", "refresh-requested", ());
                    }
                }
            }
            "theme-system" | "theme-dark" | "theme-light" => {
                if let Some(state) = app.try_state::<AppState>() {
                    if let Ok(mut prefs) = state.preferences.lock() {
                        prefs.appearance = match event.id.as_ref() {
                            "theme-dark" => "dark".into(),
                            "theme-light" => "light".into(),
                            _ => "system".into(),
                        };
                        let normalized = prefs.clone().normalized();
                        *prefs = normalized.clone();
                        if persist_preferences(&state.preferences_path, &normalized).is_ok() {
                            let _ =
                                theme_system_state.set_checked(normalized.appearance == "system");
                            let _ = theme_dark_state.set_checked(normalized.appearance == "dark");
                            let _ = theme_light_state.set_checked(normalized.appearance == "light");
                            emit_preferences_changed(app, &normalized);
                        }
                    }
                }
            }
            "autostart" => {
                let manager = app.autolaunch();
                let result = read_launch_at_login(manager.inner())
                    .and_then(|enabled| set_launch_at_login_internal(app, !enabled));
                match result {
                    Ok(actual) => {
                        let _ = autostart_menu.set_checked(actual);
                    }
                    Err(error) => {
                        eprintln!("autostart update failed: {error}");
                        let manager = app.autolaunch();
                        match read_launch_at_login(manager.inner()) {
                            Ok(actual) => {
                                let _ = autostart_menu.set_enabled(true);
                                let _ = autostart_menu.set_checked(actual);
                            }
                            Err(read_error) => {
                                eprintln!(
                                    "autostart state unavailable after failed update: {read_error}"
                                );
                                let _ = autostart_menu.set_enabled(false);
                            }
                        }
                    }
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    app.manage(TrayMenuState {
        autostart,
        size_small,
        size_medium,
        size_large,
        theme_system,
        theme_dark,
        theme_light,
        skin_default,
        skin_computer,
        skin_glass,
    });
    Ok(())
}

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("widget") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(WindowStateBuilder::default().build())
        .setup(|app| {
            let data_dir = app.path().app_config_dir()?;
            let preferences_path = data_dir.join("preferences.json");
            let preferences =
                load_preferences_with_skin_reconciliation(&data_dir, &preferences_path);
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(12))
                .redirect(reqwest::redirect::Policy::none())
                .user_agent("QuotaFloat/0.1")
                .build()
                .expect("static HTTP client configuration must be valid");
            app.manage(AppState {
                client,
                preferences: Mutex::new(preferences.clone()),
                preferences_path,
                fetch_lock: tokio::sync::Mutex::new(()),
                snapshot_cache: Mutex::new(None),
                #[cfg(debug_assertions)]
                simulate_short_window_for_testing: Mutex::new(false),
                geometry: Mutex::new(None),
                drag_mode: Mutex::new(None),
                resize_state: Mutex::new(None),
            });
            setup_application_menu(app)?;
            // Window-state restores the last native rectangle before this
            // setup hook runs. Apply the persisted widget mode first, then
            // reveal the transparent window so WebView content never paints
            // an expanded card inside a stale compact rectangle.
            if let Some(state) = app.try_state::<AppState>() {
                if let Ok(mode) = mode_from_preference(&preferences.widget_mode) {
                    let _ = set_widget_mode_internal(mode, None, app.handle(), state.inner(), None);
                }
            }
            if setup_tray(app).is_err() {
                eprintln!("tray setup failed; enabling taskbar fallback");
                if let Some(window) = app.get_webview_window("widget") {
                    let _ = window.set_skip_taskbar(false);
                }
            }
            if preferences.locked {
                let _ = apply_lock(app.handle(), true);
            }
            if let Some(window) = app.get_webview_window("widget") {
                clear_widget_window_background(&window);
                let _ = window.set_always_on_top(preferences.always_on_top);
                let _ = window.show();
                // Showing the WKWebView can recreate its backing surface on
                // macOS; clear it once more after the native surface exists.
                clear_widget_window_background(&window);
                // Let tao finish its did-finish-launching callback before
                // inserting the AppKit visual-effect sibling. Mutating the
                // WKWebView hierarchy from inside that callback can make
                // macOS 15 unwind through tao's C callback boundary.
                #[cfg(target_os = "macos")]
                {
                    let material_window = window.clone();
                    let material_preferences = preferences.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(250));
                        sync_native_glass_material(&material_window, &material_preferences);
                    });
                }
                // A saved window position can be outside the active monitor while
                // iterating in development. Keep the test widget discoverable.
                #[cfg(debug_assertions)]
                {
                    let _ = window.center();
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            #[cfg(debug_assertions)]
            {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(800));
                    if let Some(window) = handle.get_webview_window("widget") {
                        let _ = window.set_position(PhysicalPosition::new(120, 120));
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshots,
            refresh_snapshots,
            set_widget_mode,
            sync_widget_layout,
            set_widget_size,
            set_widget_dimensions,
            begin_widget_resize,
            preview_widget_resize,
            finish_widget_resize,
            cancel_widget_resize,
            reset_widget_size,
            begin_widget_drag,
            finish_widget_drag,
            get_preferences,
            set_preferences,
            set_widget_locked,
            set_widget_always_on_top,
            sync_widget_appearance,
            import_custom_skin,
            get_custom_skin_asset,
            update_custom_skin,
            delete_custom_skin,
            select_skin,
            show_settings,
            get_launch_at_login,
            set_launch_at_login,
            get_platform_capabilities
        ])
        .on_tray_icon_event(|app, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if let Some(window) = app.get_webview_window("widget") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if close_behavior(window.label()) == CloseBehavior::Hide {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("failed to build Quota Pro");
    app.run(|app_handle, event| {
        if matches!(event, tauri::RunEvent::Resumed) {
            let _ = app_handle.emit_to("widget", "refresh-requested", ());
        }
    });
}
