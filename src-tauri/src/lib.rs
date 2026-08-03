mod codex;
mod license;
mod models;

use std::{
    fs,
    io::Write,
    path::PathBuf,
    sync::{Mutex, MutexGuard},
    time::{Duration, Instant},
};

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use license::{
    device_request_code, parse_and_verify, SupporterStatus, BLUR_SKIN_ID, COMPUTER_SKIN_ID,
};
#[cfg(debug_assertions)]
use models::UsageWindow;
use models::{ProviderSnapshot, WidgetPreferences};
use serde::Deserialize;
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Listener, Manager, PhysicalPosition, PhysicalSize, State, WindowEvent,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_updater::UpdaterExt;
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

#[derive(Clone, Copy)]
struct WidgetRect {
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
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
}

#[derive(Clone, Copy)]
struct WidgetResizeState {
    mode: WidgetMode,
    edge: ResizeEdge,
    start_rect: WidgetRect,
    start_collapsed_rect: WidgetRect,
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
    update_available: Mutex<bool>,
}

fn update_menu_label(language: &str, update_available: bool) -> &'static str {
    match (language == "en", update_available) {
        (true, true) => "🟢 Check for updates",
        (false, true) => "🟢 检查更新",
        (true, false) => "Check for updates",
        (false, false) => "检查更新",
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

const SUPPORTER_PROMPT_DELAY_DAYS: i64 = 3;

fn should_show_supporter_prompt(
    preferences: &mut WidgetPreferences,
    now: DateTime<Utc>,
    has_supporter_license: bool,
) -> bool {
    if has_supporter_license || preferences.supporter_prompt_shown_at.is_some() {
        return false;
    }
    let first_seen = preferences
        .supporter_prompt_first_seen_at
        .as_deref()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc));
    let Some(first_seen) = first_seen else {
        preferences.supporter_prompt_first_seen_at = Some(now.to_rfc3339());
        return false;
    };
    if now.signed_duration_since(first_seen) < ChronoDuration::days(SUPPORTER_PROMPT_DELAY_DAYS) {
        return false;
    }
    preferences.supporter_prompt_shown_at = Some(now.to_rfc3339());
    true
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

fn clamp_position_to_monitor(
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    monitor: &tauri::Monitor,
    safe_inset: i32,
) -> PhysicalPosition<i32> {
    let monitor_position = monitor.position();
    let monitor_size = monitor.size();
    let left = monitor_position.x;
    let top = monitor_position.y;
    let right = left + monitor_size.width as i32;
    let bottom = top + monitor_size.height as i32;
    PhysicalPosition::new(
        position
            .x
            .clamp(left - safe_inset, right - size.width as i32 + safe_inset),
        position
            .y
            .clamp(top - safe_inset, bottom - size.height as i32 + safe_inset),
    )
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

fn expanded_position_in_bounds(
    collapsed: WidgetRect,
    expanded_size: PhysicalSize<u32>,
    bounds_position: PhysicalPosition<i32>,
    bounds_size: PhysicalSize<u32>,
    safe_inset: i32,
) -> PhysicalPosition<i32> {
    let monitor_right = bounds_position.x + bounds_size.width as i32;
    let monitor_bottom = bounds_position.y + bounds_size.height as i32;
    let min_x = bounds_position.x - safe_inset;
    let min_y = bounds_position.y - safe_inset;
    let max_x = (monitor_right - expanded_size.width as i32 + safe_inset).max(min_x);
    let max_y = (monitor_bottom - expanded_size.height as i32 + safe_inset).max(min_y);
    PhysicalPosition::new(
        collapsed.position.x.clamp(min_x, max_x),
        collapsed.position.y.clamp(min_y, max_y),
    )
}

fn expanded_position(
    collapsed: WidgetRect,
    expanded_size: PhysicalSize<u32>,
    monitor: &tauri::Monitor,
    work_area: Option<WorkAreaPayload>,
    safe_inset: i32,
) -> PhysicalPosition<i32> {
    let (bounds_position, bounds_size) = work_area
        .map(|area| {
            (
                PhysicalPosition::new(area.position.x, area.position.y),
                PhysicalSize::new(area.size.width, area.size.height),
            )
        })
        .unwrap_or_else(|| {
            let area = monitor.work_area();
            (
                PhysicalPosition::new(area.position.x, area.position.y),
                PhysicalSize::new(area.size.width, area.size.height),
            )
        });
    expanded_position_in_bounds(
        collapsed,
        expanded_size,
        bounds_position,
        bounds_size,
        safe_inset,
    )
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
    let anchor = previous
        .map(|value| value.collapsed_rect.position)
        .unwrap_or(current.position);
    let Some(monitor) = monitor else {
        let size = if matches!(mode, WidgetMode::Collapsed) {
            collapsed_size
        } else {
            expanded_size
        };
        window
            .set_size(size)
            .map_err(|_| "failed to resize widget".to_string())?;
        let mut preferences = preferences_lock_value(state).clone();
        preferences.widget_mode = mode_preference(mode).into();
        persist_preferences(&state.preferences_path, &preferences)?;
        *preferences_lock_value(state) = preferences.clone();
        let _ = app.emit_to("widget", "preferences-changed", preferences.clone());
        return Ok(preferences);
    };
    let anchor = WidgetRect {
        position: clamp_position_to_monitor(anchor, collapsed_size, &monitor, safe_inset),
        size: collapsed_size,
    };
    let (target_position, target_size) = match mode {
        WidgetMode::Collapsed => (anchor.position, collapsed_size),
        WidgetMode::Expanded => (
            expanded_position(anchor, expanded_size, &monitor, work_area, safe_inset),
            expanded_size,
        ),
    };
    window
        .set_position(target_position)
        .map_err(|_| "failed to position widget".to_string())?;
    window
        .set_size(target_size)
        .map_err(|_| "failed to resize widget".to_string())?;
    if let Ok(mut geometry) = state.geometry.lock() {
        *geometry = Some(WidgetGeometryState {
            mode,
            collapsed_rect: anchor,
        });
    }
    let mut preferences = preferences_lock_value(state).clone();
    preferences.widget_mode = mode_preference(mode).into();
    persist_preferences(&state.preferences_path, &preferences)?;
    *preferences_lock_value(state) = preferences.clone();
    let _ = app.emit_to("widget", "preferences-changed", preferences.clone());
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
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<WidgetPreferences, String> {
    set_widget_mode_internal(mode_from_preference(&mode)?, work_area, &app, state.inner())
}

fn set_widget_size_internal(
    size: WidgetSize,
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
    let factor = widget_size_factor(size);
    next_preferences.compact_size = COLLAPSED_LOGICAL_SIZE * factor;
    next_preferences.expanded_size = EXPANDED_LOGICAL_SIZE * factor;
    next_preferences.widget_size = widget_size_preference(size).into();
    let (collapsed_size, expanded_size) =
        widget_dimensions(&next_preferences, scale_factor, safe_inset as u32);
    let previous = state.geometry.lock().ok().and_then(|value| *value);
    let mode = previous
        .map(|value| value.mode)
        .or_else(|| mode_from_preference(&current_preferences.widget_mode).ok())
        .unwrap_or_else(|| infer_mode(current, old_collapsed_size));
    let anchor_position = previous
        .map(|value| value.collapsed_rect.position)
        .unwrap_or(current.position);

    let Some(monitor) = monitor else {
        let target_size = if matches!(mode, WidgetMode::Collapsed) {
            collapsed_size
        } else {
            expanded_size
        };
        window
            .set_size(target_size)
            .map_err(|_| "failed to resize widget".to_string())?;
        let preferences = next_preferences;
        persist_preferences(&state.preferences_path, &preferences)?;
        *preferences_lock_value(state) = preferences.clone();
        if let Ok(mut geometry) = state.geometry.lock() {
            *geometry = Some(WidgetGeometryState {
                mode,
                collapsed_rect: WidgetRect {
                    position: anchor_position,
                    size: collapsed_size,
                },
            });
        }
        let _ = app.emit_to("widget", "preferences-changed", preferences.clone());
        return Ok(preferences);
    };

    let anchor = WidgetRect {
        position: clamp_position_to_monitor(anchor_position, collapsed_size, &monitor, safe_inset),
        size: collapsed_size,
    };
    let (target_position, target_size) = match mode {
        WidgetMode::Collapsed => (anchor.position, collapsed_size),
        WidgetMode::Expanded => (
            expanded_position(anchor, expanded_size, &monitor, work_area, safe_inset),
            expanded_size,
        ),
    };
    window
        .set_position(target_position)
        .map_err(|_| "failed to position widget".to_string())?;
    window
        .set_size(target_size)
        .map_err(|_| "failed to resize widget".to_string())?;
    if let Ok(mut geometry) = state.geometry.lock() {
        *geometry = Some(WidgetGeometryState {
            mode,
            collapsed_rect: anchor,
        });
    }
    let preferences = next_preferences;
    persist_preferences(&state.preferences_path, &preferences)?;
    *preferences_lock_value(state) = preferences.clone();
    let _ = app.emit_to("widget", "preferences-changed", preferences.clone());
    Ok(preferences)
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

fn apply_widget_resize(
    size: f64,
    work_area: Option<WorkAreaPayload>,
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
    let (monitor, scale_factor) = monitor_and_scale(&window)?;
    let safe_inset = safe_inset_for_current_appearance(state, scale_factor);
    let logical_size = clamp_logical_size(session.mode, size);
    let target = widget_window_size(logical_size, scale_factor, safe_inset);
    let target_size = PhysicalSize::new(target, target);
    let raw_position = resize_position(session.start_rect, target_size, session.edge);
    let target_position = clamp_position_in_bounds(
        raw_position,
        target_size,
        bounds_for_resize(monitor.as_ref(), work_area),
        safe_inset as i32,
    );
    window
        .set_position(target_position)
        .map_err(|_| "failed to position widget during resize".to_string())?;
    window
        .set_size(target_size)
        .map_err(|_| "failed to resize widget".to_string())?;

    if let Ok(mut geometry) = state.geometry.lock() {
        *geometry = Some(WidgetGeometryState {
            mode: session.mode,
            collapsed_rect: if matches!(session.mode, WidgetMode::Collapsed) {
                WidgetRect {
                    position: target_position,
                    size: target_size,
                }
            } else {
                session.start_collapsed_rect
            },
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
    preferences.widget_size = "custom".into();
    preferences.widget_mode = mode_preference(session.mode).into();
    persist_preferences(&state.preferences_path, &preferences)?;
    *preferences_lock_value(state) = preferences.clone();
    let _ = app.emit_to("widget", "preferences-changed", preferences.clone());
    Ok(Some(preferences))
}

#[tauri::command]
fn begin_widget_resize(
    mode: String,
    edge: String,
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
    let (_, scale_factor) = monitor_and_scale(&window)?;
    let safe_inset = safe_inset_for_current_appearance(state.inner(), scale_factor);
    let (collapsed_size, _) = widget_dimensions(&preferences, scale_factor, safe_inset);
    let geometry = state.geometry.lock().ok().and_then(|value| *value);
    // The renderer starts a session only for the mode it is displaying. Use
    // that request as the source of truth; geometry can briefly lag during a
    // mode transition and must not widen the compact range or vice versa.
    let actual_mode = requested_mode;
    let start_collapsed_rect = geometry
        .map(|value| value.collapsed_rect)
        .unwrap_or_else(|| WidgetRect {
            position: current.position,
            size: collapsed_size,
        });
    let session = WidgetResizeState {
        mode: actual_mode,
        edge: resize_edge_from_preference(&edge)?,
        start_rect: current,
        start_collapsed_rect,
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
    work_area: Option<WorkAreaPayload>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let _ = apply_widget_resize(size, work_area, &app, state.inner(), false)?;
    Ok(())
}

#[tauri::command]
fn finish_widget_resize(
    size: f64,
    work_area: Option<WorkAreaPayload>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<WidgetPreferences, String> {
    let result = apply_widget_resize(size, work_area, &app, state.inner(), true)?
        .ok_or_else(|| "failed to commit widget resize".to_string())?;
    *state
        .resize_state
        .lock()
        .map_err(|_| "resize state unavailable".to_string())? = None;
    Ok(result)
}

#[tauri::command]
fn cancel_widget_resize(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let session = state
        .resize_state
        .lock()
        .map_err(|_| "resize state unavailable".to_string())?
        .take();
    if let Some(session) = session {
        if let Some(window) = app.get_webview_window("widget") {
            let _ = window.set_position(session.start_rect.position);
            let _ = window.set_size(session.start_rect.size);
        }
        if let Ok(mut geometry) = state.geometry.lock() {
            *geometry = Some(WidgetGeometryState {
                mode: session.mode,
                collapsed_rect: session.start_collapsed_rect,
            });
        }
    }
    Ok(())
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
        let position = expanded_position_in_bounds(
            rect(1844, 964, 80),
            PhysicalSize::new(314, 314),
            PhysicalPosition::new(0, 0),
            PhysicalSize::new(1920, 1040),
            4,
        );
        assert_eq!(position, PhysicalPosition::new(1610, 730));
    }

    #[test]
    fn expansion_handles_negative_origin_work_areas() {
        let position = expanded_position_in_bounds(
            rect(-1284, -4, 80),
            PhysicalSize::new(314, 314),
            PhysicalPosition::new(-1280, 0),
            PhysicalSize::new(1280, 984),
            4,
        );
        assert_eq!(position, PhysicalPosition::new(-1284, -4));
    }

    #[test]
    fn expansion_clamps_only_when_the_expanded_window_would_overflow() {
        let position = expanded_position_in_bounds(
            rect(1750, 900, 80),
            PhysicalSize::new(314, 314),
            PhysicalPosition::new(0, 0),
            PhysicalSize::new(1920, 1040),
            4,
        );
        assert_eq!(position, PhysicalPosition::new(1610, 730));
    }

    #[test]
    fn expansion_preserves_an_arbitrary_compact_anchor() {
        let position = expanded_position_in_bounds(
            rect(720, 420, 80),
            PhysicalSize::new(314, 314),
            PhysicalPosition::new(0, 0),
            PhysicalSize::new(1920, 1040),
            4,
        );
        assert_eq!(position, PhysicalPosition::new(720, 420));
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
            let next_position = clamp_position_to_monitor(
                current.position,
                collapsed_size,
                &monitor,
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
                });
            }
        }
        WidgetMode::Expanded => {
            let current_position = clamp_position_to_monitor(
                current.position,
                expanded_size,
                &monitor,
                safe_inset as i32,
            );
            window
                .set_position(current_position)
                .map_err(|_| "failed to position widget".to_string())?;
            if let Ok(mut geometry) = state.geometry.lock() {
                if let Some(mut value) = *geometry {
                    value.mode = WidgetMode::Expanded;
                    value.collapsed_rect.position = current_position;
                    *geometry = Some(value);
                }
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

#[tauri::command]
fn set_preferences(
    preferences: WidgetPreferences,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let current = preferences_lock(&state).clone();
    let preferences = renderer_preferences(&current, preferences);
    persist_preferences(&state.preferences_path, &preferences)?;
    *preferences_lock(&state) = preferences;
    Ok(())
}

fn renderer_preferences(
    current: &WidgetPreferences,
    requested: WidgetPreferences,
) -> WidgetPreferences {
    // License state can only be changed by the commands that validate it.
    // Never trust an arbitrary renderer payload to unlock a supporter skin.
    let mut preferences = requested.normalized();
    preferences.license = current.license.clone();
    preferences.licenses = current.licenses.clone();
    preferences.unlocked_skin = current.unlocked_skin.clone();
    preferences.unlocked_skins = current.unlocked_skins.clone();
    preferences.selected_skin = current.selected_skin.clone();
    preferences.supporter_prompt_first_seen_at = current.supporter_prompt_first_seen_at.clone();
    preferences.supporter_prompt_shown_at = current.supporter_prompt_shown_at.clone();
    preferences
}

#[cfg(test)]
mod supporter_preference_tests {
    use super::*;

    #[test]
    fn renderer_preferences_cannot_unlock_or_select_a_supporter_skin() {
        let current = WidgetPreferences::default();
        let requested = WidgetPreferences {
            license: Some("forged".into()),
            unlocked_skin: Some(BLUR_SKIN_ID.into()),
            selected_skin: BLUR_SKIN_ID.into(),
            ..WidgetPreferences::default()
        };
        let saved = renderer_preferences(&current, requested);
        assert_eq!(saved.license, None);
        assert_eq!(saved.unlocked_skin, None);
        assert_eq!(saved.selected_skin, "default");
    }

    #[test]
    fn forged_stored_unlock_flags_do_not_activate_supporter_skins() {
        let preferences = WidgetPreferences {
            unlocked_skin: Some(BLUR_SKIN_ID.into()),
            unlocked_skins: vec![BLUR_SKIN_ID.into(), COMPUTER_SKIN_ID.into()],
            selected_skin: COMPUTER_SKIN_ID.into(),
            ..WidgetPreferences::default()
        };
        let status = supporter_status(&preferences, "QF1-FORGED-DEVICE-CODE");
        assert!(!status.active);
        assert_eq!(status.available_skins, vec!["default"]);
        assert_eq!(status.selected_skin, "default");
    }

    #[test]
    fn supporter_prompt_waits_three_days_then_only_shows_once() {
        let first_seen = Utc::now() - ChronoDuration::days(SUPPORTER_PROMPT_DELAY_DAYS);
        let mut preferences = WidgetPreferences {
            supporter_prompt_first_seen_at: Some(first_seen.to_rfc3339()),
            ..WidgetPreferences::default()
        };
        assert!(should_show_supporter_prompt(
            &mut preferences,
            Utc::now(),
            false
        ));
        assert!(preferences.supporter_prompt_shown_at.is_some());
        assert!(!should_show_supporter_prompt(
            &mut preferences,
            Utc::now(),
            false
        ));
    }

    #[test]
    fn supporter_prompt_never_shows_for_an_active_supporter() {
        let mut preferences = WidgetPreferences {
            supporter_prompt_first_seen_at: Some(
                (Utc::now() - ChronoDuration::days(4)).to_rfc3339(),
            ),
            ..WidgetPreferences::default()
        };
        assert!(!should_show_supporter_prompt(
            &mut preferences,
            Utc::now(),
            true
        ));
        assert!(preferences.supporter_prompt_shown_at.is_none());
    }

    #[test]
    fn verified_skin_set_removes_forged_supporter_flags() {
        let mut preferences = WidgetPreferences {
            unlocked_skin: Some(COMPUTER_SKIN_ID.into()),
            unlocked_skins: vec![BLUR_SKIN_ID.into(), COMPUTER_SKIN_ID.into()],
            selected_skin: COMPUTER_SKIN_ID.into(),
            ..WidgetPreferences::default()
        };
        assert!(reconcile_supporter_fields(
            &mut preferences,
            vec![BLUR_SKIN_ID.into()]
        ));
        assert_eq!(preferences.unlocked_skin.as_deref(), Some(BLUR_SKIN_ID));
        assert_eq!(preferences.unlocked_skins, vec![BLUR_SKIN_ID]);
        assert_eq!(preferences.selected_skin, "default");
    }
}

fn verified_supporter_documents(
    preferences: &WidgetPreferences,
    request_code: &str,
) -> Vec<license::LicenseDocument> {
    let mut raw_licenses = preferences.licenses.clone();
    if let Some(legacy) = preferences.license.as_ref() {
        if !raw_licenses.contains(legacy) {
            raw_licenses.push(legacy.clone());
        }
    }
    let mut documents = Vec::new();
    for raw in raw_licenses {
        if let Ok(document) = parse_and_verify(&raw, request_code) {
            if !documents
                .iter()
                .any(|known: &license::LicenseDocument| known.skin_id == document.skin_id)
            {
                documents.push(document);
            }
        }
    }
    documents
}

fn reconcile_supporter_fields(
    preferences: &mut WidgetPreferences,
    mut verified_skins: Vec<String>,
) -> bool {
    verified_skins.sort();
    verified_skins.dedup();
    let selected_skin = if preferences.selected_skin == "default"
        || verified_skins
            .iter()
            .any(|skin| skin == &preferences.selected_skin)
    {
        preferences.selected_skin.clone()
    } else {
        "default".into()
    };
    let unlocked_skin = verified_skins.first().cloned();
    let changed = preferences.unlocked_skin != unlocked_skin
        || preferences.unlocked_skins != verified_skins
        || preferences.selected_skin != selected_skin;
    preferences.unlocked_skin = unlocked_skin;
    preferences.unlocked_skins = verified_skins;
    preferences.selected_skin = selected_skin;
    changed
}

fn reconcile_verified_supporter_fields(
    preferences: &mut WidgetPreferences,
    request_code: &str,
) -> bool {
    let verified_skins = verified_supporter_documents(preferences, request_code)
        .into_iter()
        .map(|document| document.skin_id)
        .collect();
    reconcile_supporter_fields(preferences, verified_skins)
}

fn supporter_status(preferences: &WidgetPreferences, request_code: &str) -> SupporterStatus {
    let documents = verified_supporter_documents(preferences, request_code);
    if !documents.is_empty() {
        let unlocked_skins = documents
            .iter()
            .map(|document| document.skin_id.clone())
            .collect::<Vec<_>>();
        let selected_skin = if preferences.selected_skin == "default"
            || unlocked_skins
                .iter()
                .any(|skin| skin == &preferences.selected_skin)
        {
            preferences.selected_skin.clone()
        } else {
            "default".into()
        };
        SupporterStatus {
            request_code: request_code.into(),
            active: true,
            message: "Supporter licenses are active.".into(),
            unlocked_skin: unlocked_skins.first().cloned(),
            unlocked_skins: unlocked_skins.clone(),
            selected_skin,
            available_skins: std::iter::once("default".into())
                .chain(unlocked_skins)
                .collect(),
        }
    } else {
        SupporterStatus {
            request_code: request_code.into(),
            active: false,
            message: "No supporter license has been activated on this device.".into(),
            unlocked_skin: None,
            unlocked_skins: Vec::new(),
            selected_skin: "default".into(),
            available_skins: vec!["default".into()],
        }
    }
}

#[tauri::command]
fn get_supporter_status(state: State<'_, AppState>) -> Result<SupporterStatus, String> {
    let request_code = device_request_code()?;
    let mut preferences = preferences_lock(&state);
    let changed = reconcile_verified_supporter_fields(&mut preferences, &request_code);
    let status = supporter_status(&preferences, &request_code);
    if changed {
        // Returning the request code must not depend on a best-effort cleanup
        // of forged or obsolete supporter flags. Otherwise a transient
        // file-write failure hides the device code even though it was
        // generated safely.
        if persist_preferences(&state.preferences_path, &preferences).is_err() {
            eprintln!("failed to persist supporter skin cleanup");
        }
    }
    Ok(status)
}

#[tauri::command]
fn activate_supporter_license(
    license: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<SupporterStatus, String> {
    let request_code = device_request_code()?;
    let document = parse_and_verify(&license, &request_code)?;
    let mut preferences = preferences_lock(&state);
    preferences.licenses.retain(|raw| {
        serde_json::from_str::<license::LicenseDocument>(raw)
            .map(|existing| existing.skin_id != document.skin_id)
            .unwrap_or(true)
    });
    preferences.licenses.push(license.trim().into());
    preferences.unlocked_skins.push(document.skin_id.clone());
    let mut normalized = preferences.clone().normalized();
    normalized.selected_skin = document.skin_id;
    *preferences = normalized;
    persist_preferences(&state.preferences_path, &preferences)?;
    let saved = preferences.clone();
    let _ = app.emit_to("widget", "preferences-changed", saved.clone());
    let _ = app.emit("supporter-skin-changed", saved.selected_skin.clone());
    let status = supporter_status(&saved, &request_code);
    let _ = app.emit("supporter-skins-changed", status.clone());
    Ok(status)
}

#[tauri::command]
fn select_supporter_skin(
    skin_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<SupporterStatus, String> {
    let request_code = device_request_code()?;
    let mut preferences = preferences_lock(&state);
    if skin_id == "default" {
        preferences.selected_skin = "default".into();
    } else if matches!(skin_id.as_str(), BLUR_SKIN_ID | COMPUTER_SKIN_ID) {
        let status = supporter_status(&preferences, &request_code);
        if !status
            .available_skins
            .iter()
            .any(|available| available == &skin_id)
        {
            return Err("this skin is not activated on this device".into());
        }
        preferences.selected_skin = skin_id;
    } else {
        return Err("unknown supporter skin".into());
    }
    persist_preferences(&state.preferences_path, &preferences)?;
    let saved = preferences.clone();
    let _ = app.emit_to("widget", "preferences-changed", saved.clone());
    let _ = app.emit("supporter-skin-changed", saved.selected_skin.clone());
    Ok(supporter_status(&saved, &request_code))
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
    let _ = app.emit_to("widget", "preferences-changed", next.clone());
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
    window
        .set_size(target_size)
        .map_err(|_| "failed to resize widget for appearance".to_string())
}

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show / Hide", true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh", "Refresh now", true, None::<&str>)?;
    let update = MenuItem::with_id(app, "update", "Check for updates", true, None::<&str>)?;
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
    // Keep every built-in supporter skin visible. Selecting one that is not
    // activated on this device opens the supporter window instead.
    let supporter_blur = CheckMenuItem::with_id(
        app,
        "supporter-skin-blur",
        "Blur",
        true,
        false,
        None::<&str>,
    )?;
    let supporter_computer = CheckMenuItem::with_id(
        app,
        "supporter-skin-computer",
        "Computer",
        true,
        false,
        None::<&str>,
    )?;
    let supporter_skins = Submenu::with_items(
        app,
        "Supporter skins / 支持者皮肤",
        true,
        &[&supporter_blur, &supporter_computer],
    )?;
    let supporter_skins_top = MenuItem::with_id(
        app,
        "supporter-skins-top",
        "Support developer (skins) / 赞赏开发者（皮肤）",
        true,
        None::<&str>,
    )?;
    // The default skin has exactly three mutually exclusive appearance
    // choices. Selecting any one also restores the free default skin.
    let default_skin = Submenu::with_items(
        app,
        "Default skin / 默认皮肤",
        true,
        &[&theme_system, &theme_dark, &theme_light],
    )?;
    let theme = Submenu::with_items(
        app,
        "Theme / 主题",
        true,
        &[&default_skin, &supporter_skins],
    )?;
    let autostart_enabled = app.autolaunch().is_enabled().unwrap_or(false);
    let autostart = CheckMenuItem::with_id(
        app,
        "autostart",
        "Start at login",
        true,
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
    let settings = Submenu::with_items(
        app,
        "Settings / 设置",
        true,
        &[
            &unlock,
            &pin,
            &toggle_mode,
            &widget_size,
            &language,
            &autostart,
        ],
    )?;
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
        .unwrap_or_else(|| "default".into());
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
    let _ = supporter_blur.set_checked(initial_selected_skin == BLUR_SKIN_ID);
    let _ = supporter_computer.set_checked(initial_selected_skin == COMPUTER_SKIN_ID);
    let _ = theme_system.set_checked(initial_appearance == "system");
    let _ = theme_dark.set_checked(initial_appearance == "dark");
    let _ = theme_light.set_checked(initial_appearance == "light");
    let _ = size_small.set_checked(initial_widget_size == "small");
    let _ = size_medium.set_checked(initial_widget_size == "medium");
    let _ = size_large.set_checked(initial_widget_size == "large");
    let enabled_skins = app
        .try_state::<AppState>()
        .and_then(|state| {
            let preferences = state.preferences.lock().ok()?.clone();
            let request_code = device_request_code().ok()?;
            Some(supporter_status(&preferences, &request_code))
        })
        .map(|status| status.available_skins)
        .unwrap_or_else(|| vec!["default".into()]);
    let _ = supporter_blur.set_enabled(enabled_skins.iter().any(|skin| skin == BLUR_SKIN_ID));
    let _ =
        supporter_computer.set_enabled(enabled_skins.iter().any(|skin| skin == COMPUTER_SKIN_ID));
    if initial_language != "en" {
        let _ = show.set_text("显示 / 隐藏");
        let _ = refresh.set_text("立即刷新");
        let _ = update.set_text(update_menu_label(&initial_language, false));
        let _ = unlock.set_text("解锁悬浮窗");
        let _ = pin.set_text("固定 / 取消固定 Codex");
        let _ = toggle_mode.set_text("切换展开状态");
        let _ = widget_size.set_text("组件大小");
        let _ = size_small.set_text("小");
        let _ = size_medium.set_text("中");
        let _ = size_large.set_text("大");
        let _ = language.set_text("Switch to English");
        let _ = theme.set_text("主题");
        let _ = default_skin.set_text("默认皮肤");
        let _ = theme_system.set_text("跟随系统");
        let _ = theme_dark.set_text("深色");
        let _ = theme_light.set_text("浅色");
        let _ = supporter_skins.set_text("支持者皮肤");
        let _ = supporter_skins_top.set_text("赞赏开发者（皮肤）");
        let _ = autostart.set_text("开机启动");
        let _ = quit.set_text("退出");
    }
    if initial_language == "en" {
        let _ = theme.set_text("Theme");
        let _ = default_skin.set_text("Default skin");
        let _ = theme_system.set_text("Follow system");
        let _ = theme_dark.set_text("Dark");
        let _ = theme_light.set_text("Light");
        let _ = supporter_skins.set_text("Supporter skins");
        let _ = supporter_skins_top.set_text("Support developer (skins)");
        let _ = widget_size.set_text("Widget size");
        let _ = size_small.set_text("Small");
        let _ = size_medium.set_text("Medium");
        let _ = size_large.set_text("Large");
    }
    #[cfg(debug_assertions)]
    let menu = Menu::with_items(
        app,
        &[
            &show,
            &refresh,
            &update,
            &settings,
            &theme,
            &supporter_skins_top,
            &test_short_window,
            &quit,
        ],
    )?;
    #[cfg(not(debug_assertions))]
    let menu = Menu::with_items(
        app,
        &[
            &show,
            &refresh,
            &update,
            &settings,
            &theme,
            &supporter_skins_top,
            &quit,
        ],
    )?;
    let mut builder = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .tooltip("Quota Float");
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    let autostart_menu = autostart.clone();
    let show_menu = show.clone();
    let refresh_menu = refresh.clone();
    let update_menu = update.clone();
    let update_indicator = update.clone();
    let unlock_menu = unlock.clone();
    let pin_menu = pin.clone();
    let toggle_mode_menu = toggle_mode.clone();
    let widget_size_menu = widget_size.clone();
    let size_small_menu = size_small.clone();
    let size_medium_menu = size_medium.clone();
    let size_large_menu = size_large.clone();
    let language_menu = language.clone();
    let theme_menu = theme.clone();
    let default_skin_menu = default_skin.clone();
    let theme_system_menu = theme_system.clone();
    let theme_dark_menu = theme_dark.clone();
    let theme_light_menu = theme_light.clone();
    let theme_system_state = theme_system.clone();
    let theme_dark_state = theme_dark.clone();
    let theme_light_state = theme_light.clone();
    let supporter_skins_menu = supporter_skins.clone();
    let supporter_blur_menu = supporter_blur.clone();
    let supporter_computer_menu = supporter_computer.clone();
    let supporter_blur_state = supporter_blur.clone();
    let supporter_computer_state = supporter_computer.clone();
    let supporter_blur_access = supporter_blur.clone();
    let supporter_computer_access = supporter_computer.clone();
    let supporter_skins_top_menu = supporter_skins_top.clone();
    let quit_menu = quit.clone();
    #[cfg(debug_assertions)]
    let test_short_window_menu = test_short_window.clone();
    let _tray_skin_listener = app.listen("supporter-skin-changed", move |event| {
        if let Ok(skin_id) = serde_json::from_str::<String>(event.payload()) {
            let _ = supporter_blur_state.set_checked(skin_id == BLUR_SKIN_ID);
            let _ = supporter_computer_state.set_checked(skin_id == COMPUTER_SKIN_ID);
        }
    });
    let _tray_skin_access_listener = app.listen("supporter-skins-changed", move |event| {
        if let Ok(status) = serde_json::from_str::<SupporterStatus>(event.payload()) {
            let _ = supporter_blur_access.set_enabled(
                status
                    .available_skins
                    .iter()
                    .any(|skin| skin == BLUR_SKIN_ID),
            );
            let _ = supporter_computer_access.set_enabled(
                status
                    .available_skins
                    .iter()
                    .any(|skin| skin == COMPUTER_SKIN_ID),
            );
        }
    });
    builder
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("widget") {
                    if window.is_visible().unwrap_or(false) {
                        let _ = window.hide();
                    } else {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
            }
            "refresh" => {
                let _ = app.emit_to("widget", "refresh-requested", ());
            }
            "update" => {
                let _ = app.emit_to("widget", "update-check-requested", ());
            }
            "supporter-skins-top" => {
                if let Some(window) = app.get_webview_window("supporter") {
                    if let Some(state) = app.try_state::<AppState>() {
                        if let Ok(preferences) = state.preferences.lock() {
                            let english = preferences.language == "en";
                            let _ = window.set_title(if english {
                                "Quota Float · Supporter skins"
                            } else {
                                "Quota Float · 支持者皮肤"
                            });
                            let _ = app.emit_to(
                                "supporter",
                                "preferences-changed",
                                preferences.clone(),
                            );
                        }
                    }
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "supporter-skin-blur" | "supporter-skin-computer" => {
                let requested_skin = if event.id.as_ref() == "supporter-skin-blur" {
                    BLUR_SKIN_ID
                } else {
                    COMPUTER_SKIN_ID
                };
                if let Some(state) = app.try_state::<AppState>() {
                    if let Ok(request_code) = device_request_code() {
                        if let Ok(mut preferences) = state.preferences.lock() {
                            let status = supporter_status(&preferences, &request_code);
                            if status
                                .available_skins
                                .iter()
                                .any(|skin| skin == requested_skin)
                            {
                                preferences.selected_skin = requested_skin.into();
                                if persist_preferences(&state.preferences_path, &preferences)
                                    .is_ok()
                                {
                                    let saved = preferences.clone();
                                    let _ = supporter_blur_menu
                                        .set_checked(requested_skin == BLUR_SKIN_ID);
                                    let _ = supporter_computer_menu
                                        .set_checked(requested_skin == COMPUTER_SKIN_ID);
                                    let _ =
                                        app.emit_to("widget", "preferences-changed", saved.clone());
                                    let _ = app.emit_to("supporter", "preferences-changed", saved);
                                }
                            } else if let Some(window) = app.get_webview_window("supporter") {
                                let _ = app.emit_to(
                                    "supporter",
                                    "preferences-changed",
                                    preferences.clone(),
                                );
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
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
            "unlock" => {
                let _ = apply_lock(app, false);
                if let Some(state) = app.try_state::<AppState>() {
                    if let Ok(mut prefs) = state.preferences.lock() {
                        prefs.locked = false;
                        let _ = persist_preferences(&state.preferences_path, &prefs);
                        let _ = app.emit_to("widget", "preferences-changed", prefs.clone());
                    }
                }
            }
            "pin" => {
                if let Some(state) = app.try_state::<AppState>() {
                    if let Ok(mut prefs) = state.preferences.lock() {
                        prefs.pinned_provider = if prefs.pinned_provider.is_some() {
                            None
                        } else {
                            Some("codex".into())
                        };
                        let _ = persist_preferences(&state.preferences_path, &prefs);
                        let _ = app.emit_to("widget", "preferences-changed", prefs.clone());
                    }
                }
            }
            "toggle-mode" => {
                if let Some(state) = app.try_state::<AppState>() {
                    let current = state
                        .preferences
                        .lock()
                        .ok()
                        .map(|prefs| prefs.widget_mode.clone())
                        .unwrap_or_else(|| "compact".into());
                    let next = if current == "expanded" {
                        "compact"
                    } else {
                        "expanded"
                    };
                    let _ = set_widget_mode_internal(
                        mode_from_preference(next).unwrap(),
                        None,
                        app,
                        state.inner(),
                    );
                }
            }
            "widget-size-small" | "widget-size-medium" | "widget-size-large" => {
                let size = match event.id.as_ref() {
                    "widget-size-small" => WidgetSize::Small,
                    "widget-size-large" => WidgetSize::Large,
                    _ => WidgetSize::Medium,
                };
                if let Some(state) = app.try_state::<AppState>() {
                    if set_widget_size_internal(size, None, app, state.inner()).is_ok() {
                        let _ = size_small_menu.set_checked(matches!(size, WidgetSize::Small));
                        let _ = size_medium_menu.set_checked(matches!(size, WidgetSize::Medium));
                        let _ = size_large_menu.set_checked(matches!(size, WidgetSize::Large));
                    }
                }
            }
            "language" => {
                if let Some(state) = app.try_state::<AppState>() {
                    if let Ok(mut prefs) = state.preferences.lock() {
                        prefs.language = if prefs.language == "en" {
                            "zh-CN".into()
                        } else {
                            "en".into()
                        };
                        let normalized = prefs.clone().normalized();
                        *prefs = normalized.clone();
                        let _ = persist_preferences(&state.preferences_path, &normalized);
                        let english = normalized.language == "en";
                        let _ = show_menu.set_text(if english {
                            "Show / Hide"
                        } else {
                            "显示 / 隐藏"
                        });
                        let _ = refresh_menu.set_text(if english {
                            "Refresh now"
                        } else {
                            "立即刷新"
                        });
                        let update_available = state
                            .update_available
                            .lock()
                            .map(|value| *value)
                            .unwrap_or(false);
                        let _ = update_menu
                            .set_text(update_menu_label(&normalized.language, update_available));
                        let _ = unlock_menu.set_text(if english {
                            "Unlock widget"
                        } else {
                            "解锁悬浮窗"
                        });
                        let _ = pin_menu.set_text(if english {
                            "Pin / Unpin Codex"
                        } else {
                            "固定 / 取消固定 Codex"
                        });
                        let _ = toggle_mode_menu.set_text(if english {
                            "Toggle widget mode"
                        } else {
                            "切换展开状态"
                        });
                        let _ = widget_size_menu.set_text(if english {
                            "Widget size"
                        } else {
                            "组件大小"
                        });
                        let _ = size_small_menu.set_text(if english { "Small" } else { "小" });
                        let _ = size_medium_menu.set_text(if english { "Medium" } else { "中" });
                        let _ = size_large_menu.set_text(if english { "Large" } else { "大" });
                        let _ = language_menu.set_text(if english {
                            "切换到中文"
                        } else {
                            "Switch to English"
                        });
                        let _ = theme_menu.set_text(if english { "Theme" } else { "主题" });
                        let _ = default_skin_menu.set_text(if english {
                            "Default skin"
                        } else {
                            "默认皮肤"
                        });
                        let _ = theme_system_menu.set_text(if english {
                            "Follow system"
                        } else {
                            "跟随系统"
                        });
                        let _ = theme_dark_menu.set_text(if english { "Dark" } else { "深色" });
                        let _ = theme_light_menu.set_text(if english { "Light" } else { "浅色" });
                        let _ = supporter_skins_menu.set_text(if english {
                            "Supporter skins"
                        } else {
                            "支持者皮肤"
                        });
                        let _ = supporter_skins_top_menu.set_text(if english {
                            "Support developer (skins)"
                        } else {
                            "赞赏开发者（皮肤）"
                        });
                        let _ = autostart_menu.set_text(if english {
                            "Start at login"
                        } else {
                            "开机启动"
                        });
                        let _ = quit_menu.set_text(if english { "Quit" } else { "退出" });
                        let _ = app.emit_to("widget", "preferences-changed", normalized.clone());
                        let _ = app.emit_to("supporter", "preferences-changed", normalized);
                        if let Some(window) = app.get_webview_window("supporter") {
                            let _ = window.set_title(if english {
                                "Quota Float · Supporter skins"
                            } else {
                                "Quota Float · 支持者皮肤"
                            });
                        }
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
                        // The three free appearance choices always render the
                        // default skin. A supporter skin is selected only by
                        // its own menu item, never as an extra prerequisite.
                        prefs.selected_skin = "default".into();
                        let normalized = prefs.clone().normalized();
                        *prefs = normalized.clone();
                        if persist_preferences(&state.preferences_path, &normalized).is_ok() {
                            let _ = supporter_blur_menu.set_checked(false);
                            let _ = supporter_computer_menu.set_checked(false);
                            let _ =
                                theme_system_state.set_checked(normalized.appearance == "system");
                            let _ = theme_dark_state.set_checked(normalized.appearance == "dark");
                            let _ = theme_light_state.set_checked(normalized.appearance == "light");
                            let _ =
                                app.emit_to("widget", "preferences-changed", normalized.clone());
                            let _ =
                                app.emit_to("supporter", "preferences-changed", normalized.clone());
                            let _ = app.emit("supporter-skin-changed", normalized.selected_skin);
                        }
                    }
                }
            }
            "autostart" => {
                let manager = app.autolaunch();
                let enabled = manager.is_enabled().unwrap_or(false);
                let result = if enabled {
                    manager.disable()
                } else {
                    manager.enable()
                };
                match result {
                    Ok(()) => {
                        let _ = autostart_menu.set_checked(!enabled);
                    }
                    Err(_) => eprintln!("autostart update failed"),
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    // Do this after creating the tray and off the UI thread. A failed or slow
    // network check leaves the ordinary menu item untouched.
    let update_app = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        let Ok(updater) = update_app.updater() else {
            return;
        };
        if updater.check().await.ok().flatten().is_none() {
            return;
        }
        let language = update_app
            .try_state::<AppState>()
            .and_then(|state| {
                if let Ok(mut available) = state.update_available.lock() {
                    *available = true;
                }
                state
                    .preferences
                    .lock()
                    .ok()
                    .map(|prefs| prefs.language.clone())
            })
            .unwrap_or_else(|| "zh-CN".into());
        let _ = update_indicator.set_text(update_menu_label(&language, true));
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
            let mut preferences = load_preferences(&preferences_path);
            let has_supporter_license = match device_request_code() {
                Ok(request_code) => {
                    reconcile_verified_supporter_fields(&mut preferences, &request_code);
                    supporter_status(&preferences, &request_code).active
                }
                Err(_) => {
                    reconcile_supporter_fields(&mut preferences, Vec::new());
                    false
                }
            };
            let show_supporter_prompt =
                should_show_supporter_prompt(&mut preferences, Utc::now(), has_supporter_license);
            // Persist the first-use timestamp immediately; persist the shown
            // marker before opening the window so a crash or restart cannot
            // produce repeated prompts.
            if preferences.supporter_prompt_first_seen_at.is_some() {
                let _ = persist_preferences(&preferences_path, &preferences);
            }
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
                update_available: Mutex::new(false),
            });
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
                let _ = window.set_always_on_top(preferences.always_on_top);
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
            if show_supporter_prompt {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(900));
                    if let Some(window) = handle.get_webview_window("supporter") {
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
            set_widget_size,
            begin_widget_resize,
            preview_widget_resize,
            finish_widget_resize,
            cancel_widget_resize,
            begin_widget_drag,
            finish_widget_drag,
            get_preferences,
            set_preferences,
            set_widget_locked,
            set_widget_always_on_top,
            sync_widget_appearance,
            get_supporter_status,
            activate_supporter_license,
            select_supporter_skin
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
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("failed to build Quota Float");
    app.run(|app_handle, event| {
        if matches!(event, tauri::RunEvent::Resumed) {
            let _ = app_handle.emit_to("widget", "refresh-requested", ());
        }
    });
}
