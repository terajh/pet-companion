use base64::Engine;
use notify::{Event as NotifyEvent, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{TrayIconBuilder, TrayIconEvent},
    ActivationPolicy, AppHandle, Emitter, LogicalPosition, Manager, State, WindowEvent,
};
use tauri_plugin_autostart::MacosLauncher;
use time::format_description::well_known::Rfc3339;
use tokio::sync::{mpsc, Mutex};

const APP_EVENT: &str = "companion:update";
/// Lightweight pet-scale-only event emitted by `cmd_set_pet_scale` before the
/// model lock is touched.  Frontend updates its local payload optimistically
/// so the slider feels instantaneous regardless of refresh-tick contention.
const PET_SCALE_EVENT: &str = "companion:pet_scale";
/// Same pattern for pet override.  Payload is just the new override id (or
/// null for auto-follow).  Frontend resolves the descriptor from its local
/// `payload.pets` list so the overlay sprite swaps before the spawn's
/// `refresh_and_emit` has a chance to acquire the model lock.
const PET_OVERRIDE_EVENT: &str = "companion:pet_override";
/// Drag facing event.  Emitted by `run_drag_cursor_loop` whenever the drag
/// direction crosses the hysteresis threshold (and once at drag start /
/// end).  Payload is `{ dragging: bool, facingLeft: bool }`.  Frontend
/// switches the pet sprite to running animation and mirrors it horizontally
/// when `facingLeft = true`.
const FACING_EVENT: &str = "companion:facing";
const CONFIG_FILE_NAME: &str = "config.json";
const OVERLAY_WINDOW_LABEL: &str = "overlay";
const SETTINGS_WINDOW_LABEL: &str = "settings";
const TRAY_ID: &str = "companion-tray";
const FALLBACK_PET_ID: &str = "bori";
const OVERLAY_WIDTH: i32 = 520;
const OVERLAY_HEIGHT: i32 = 760;
const ATTACHED_MARGIN_X: i32 = 24;
const ATTACHED_MARGIN_Y: i32 = 24;
// macOS menu bar is roughly 24-28 pt; add a small top guard so the overlay
// never creeps under the menu bar.
const MACOS_MENU_BAR_HEIGHT: i32 = 28;
// Pet sprite position within the overlay window (mirrors `.pet-shell` CSS at
// scale=1: right:28px, bottom:16px, 112×124).  Used by `clamp_overlay_position`
// to keep the *pet*, not the whole 520×760 window, inside the monitor frame so
// the user can drag the pet up to the menu bar even though the upper portion of
// the window is empty.
const PET_BOX_RIGHT: i32 = 28;
const PET_BOX_BOTTOM: i32 = 16;
const PET_BOX_WIDTH: i32 = 112;
const PET_BOX_HEIGHT: i32 = 124;
const PET_VISIBLE_PADDING: i32 = 24;
const RUNNING_THRESHOLD_MS: u64 = 3_000;
const WAITING_THRESHOLD_MS: u64 = 30_000;
const WAVING_DURATION_MS: u64 = 2_000;
const JUMPING_DURATION_MS: u64 = 800;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
enum SessionApp {
    Claude,
    Codex,
}

impl Default for SessionApp {
    fn default() -> Self {
        Self::Claude
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum PetAnimationState {
    Idle,
    Sleeping,
    Running,
    Waiting,
    Waving,
    Jumping,
    Review,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionSummary {
    app_kind: SessionApp,
    #[serde(skip_serializing)]
    cli_session_id: Option<String>,
    completed_preview: Option<String>,
    completed_turns: Option<u64>,
    cwd: String,
    in_progress: bool,
    is_archived: bool,
    last_activity_at: u64,
    session_id: String,
    title: String,
    user_preview: Option<String>,
    assistant_preview: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PetDescriptor {
    description: String,
    display_name: String,
    id: String,
    source: String,
    sprite_sheet_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OverlaySnapshot {
    active_session: Option<SessionSummary>,
    claude_frontmost: bool,
    codex_frontmost: bool,
    current_window_title: Option<String>,
    effective_state: PetAnimationState,
    message_preview: Option<String>,
    permission_granted: bool,
    pet: PetDescriptor,
    sessions: Vec<SessionSummary>,
    show_card: bool,
    state_label: String,
    dismissed_session_ids: Vec<String>,
    /// Session ids that transitioned from `in_progress=true` to `false`
    /// during the current Pet Companion process lifetime.  Used by the
    /// frontend to display a "완료" card that persists until clicked.
    completed_runtime_session_ids: Vec<String>,
    /// When true, the card stack should render *below* the pet because the
    /// pet sits too close to the top of the monitor for cards to fit above.
    cards_below: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FrontendConfig {
    attached: bool,
    language: String,
    pet_override_id: Option<String>,
    pet_scale: f32,
    tracked_app: String,
    watch_claude: bool,
    watch_codex: bool,
    pet_hidden: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppPayload {
    codex_selected_pet_id: Option<String>,
    config: FrontendConfig,
    overlay: OverlaySnapshot,
    pets: Vec<PetDescriptor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedConfig {
    detached: bool,
    detached_position: Option<SavedPosition>,
    last_custom_pet_id: Option<String>,
    #[serde(default = "default_language")]
    language: String,
    manual_session_app: Option<SessionApp>,
    manual_session_id: Option<String>,
    pet_override_id: Option<String>,
    #[serde(default = "default_tracked_app")]
    tracked_app: String,
    #[serde(default = "default_pet_scale")]
    pet_scale: f32,
    // Per-app watch toggles surface in the settings window as two
    // checkboxes; defaulting to true matches the pre-toggle behavior
    // so existing users see no change after upgrading.
    #[serde(default = "default_watch_app")]
    watch_claude: bool,
    #[serde(default = "default_watch_app")]
    watch_codex: bool,
    // Persists the user's "hide pet" choice across app restarts.  When
    // true the overlay window is hidden on launch; toggled via the pet
    // right-click menu, tray menu, or tray icon click.
    #[serde(default)]
    pet_hidden: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SavedPosition {
    x: i32,
    y: i32,
}


#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PetManifest {
    description: String,
    display_name: String,
    id: String,
    spritesheet_path: String,
}

#[derive(Debug, Clone, Default)]
struct ClaudeWindowInfo {
    frontmost: bool,
    permission_granted: bool,
    title: Option<String>,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

#[derive(Debug, Clone, Default)]
struct FrontWindowState {
    claude: ClaudeWindowInfo,
    codex: ClaudeWindowInfo,
    frontmost_app: Option<SessionApp>,
    claude_running: bool,
    codex_running: bool,
}

#[derive(Debug, Clone)]
struct AnimationLatch {
    state: PetAnimationState,
    until: Instant,
}

#[derive(Debug, Default)]
struct RuntimeModel {
    config: PersistedConfig,
    current_payload: Option<AppPayload>,
    // session_ids that have been "dismissed" by the user clicking on a
    // Completed/Idle card.  The card is hidden until the session re-enters
    // an in-progress state (Running / Waiting).
    dismissed_sessions: HashSet<String>,
    /// session_ids that flipped from `in_progress=true` to `false` during
    /// the current process lifetime.  Cleared per-id when the user clicks
    /// the corresponding "완료" card, or when the session re-enters
    /// `in_progress`.
    completed_during_runtime: HashSet<String>,
    /// Last observed `in_progress` value per session_id.  Used to detect
    /// the true→false transition that promotes a session into
    /// `completed_during_runtime`.
    prev_in_progress: HashMap<String, bool>,
    /// Last notification-target state (`waiting` / `waving`) recorded per
    /// session.  Used to suppress repeated notifications while a session
    /// remains in the same attention state, and cleared when the session
    /// returns to `running`.
    last_notified_state: HashMap<String, String>,
    last_base_state: Option<PetAnimationState>,
    last_completed_turns: HashMap<String, u64>,
    last_focused_app: Option<SessionApp>,
    transcript_paths: HashMap<String, PathBuf>,
    onboarding_shown: bool,
    override_animation: Option<AnimationLatch>,
    /// Last logical position written by sync_overlay_window / move_overlay_to_safe_home.
    /// Used by the Moved handler to distinguish programmatic repositioning from user drag.
    expected_attached_position: Option<(i32, i32)>,
}

struct AppState {
    config_path: PathBuf,
    model: Arc<Mutex<RuntimeModel>>,
    /// Cancel signal for the cursor-polling drag loop spawned by `cmd_begin_drag`.
    /// The loop reads this every 16 ms; `cmd_finalize_drag_position` flips it to
    /// `true` to stop the loop.  Kept in a non-async `StdMutex` (not inside
    /// `RuntimeModel`) so begin/finalize never have to wait on the heavy 750 ms
    /// `refresh_and_emit` tick that holds the model lock — without this split,
    /// begin_drag could be queued behind refresh_and_emit for >1 s, by which
    /// time the user has already released the mouse and finalize cancels the
    /// loop the instant it starts (root cause of the v0.1.26 "doesn't move at all"
    /// bug).
    drag_cancel: StdMutex<Option<Arc<AtomicBool>>>,
    /// Holds the live `notify` filesystem watcher.  Created in `setup()` and
    /// kept alive for the lifetime of the app — dropping the watcher stops
    /// FSEvents notifications.  Stored in an `Option` because the watcher
    /// may fail to initialise (e.g. on platforms without FSEvents support);
    /// in that case the 750 ms polling tick still drives `refresh_and_emit`.
    _fs_watcher: StdMutex<Option<RecommendedWatcher>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionNotification {
    session_id: String,
    state: PetAnimationState,
    subtitle: String,
    body: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PetOverrideInput {
    pet_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LanguageInput {
    language: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrackedAppInput {
    app: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PetScaleInput {
    scale: f32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WatchAppsInput {
    watch_claude: bool,
    watch_codex: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PetHiddenInput {
    hidden: bool,
}

/// Payload for `FACING_EVENT`. `dragging=false` means the pet is no longer
/// being dragged (frontend should drop the running override and reset
/// horizontal flip). `facing_left=true` mirrors the sprite via `scaleX(-1)`.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
struct FacingPayload {
    dragging: bool,
    facing_left: bool,
}


#[derive(Debug, Clone)]
struct PetResolution {
    codex_selected_pet_id: Option<String>,
    effective_pet: PetDescriptor,
}

#[derive(Debug, Clone, Default)]
struct CodexGlobalState {
    active_workspace_roots: Vec<String>,
    selected_custom_pet_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BaseState {
    Idle,
    Running,
    Waiting,
    Completed,
}

impl Default for PersistedConfig {
    fn default() -> Self {
        Self {
            detached: false,
            detached_position: None,
            last_custom_pet_id: None,
            language: default_language(),
            manual_session_app: None,
            manual_session_id: None,
            pet_override_id: None,
            pet_scale: default_pet_scale(),
            tracked_app: default_tracked_app(),
            watch_claude: default_watch_app(),
            watch_codex: default_watch_app(),
            pet_hidden: false,
        }
    }
}

#[tauri::command]
async fn cmd_get_app_payload(state: State<'_, AppState>) -> Result<AppPayload, String> {
    // The first refresh_and_emit may not have completed yet when the WebView
    // calls this on mount.  Poll for up to 3 s (60 × 50 ms) before giving up.
    for _attempt in 0..60u32 {
        {
            let model = state.model.lock().await;
            if let Some(payload) = model.current_payload.clone() {
                return Ok(payload);
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err("Companion state is not ready yet.".to_string())
}

#[tauri::command]
async fn cmd_show_settings(app: AppHandle) -> Result<(), String> {
    let Some(window) = app.get_webview_window(SETTINGS_WINDOW_LABEL) else {
        return Err("Settings window is unavailable.".to_string());
    };
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn cmd_set_tracked_app(
    app: AppHandle,
    state: State<'_, AppState>,
    input: TrackedAppInput,
) -> Result<(), String> {
    {
        let mut model = state.model.lock().await;
        model.config.tracked_app = normalize_tracked_app(&input.app).to_string();
        persist_config(&state.config_path, &model.config)?;
    }
    refresh_and_emit(&app, &state).await?;
    Ok(())
}

#[tauri::command]
async fn cmd_set_pet_override(
    app: AppHandle,
    _state: State<'_, AppState>,
    input: PetOverrideInput,
) -> Result<(), String> {
    // v0.1.33: even v0.1.32's spawn pattern wasn't enough — the user still
    // perceived "펫 선택이 아직 여전히 바로 반응을 안하고 있어".  Root
    // cause: the spawned task still awaits `state.model.lock()` and can sit
    // behind the 750ms refresh tick for hundreds of ms before
    // `refresh_and_emit` (the heavy `rebuild_payload` + emit) fires.  Until
    // that emit lands the overlay keeps showing the old sprite.
    //
    // Fix: emit `companion:pet_override` synchronously BEFORE any lock or
    // spawn.  The frontend already holds the full `payload.pets[]` list, so
    // it can resolve the new PetDescriptor locally and patch the overlay
    // instantly.  The heavy work (lock + persist + refresh_and_emit) still
    // runs in a background spawn so the IPC returns immediately and the
    // backend eventually converges with the optimistic frontend state.
    let pet_id = input.pet_id;
    let _ = app.emit(PET_OVERRIDE_EVENT, pet_id.clone());

    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app_clone.state::<AppState>();
        {
            let mut model = state.model.lock().await;
            model.config.pet_override_id = pet_id;
            if let Err(err) = persist_config(&state.config_path, &model.config) {
                eprintln!("[pet_override] persist_config failed: {}", err);
                return;
            }
        }
        if let Err(err) = refresh_and_emit(&app_clone, &state).await {
            eprintln!("[pet_override] refresh_and_emit failed: {}", err);
        }
    });
    Ok(())
}

#[tauri::command]
async fn cmd_set_language(
    app: AppHandle,
    state: State<'_, AppState>,
    input: LanguageInput,
) -> Result<(), String> {
    let language = normalize_language(&input.language);
    {
        let mut model = state.model.lock().await;
        model.config.language = language.to_string();
        persist_config(&state.config_path, &model.config)?;
    }
    refresh_and_emit(&app, &state).await?;
    Ok(())
}

#[tauri::command]
async fn cmd_set_pet_scale(
    app: AppHandle,
    _state: State<'_, AppState>,
    input: PetScaleInput,
) -> Result<(), String> {
    let clamped = input.scale.clamp(0.5, 2.0);
    // v0.1.32: even with v0.1.31's spawn pattern users still saw "캐릭터
    // 크기도 바로바로 반영이 안되고 좀 시간이 지나고 반영되고 있어".  Root
    // cause: the spawn still awaits `state.model.lock()` and can sit behind
    // the 750ms refresh tick for hundreds of ms before the emit fires.
    //
    // Fix: emit a LIGHTWEIGHT `companion:pet_scale` event SYNCHRONOUSLY
    // before any lock is touched.  The overlay listens for it and applies
    // the scale to its local payload state immediately (no Rust round-trip
    // on the visual response path).  The model lock + persist + payload
    // update then run in a background spawn for state durability.
    let _ = app.emit(PET_SCALE_EVENT, clamped);

    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app_clone.state::<AppState>();
        let mut model = state.model.lock().await;
        if (model.config.pet_scale - clamped).abs() < f32::EPSILON {
            return;
        }
        model.config.pet_scale = clamped;
        if let Err(err) = persist_config(&state.config_path, &model.config) {
            eprintln!("[pet_scale] persist_config failed: {}", err);
        }
        if let Some(payload) = model.current_payload.as_mut() {
            payload.config.pet_scale = clamped;
        }
    });
    Ok(())
}

#[tauri::command]
async fn cmd_set_watch_apps(
    app: AppHandle,
    _state: State<'_, AppState>,
    input: WatchAppsInput,
) -> Result<(), String> {
    // Same v0.1.27 model-lock invariant as cmd_set_pet_scale: avoid awaiting
    // `state.model.lock()` on the IPC hot path because the 750ms refresh tick
    // may be holding it through a full session scan.  Spawn the lock + persist
    // + refresh_and_emit work to a background task so the IPC returns
    // immediately and the checkbox feels responsive.  The local-mirror state
    // in `SettingsApp` keeps the UI in sync until the backend echo arrives.
    let app_clone = app.clone();
    let watch_claude = input.watch_claude;
    let watch_codex = input.watch_codex;
    tauri::async_runtime::spawn(async move {
        let state = app_clone.state::<AppState>();
        {
            let mut model = state.model.lock().await;
            model.config.watch_claude = watch_claude;
            model.config.watch_codex = watch_codex;
            if let Err(err) = persist_config(&state.config_path, &model.config) {
                eprintln!("[watch_apps] persist_config failed: {}", err);
                return;
            }
        }
        if let Err(err) = refresh_and_emit(&app_clone, &state).await {
            eprintln!("[watch_apps] refresh_and_emit failed: {}", err);
        }
    });
    Ok(())
}

/// Absolute setter for the overlay hide state.  Used by every entry point
/// that toggles pet visibility (IPC from the right-click menu, tray menu
/// item, tray icon click).  Two invariants:
///
///   • Visual flip runs **before** the model lock is acquired so the user
///     sees the window change immediately.  The 750 ms refresh tick can
///     hold the lock for hundreds of ms; deferring the hide/show until the
///     spawn finishes would feel laggy (v0.1.32 sidechannel rule).
///   • Persistence runs on a background spawn so the entry-point function
///     (IPC handler / menu closure / tray closure) returns immediately and
///     never blocks on `model.lock().await` (v0.1.27 model-lock rule).
///
/// Both tray entry points compute `hidden` from the **current window
/// visibility** synchronously, so even if two toggles are queued back to
/// back, each one targets an absolute state instead of toggling whatever
/// the other spawn happened to write.  This eliminates the race where two
/// "toggle" callbacks could resolve in either order and leave the window
/// in a state opposite to the user's last click.
fn apply_pet_hidden(app: &AppHandle, hidden: bool) {
    if let Some(window) = app.get_webview_window(OVERLAY_WINDOW_LABEL) {
        let _ = if hidden { window.hide() } else { window.show() };
    }
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app_clone.state::<AppState>();
        {
            let mut model = state.model.lock().await;
            model.config.pet_hidden = hidden;
            if let Err(err) = persist_config(&state.config_path, &model.config) {
                eprintln!("[pet_hidden] persist_config failed: {}", err);
                return;
            }
        }
        if let Err(err) = refresh_and_emit(&app_clone, &state).await {
            eprintln!("[pet_hidden] refresh_and_emit failed: {}", err);
        }
    });
}

#[tauri::command]
async fn cmd_set_pet_hidden(
    app: AppHandle,
    _state: State<'_, AppState>,
    input: PetHiddenInput,
) -> Result<(), String> {
    apply_pet_hidden(&app, input.hidden);
    Ok(())
}

#[tauri::command]
async fn cmd_read_pet_sprite_data_url(path: String) -> Result<String, String> {
    let path = PathBuf::from(&path);
    eprintln!("[PetSprite] resolving path: {:?}", path);
    if !path.exists() {
        eprintln!("[PetSprite] FATAL: path does not exist: {:?}", path);
        return Err(format!("sprite path does not exist: {:?}", path));
    }
    let bytes = fs::read(&path).map_err(|e| {
        eprintln!("[PetSprite] FATAL: failed to read {:?}: {}", path, e);
        e.to_string()
    })?;
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());
    let mime = match ext.as_deref() {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => "application/octet-stream",
    };
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let data_url = format!("data:{mime};base64,{encoded}");
    eprintln!(
        "[PetSprite] data URL generated: mime={}, base64_len={}, prefix={}",
        mime,
        encoded.len(),
        &data_url[..data_url.len().min(60)]
    );
    Ok(data_url)
}

#[tauri::command]
async fn cmd_pet_reaction(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    {
        let mut model = state.model.lock().await;
        model.override_animation = Some(AnimationLatch {
            state: PetAnimationState::Jumping,
            until: Instant::now() + Duration::from_millis(JUMPING_DURATION_MS),
        });
    }
    refresh_and_emit(&app, &state).await?;
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BeginDragInput {
    /// Cursor offset within the overlay window at pointerdown, in logical CSS
    /// pixels (i.e. `event.clientX` / `event.clientY`).  Stays constant for the
    /// duration of the drag so the pet stays anchored under the cursor.
    grab_offset_x: f64,
    grab_offset_y: f64,
    /// Initial cursor direction known to the frontend at the moment the drag
    /// crossed `DRAG_THRESHOLD`.  `true` = cursor moved left of pointerdown
    /// origin (negative dx in screen coords).  Used to seed the initial
    /// `companion:facing` emit and the loop's `last_facing_left` so the pet
    /// renders the correct horizontal flip immediately — avoiding the ~35 ms
    /// "weird motion" flash where the pet briefly ran right while the user
    /// dragged left (v0.1.37 user report).
    initial_facing_left: bool,
}

/// Starts a Rust-driven cursor-polling drag loop.  Frontend calls this exactly
/// once when a drag is detected (pointer moved past `DRAG_THRESHOLD`); from
/// then on Rust polls `window.cursor_position()` every 16 ms and writes the
/// overlay position, completely bypassing WKWebView's per-monitor `event.screenY`
/// origin flip (root cause of the v0.1.25 oscillation across the upper-monitor
/// boundary).  The frontend no longer sends per-frame IPC at all during drag.
///
/// Flips `detached = true` synchronously so the 750 ms `sync_overlay_window`
/// tick stops re-anchoring to the tracked Claude/Codex window.  Any
/// previously-running drag loop is cancelled before the new one starts.
#[tauri::command]
async fn cmd_begin_drag(
    app: AppHandle,
    state: State<'_, AppState>,
    input: BeginDragInput,
) -> Result<(), String> {
    // ── Hot path: spawn the cursor-polling loop IMMEDIATELY.
    //
    // We must NOT await the heavy `state.model.lock()` (async RuntimeModel
    // mutex) before spawning, because the 750 ms `sync_overlay_window` tick
    // can hold that lock for ≥1 s mid-`refresh_and_emit`.  In v0.1.26 that
    // caused begin_drag to stall long enough for the user to release the
    // mouse; finalize then queued up behind begin_drag and cancelled the
    // loop one tick after it started — pet never moved.
    //
    // Fix: store the cancel handle in `AppState.drag_cancel` (a `std::sync::
    // Mutex`), not in `RuntimeModel`.  Swap is instant — no `.await`.
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut slot = state
            .drag_cancel
            .lock()
            .map_err(|e| format!("drag_cancel poisoned: {e}"))?;
        if let Some(prev) = slot.take() {
            prev.store(true, Ordering::Relaxed);
        }
        *slot = Some(cancel.clone());
    }

    // pet_scale is captured once at drag start to keep the hot loop free of
    // model-lock contention.  `try_lock` avoids stalling here too — if a
    // refresh tick happens to hold the lock right now we fall back to 1.0
    // (matches the previous default for the first tick).
    let pet_scale = match state.model.try_lock() {
        Ok(model) => model.config.pet_scale,
        Err(_) => 1.0,
    };

    // primary_scale is captured once and used for ALL coordinate conversions
    // during this drag session (v0.1.30).  Rationale: tao's
    // `window.cursor_position()` always projects through `primary_monitor()
    // .scale_factor()` (see tao/src/platform_impl/macos/util/mod.rs —
    // `CGDisplay::main().pixels_high()` + `primary_monitor().scale_factor()`).
    // In contrast `window.scale_factor()` reads `NSWindow.backingScaleFactor`,
    // which FLIPS between 2.0 and 1.0 when the window crosses a monitor
    // boundary in a mixed-DPI setup (Retina primary + non-Retina externals).
    // Mixing the two scales causes the cursor's logical coords to halve/double
    // each time the window crosses the boundary, which manifests as a
    // two-position oscillation when the user drags the pet toward the upper
    // external monitor (the v0.1.29 user report).
    let primary_scale = app
        .get_webview_window(OVERLAY_WINDOW_LABEL)
        .and_then(|w| w.primary_monitor().ok().flatten())
        .map(|m| m.scale_factor())
        .unwrap_or(1.0);

    // Emit initial facing state immediately.  Frontend has already moved the
    // cursor past DRAG_THRESHOLD (≥ 6 logical px) by the time it calls this,
    // so it KNOWS the initial direction — passing it as `initial_facing_left`
    // lets us emit the correct flip on the very first frame.  Hard-coding
    // `facing_left: false` here (the pre-v0.1.37 behaviour) caused a ~35 ms
    // window where the pet ran right while the user dragged left before the
    // loop's first tick caught up and emitted the correct direction.
    let initial_facing_left = input.initial_facing_left;
    let _ = app.emit(
        FACING_EVENT,
        FacingPayload {
            dragging: true,
            facing_left: initial_facing_left,
        },
    );

    let app_for_loop = app.clone();
    let grab_x = input.grab_offset_x;
    let grab_y = input.grab_offset_y;
    let cancel_for_loop = cancel.clone();
    tauri::async_runtime::spawn(async move {
        run_drag_cursor_loop(
            app_for_loop,
            grab_x,
            grab_y,
            pet_scale,
            primary_scale,
            initial_facing_left,
            cancel_for_loop,
        )
        .await;
    });

    // ── Cold path: persist `detached=true` and trigger a refresh in the
    // background.  These can wait for the model lock without blocking the
    // drag loop, because the loop is already running and reading the cursor.
    let app_for_persist = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app_for_persist.state::<AppState>();
        {
            let mut model = state.model.lock().await;
            if !model.config.detached {
                model.config.detached = true;
                if let Err(e) = persist_config(&state.config_path, &model.config) {
                    eprintln!("[drag] background persist_config FAILED: {e}");
                }
            }
        }
        if let Err(e) = refresh_and_emit(&app_for_persist, &state).await {
            eprintln!("[drag] background refresh_and_emit FAILED: {e}");
        }
    });

    Ok(())
}

/// Cursor-polling loop body. Reads `window.cursor_position()` (single
/// global coordinate system, immune to WKWebView's per-monitor screen-coord
/// origin flips) and writes the overlay's new logical position every 16 ms
/// until `cancel` is flipped to `true` by `cmd_finalize_drag_position`.
///
/// `pet_scale` is captured once at drag start to keep this hot loop
/// free of model-lock contention with other async tasks (e.g. refresh ticks,
/// session focus calls).
///
/// `primary_scale` is captured once at drag start (see `cmd_begin_drag` for
/// the rationale).  It must NOT be replaced with `window.scale_factor()`,
/// which flips between monitors in mixed-DPI setups and reintroduces the
/// v0.1.29 oscillation bug.
async fn run_drag_cursor_loop(
    app: AppHandle,
    grab_offset_x: f64,
    grab_offset_y: f64,
    pet_scale: f32,
    primary_scale: f64,
    initial_facing_left: bool,
    cancel: Arc<AtomicBool>,
) {
    let mut interval = tokio::time::interval(Duration::from_millis(16));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut tick_count: u64 = 0;
    let mut cursor_err_count: u64 = 0;
    let mut set_pos_err_count: u64 = 0;
    let exit_reason: &str;
    // Facing tracking: emit `companion:facing` only when the cursor direction
    // actually flips (with a 4-logical-px hysteresis on the running anchor) so
    // we never spam the event bus at 60Hz. `last_cursor_x` is the X coord we
    // compared against last; `last_facing_left` is the most-recently-emitted
    // facing. We seed `last_facing_left = initial_facing_left` so the loop's
    // diff matches whatever `cmd_begin_drag` already emitted — otherwise the
    // first tick that crosses hysteresis would re-emit the same value.
    let mut last_cursor_x: Option<f64> = None;
    let mut last_facing_left: bool = initial_facing_left;
    const FACING_HYSTERESIS_PX: f64 = 4.0;
    loop {
        interval.tick().await;
        tick_count += 1;
        if cancel.load(Ordering::Relaxed) {
            exit_reason = "cancel";
            break;
        }
        let Some(window) = app.get_webview_window(OVERLAY_WINDOW_LABEL) else {
            exit_reason = "window-gone";
            break;
        };
        let cursor = match window.cursor_position() {
            Ok(p) => p,
            Err(e) => {
                cursor_err_count += 1;
                if cursor_err_count <= 3 || cursor_err_count % 60 == 0 {
                    eprintln!(
                        "[drag-loop] tick#{tick_count} cursor_position ERR ({cursor_err_count}): {e}"
                    );
                }
                continue;
            }
        };
        // `cursor_position()` returns PhysicalPosition<f64> already multiplied
        // by tao's primary_monitor scale factor (see tao's
        // util/mod.rs::cursor_position).  Dividing by THE SAME primary scale
        // undoes that projection exactly — yielding logical coords on the
        // primary monitor's coordinate system, which is what
        // `set_position(LogicalPosition)` consumes.
        let logical_cursor_x = cursor.x / primary_scale;
        let logical_cursor_y = cursor.y / primary_scale;

        // Facing detection runs against the logical cursor X (already in the
        // primary-monitor coord space, immune to mixed-DPI monitor flips for
        // the same reason as the position math above).
        match last_cursor_x {
            None => {
                last_cursor_x = Some(logical_cursor_x);
            }
            Some(anchor) => {
                let dx = logical_cursor_x - anchor;
                if dx.abs() >= FACING_HYSTERESIS_PX {
                    let next_facing_left = dx < 0.0;
                    if next_facing_left != last_facing_left {
                        last_facing_left = next_facing_left;
                        let _ = app.emit(
                            FACING_EVENT,
                            FacingPayload {
                                dragging: true,
                                facing_left: next_facing_left,
                            },
                        );
                    }
                    // Re-anchor whenever we've moved past the hysteresis so
                    // small jitter near the new anchor doesn't flip again
                    // immediately.  Without this the user dragging gently
                    // would oscillate at every threshold cross.
                    last_cursor_x = Some(logical_cursor_x);
                }
            }
        }

        let target_x = (logical_cursor_x - grab_offset_x).round() as i32;
        let target_y = (logical_cursor_y - grab_offset_y).round() as i32;
        let (cx, cy) = clamp_overlay_position(&window, pet_scale, target_x, target_y);
        if let Err(e) = window.set_position(LogicalPosition::new(cx, cy)) {
            set_pos_err_count += 1;
            if set_pos_err_count <= 3 || set_pos_err_count % 60 == 0 {
                eprintln!(
                    "[drag-loop] tick#{tick_count} set_position ERR ({set_pos_err_count}): {e}"
                );
            }
        }
    }
    // One-shot exit line preserved — useful for diagnosing the "doesn't move"
    // class of bug (e.g. reason=cancel with ticks=1 means the loop was
    // cancelled the instant it spawned, as in the v0.1.26 lock-contention
    // regression).
    if cursor_err_count > 0 || set_pos_err_count > 0 {
        eprintln!(
            "[drag-loop] EXIT reason={exit_reason} ticks={tick_count} cursor_errs={cursor_err_count} set_errs={set_pos_err_count}"
        );
    } else {
        eprintln!("[drag-loop] EXIT reason={exit_reason} ticks={tick_count}");
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FocusSessionInput {
    session_id: String,
    app_kind: SessionApp,
    /// Whether to register this session in `dismissed_sessions` so the card
    /// hides until the session re-enters `in_progress = true`.  The frontend
    /// decides this from the card's *visual* state (running/waiting → false;
    /// waving/idle/jumping → true), NOT from the backend's `in_progress`
    /// flag.  Decoupling matters because Claude's `in_progress` is computed
    /// as `latest_user_at > latest_assistant_at` which can stay true after a
    /// turn finishes if the .jsonl timestamps quirk; the user sees the card
    /// as "completed" and expects a click to dismiss it. (v0.1.30)
    dismiss: bool,
}

#[tauri::command]
async fn cmd_focus_session_by_id(
    app: AppHandle,
    _state: State<'_, AppState>,
    input: FocusSessionInput,
) -> Result<(), String> {
    // Fire the activation IMMEDIATELY on a blocking thread — no mutex, no
    // payload lookup.  The background 750ms refresh tick holds the model lock
    // during its file-scan + JXA call, which used to add 200-500ms of latency
    // here before we could spawn the activate.
    //
    // Codex exposes a `codex://threads/<conversationId>` URL handler that both
    // activates the app and switches to the specific thread.  The session_id
    // we emit for Codex is the conversation UUID from the rollout file's
    // `session_meta.id`, so it can be plugged directly into the deep link.
    // Claude has no per-conversation scheme — only the cowork artifact handler
    // — so we fall back to a plain app activate.
    let FocusSessionInput {
        session_id,
        app_kind,
        dismiss,
    } = input;
    let session_id_for_focus = session_id.clone();
    let app_kind_for_focus = app_kind;
    tauri::async_runtime::spawn_blocking(move || match app_kind_for_focus {
        SessionApp::Codex => open_codex_thread(&session_id_for_focus),
        SessionApp::Claude => bring_app_forward("Claude"),
    });

    // Dismiss-and-refresh runs in the background; the user has already seen
    // the target app come forward by the time the lock is granted.
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app_clone.state::<AppState>();
        if dismiss {
            let mut model = state.model.lock().await;
            let inserted_new = model.dismissed_sessions.insert(session_id.clone());
            // A click on a "완료" card both opens the target and clears the
            // runtime-completion marker so it stops showing.
            let removed_runtime = model.completed_during_runtime.remove(&session_id);
            eprintln!(
                "[focus] dismiss session_id={session_id} app={:?} inserted_new={} removed_runtime={}",
                app_kind, inserted_new, removed_runtime
            );
            drop(model);
        } else {
            eprintln!(
                "[focus] focus-only session_id={session_id} app={:?} (in-progress card)",
                app_kind
            );
        }
        let _ = refresh_and_emit(&app_clone, &state).await;
    });

    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OverlayPositionInput {
    x: i32,
    y: i32,
}

/// One-shot at startup: lift the overlay's NSWindow level to
/// `NSStatusWindowLevel` (25) so that subsequent `setFrameOrigin:` calls can
/// place the window above the macOS menu bar plane.  At lower levels (Tauri's
/// alwaysOnTop sets `NSFloatingWindowLevel` = 3) the WindowServer enforces
/// "keep below the menu bar" — y values < ~34 snap back to 34, which is the
/// `actual=(_,34)` symptom we saw repeatedly.
///
/// Performed exactly once: doing this on every drag tick raises an AppKit
/// NSException → SIGABRT (v0.1.23 regression).
#[cfg(target_os = "macos")]
fn configure_overlay_window_level(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window(OVERLAY_WINDOW_LABEL) else {
        eprintln!("[level] overlay window missing; skipping setLevel");
        return;
    };
    let _ = app.run_on_main_thread(move || {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            use objc::runtime::Object;
            use objc::{msg_send, sel, sel_impl};

            let ns_window_ptr = window.ns_window().map_err(|e| e.to_string())?;
            if ns_window_ptr.is_null() {
                return Err::<(), String>("null ns_window".into());
            }
            let ns_window = ns_window_ptr as *mut Object;
            let before: i64 = msg_send![ns_window, level];
            let _: () = msg_send![ns_window, setLevel: 25_i64];
            let after: i64 = msg_send![ns_window, level];
            eprintln!("[level] NSWindow level {} -> {}", before, after);
            Ok(())
        }));
        match outcome {
            Ok(Ok(())) => {}
            Ok(Err(e)) => eprintln!("[level] setLevel err: {}", e),
            Err(_) => eprintln!("[level] setLevel PANIC caught"),
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn configure_overlay_window_level(_app: &tauri::AppHandle) {}

/// Fast-path drag step: clamps the requested logical position against the
/// monitor + pet_scale visual rect and writes it via `set_position` without
/// touching the model lock or persisting config.  Frontend calls this on every
/// pointermove tick (rAF-throttled).  Persistence + refresh happen once at
/// drag end via `cmd_finalize_drag_position`.
#[tauri::command]
fn cmd_set_overlay_position(
    app: AppHandle,
    state: State<'_, AppState>,
    input: OverlayPositionInput,
) -> Result<(), String> {
    let Some(window) = app.get_webview_window(OVERLAY_WINDOW_LABEL) else {
        return Err("overlay window missing".into());
    };
    // try_lock so a 750ms refresh tick holding the model never blocks the
    // 60Hz drag stream; fall back to the persisted default (1.0) if busy.
    let pet_scale = state
        .model
        .try_lock()
        .map(|m| m.config.pet_scale)
        .unwrap_or(1.0);
    let (cx, cy) = clamp_overlay_position(&window, pet_scale, input.x, input.y);
    // Drag-up strategy v0.1.25: rely on the one-shot setLevel: 25
    // (`configure_overlay_window_level`) to lift the WindowServer "stay below
    // menu bar" clamp, then use Tauri's regular set_position for per-tick
    // moves.  Direct setFrameOrigin: calls on a status-level window were
    // raising an AppKit NSException → SIGABRT (v0.1.24 regression).  Calling
    // set_position via Tauri keeps us inside AppKit's safe path.
    let _ = window.set_position(LogicalPosition::new(cx, cy));
    // Diagnostic: compute the actual logical position by dividing the physical
    // outer_position by the PRIMARY monitor's scale factor (NOT
    // `window.scale_factor()`, which flips between monitors in mixed-DPI
    // setups — see v0.1.30 note in CLAUDE.md).  Using the window scale here
    // made the diag log spam "[set_position] requested=… actual=…" with bogus
    // mirrored coords every time the user dragged across a monitor boundary,
    // which masked real clamping events.
    let primary_scale = window
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| m.scale_factor())
        .unwrap_or(1.0);
    if let Ok(phys) = window.outer_position() {
        let ax = (phys.x as f64 / primary_scale).round() as i32;
        let ay = (phys.y as f64 / primary_scale).round() as i32;
        // Only log when the actual differs from the request — quieter logs.
        if (ax - cx).abs() > 1 || (ay - cy).abs() > 1 {
            eprintln!(
                "[set_position] requested=({},{}) actual=({},{}) — clamped",
                cx, cy, ax, ay
            );
        }
    }
    Ok(())
}

/// Direct NSWindow setFrameOrigin: call, bypassing macOS's title-bar-visible
/// constraint that ships with setFrameTopLeftPoint:.
///
/// `logical_x` / `logical_y` are top-left coords with primary screen's
/// top-left as origin and y growing downward — same convention Tauri uses for
/// `LogicalPosition` on macOS.  Internally we convert to Cocoa's bottom-left
/// origin where y grows upward.
///
/// NOTE (v0.1.25): currently UNUSED.  Calling `setFrameOrigin:` on a window
/// whose level is `NSStatusWindowLevel` raises an AppKit NSException →
/// SIGABRT (Rust can't catch foreign exceptions).  Kept around as a reference
/// in case we need to revisit with `objc_exception` for proper NSException
/// catching.
#[allow(dead_code)]
#[cfg(target_os = "macos")]
unsafe fn set_window_origin_unconstrained(
    window: &tauri::WebviewWindow,
    logical_x: f64,
    logical_y: f64,
) -> Result<(), String> {
    use objc::runtime::{Class, Object};
    use objc::{class, msg_send, sel, sel_impl};

    #[repr(C)]
    #[derive(Copy, Clone)]
    struct NSPoint { x: f64, y: f64 }
    #[repr(C)]
    #[derive(Copy, Clone)]
    struct NSSize { width: f64, height: f64 }
    #[repr(C)]
    #[derive(Copy, Clone)]
    struct NSRect { origin: NSPoint, size: NSSize }

    let ns_window_ptr = window.ns_window().map_err(|e| e.to_string())?;
    if ns_window_ptr.is_null() {
        return Err("null ns_window".into());
    }
    let ns_window = ns_window_ptr as *mut Object;

    // Diagnostic: verify ns_window is actually an NSWindow.  Logged once.
    static LOGGED: std::sync::Once = std::sync::Once::new();
    LOGGED.call_once(|| {
        let cls: *mut Object = msg_send![ns_window, class];
        let name_ptr: *const i8 = msg_send![cls, name];
        if !name_ptr.is_null() {
            let cstr = std::ffi::CStr::from_ptr(name_ptr);
            eprintln!("[set_position] ns_window class = {:?}", cstr);
        }
        let cur_level: i64 = msg_send![ns_window, level];
        eprintln!("[set_position] live NSWindow level = {}", cur_level);
    });

    // NOTE: setLevel: is intentionally NOT called here.  Doing it on every
    // 60Hz drag tick raised an AppKit NSException → SIGABRT in v0.1.23.  The
    // window level is lifted once at startup via
    // `configure_overlay_window_level` so subsequent setFrameOrigin: calls can
    // place the window above the menu bar plane without WindowServer clamping.

    let frame: NSRect = msg_send![ns_window, frame];
    let window_height = frame.size.height;

    let ns_screen: &Class = class!(NSScreen);
    let screens: *mut Object = msg_send![ns_screen, screens];
    let count: usize = msg_send![screens, count];
    if count == 0 {
        return Err("no screens".into());
    }
    let main_screen: *mut Object = msg_send![screens, objectAtIndex: 0_usize];
    let main_frame: NSRect = msg_send![main_screen, frame];
    let primary_height = main_frame.size.height;

    // Cocoa origin = bottom-left of primary screen, y grows upward.
    // logical_y measures the window top from primary's top, y growing down.
    // window.frame.origin is the window's bottom-left in Cocoa coords:
    //   cocoa_y = primary_height - logical_y - window_height
    let cocoa_y = primary_height - logical_y - window_height;
    let origin = NSPoint { x: logical_x, y: cocoa_y };
    let _: () = msg_send![ns_window, setFrameOrigin: origin];
    Ok(())
}

/// Drag end: persist final position, ensure detached=true, and trigger a
/// refresh so the frontend updates `cardsBelow` for the new pet position.
#[tauri::command]
async fn cmd_finalize_drag_position(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // Step 1: cancel the cursor-polling loop FIRST, via the non-async
    // `drag_cancel` slot in AppState.  This is instant — no `.await` on
    // the heavy model lock — so the loop can stop at its next tick (≤16 ms).
    {
        let mut slot = state
            .drag_cancel
            .lock()
            .map_err(|e| format!("drag_cancel poisoned: {e}"))?;
        if let Some(cancel) = slot.take() {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    // Emit a terminal facing event so the frontend drops its drag-only
    // visual state (running override + scaleX(-1) mirror).  Send this
    // immediately — same rationale as the lightweight side-channel events
    // for pet_scale / pet_override: must not wait on the heavy model lock.
    let _ = app.emit(
        FACING_EVENT,
        FacingPayload {
            dragging: false,
            facing_left: false,
        },
    );

    // Step 2: snapshot the final overlay position (also instant — pure Tauri
    // window queries, no locks).
    //
    // v0.1.30: divide by PRIMARY monitor's scale factor, not
    // `window.scale_factor()`.  The latter reads `NSWindow
    // .backingScaleFactor`, which flips when the window crosses monitor
    // boundaries.  After a drag that ended on a non-Retina external, using
    // `window.scale_factor()` (1.0) on a `outer_position` that the
    // WindowServer already stored in *primary*-scaled physical pixels
    // doubles the persisted coords — which then snaps the pet to the wrong
    // spot on the next session restore.  Match the loop's primary_scale
    // convention so begin/loop/finalize all operate in the same coord space.
    let Some(window) = app.get_webview_window(OVERLAY_WINDOW_LABEL) else {
        return Ok(());
    };
    let primary_scale = window
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| m.scale_factor())
        .unwrap_or(1.0);
    let Ok(physical) = window.outer_position() else {
        return Ok(());
    };
    let lx = (physical.x as f64 / primary_scale).round() as i32;
    let ly = (physical.y as f64 / primary_scale).round() as i32;

    // Step 3: persist + refresh in the background.  We must not stall the
    // IPC reply on the model lock — this is the mirror image of the fix in
    // `cmd_begin_drag`.  Returning quickly here is harmless: the loop is
    // already cancelled and the final position is whatever the loop last
    // wrote to the window.
    let app_for_persist = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app_for_persist.state::<AppState>();
        {
            let mut model = state.model.lock().await;
            model.config.detached = true;
            model.config.detached_position = Some(SavedPosition { x: lx, y: ly });
            if let Err(e) = persist_config(&state.config_path, &model.config) {
                eprintln!("[drag] finalize background persist_config FAILED: {e}");
                return;
            }
        }
        if let Err(e) = refresh_and_emit(&app_for_persist, &state).await {
            eprintln!("[drag] finalize background refresh_and_emit FAILED: {e}");
        }
    });

    Ok(())
}

#[tauri::command]
async fn cmd_mark_detached(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    {
        let mut model = state.model.lock().await;
        if model.config.detached {
            return Ok(());
        }
        model.config.detached = true;
        persist_config(&state.config_path, &model.config)?;
    }
    refresh_and_emit(&app, &state).await?;
    Ok(())
}

#[tauri::command]
async fn cmd_reattach_overlay(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    {
        let mut model = state.model.lock().await;
        model.config.detached = false;
        persist_config(&state.config_path, &model.config)?;
    }
    refresh_and_emit(&app, &state).await?;
    Ok(())
}

#[tauri::command]
async fn cmd_focus_active_session(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    // Capture active session info and decide whether to dismiss the card.
    let active = {
        let mut model = state.model.lock().await;
        let session_info = model
            .current_payload
            .as_ref()
            .and_then(|payload| payload.overlay.active_session.as_ref())
            .map(|session| {
                let is_completed_or_idle = !session.in_progress && {
                    // Claude sessions: no in_progress flag — use base state heuristic.
                    // For both apps a Completed/Idle card means the session is not
                    // actively running right now.
                    true
                };
                (session.app_kind, session.session_id.clone(), session.cwd.clone(), session.title.clone(), is_completed_or_idle)
            });

        // If the active session is in a Completed/Idle state, mark it dismissed
        // so the card hides until the session becomes in-progress again.
        if let Some((_, ref session_id, _, _, is_done)) = session_info {
            if is_done {
                let effective = model
                    .current_payload
                    .as_ref()
                    .map(|p| p.overlay.effective_state.clone());
                let is_completed_state = matches!(
                    effective,
                    Some(PetAnimationState::Waving)
                        | Some(PetAnimationState::Idle)
                );
                if is_completed_state {
                    model.dismissed_sessions.insert(session_id.clone());
                }
            }
        }

        session_info
    };

    match active {
        Some((SessionApp::Codex, _, cwd, title, _)) => focus_codex_window_by_title(&cwd, &title)?,
        Some((SessionApp::Claude, _, cwd, title, _)) => focus_claude_window_by_title(&cwd, &title)?,
        None => focus_claude_app()?,
    }

    refresh_and_emit(&app, &state).await?;
    Ok(())
}

#[tauri::command]
async fn cmd_open_accessibility_settings() -> Result<(), String> {
    open_url("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
}

#[tauri::command]
async fn cmd_open_pets_folder() -> Result<(), String> {
    let home = home_dir()?;
    let pet_dir = home.join(".codex").join("pets");
    open_path(&pet_dir)
}

/// Returns the cursor position in overlay-window logical coordinates (origin = top-left of window).
///
/// Coordinate mapping:
///   cursor_position() → PhysicalPosition<f64>  — global screen physical pixels
///   outer_position()  → PhysicalPosition<i32>  — window top-left in physical pixels
///   scale_factor()    → f64                    — device pixel ratio (2.0 on Retina)
///
/// Formula:
///   logical_x = (cursor_physical_x - window_origin_x) / scale
///   logical_y = (cursor_physical_y - window_origin_y) / scale
///
/// Returns None when the overlay window is not found or any position query fails.
#[tauri::command]
fn cmd_cursor_position_in_overlay(app: tauri::AppHandle) -> Option<(f64, f64)> {
    let win = app.get_webview_window(OVERLAY_WINDOW_LABEL)?;
    // cursor_position() → PhysicalPosition<f64>, global screen coordinates
    let cursor = win.cursor_position().ok()?;
    // outer_position() → PhysicalPosition<i32>, window origin in physical pixels
    let origin = win.outer_position().ok()?;
    let scale = win.scale_factor().ok()?;
    let logical_x = (cursor.x - origin.x as f64) / scale;
    let logical_y = (cursor.y - origin.y as f64) / scale;
    Some((logical_x, logical_y))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config_path = app_support_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(CONFIG_FILE_NAME);
    let runtime = RuntimeModel {
        config: load_config(&config_path).unwrap_or_default(),
        ..RuntimeModel::default()
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None::<Vec<&str>>,
        ))
        .manage(AppState {
            config_path,
            model: Arc::new(Mutex::new(runtime)),
            drag_cancel: StdMutex::new(None),
            _fs_watcher: StdMutex::new(None),
        })
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = window.hide();
            }
            WindowEvent::Moved(_) if window.label() == OVERLAY_WINDOW_LABEL => {
                // Drag is now wholly frontend-driven via `cmd_set_overlay_position` /
                // `cmd_finalize_drag_position`.  This handler used to clamp on every
                // OS-emitted Moved event, but with the manual drag the position is
                // already clamped before set_position; the resulting Moved echo would
                // race with subsequent moves and cause visible jitter.  Intentional
                // no-op.
            }
            _ => {}
        })
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(ActivationPolicy::Accessory);

            build_tray(app)?;
            position_overlay_at_startup(app);

            // Restore the user's persisted hide state.  If they explicitly
            // hid the pet via the right-click menu or tray, keep it hidden
            // across restarts (CompanionConfig.pet_hidden).
            let pet_hidden = {
                let state = app.state::<AppState>();
                let model = state.model.blocking_lock();
                model.config.pet_hidden
            };
            if let Some(window) = app.get_webview_window(OVERLAY_WINDOW_LABEL) {
                let _ = if pet_hidden { window.hide() } else { window.show() };
            }

            // Lift overlay window level above the menu bar plane.  One-shot,
            // safe; per-tick setLevel: aborted the process in v0.1.23.
            configure_overlay_window_level(&app.handle().clone());

            let app_handle = app.handle().clone();

            // FSEvents-based instant session sync.  The recommended notify
            // watcher pushes a unit signal into `fs_tx` whenever a Create /
            // Modify / Remove event fires under the watched session dirs.
            // The refresh loop below races the 750 ms polling tick against
            // this channel; an event triggers a 50 ms debounce + refresh so
            // session state changes appear almost instantly instead of
            // waiting up to 750 ms for the next tick.  If watcher creation
            // or any individual watch call fails, we log and fall through
            // to the polling baseline.
            let (fs_tx, mut fs_rx) = mpsc::unbounded_channel::<()>();
            match notify::recommended_watcher({
                let tx = fs_tx.clone();
                move |res: notify::Result<NotifyEvent>| match res {
                    Ok(event) => {
                        if matches!(
                            event.kind,
                            EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
                        ) {
                            let _ = tx.send(());
                        }
                    }
                    Err(err) => {
                        eprintln!("[fs_watcher] event error: {}", err);
                    }
                }
            }) {
                Ok(mut watcher) => {
                    if let Ok(home) = home_dir() {
                        let watches: [(PathBuf, RecursiveMode); 3] = [
                            (home.join(".claude").join("projects"), RecursiveMode::Recursive),
                            (home.join(".codex").join("sessions"), RecursiveMode::Recursive),
                            (home.join(".codex"), RecursiveMode::NonRecursive),
                        ];
                        for (path, mode) in &watches {
                            if !path.exists() {
                                continue;
                            }
                            if let Err(err) = watcher.watch(path, *mode) {
                                eprintln!(
                                    "[fs_watcher] failed to watch {}: {}",
                                    path.display(),
                                    err
                                );
                            }
                        }
                    }
                    let state = app.state::<AppState>();
                    if let Ok(mut slot) = state._fs_watcher.lock() {
                        *slot = Some(watcher);
                    };
                }
                Err(err) => {
                    eprintln!("[fs_watcher] notify::recommended_watcher failed: {}", err);
                }
            }
            drop(fs_tx);

            tauri::async_runtime::spawn(async move {
                let state = app_handle.state::<AppState>();
                let _ = refresh_and_emit(&app_handle, &state).await;

                let mut interval = tokio::time::interval(Duration::from_millis(750));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            let _ = refresh_and_emit(&app_handle, &state).await;
                        }
                        Some(()) = fs_rx.recv() => {
                            // 50 ms debounce: FSEvents on macOS already
                            // coalesces with ~10 ms latency, but jsonl
                            // writers often flush multiple lines per
                            // turn — we drain any pending signals before
                            // refreshing to avoid back-to-back rebuilds.
                            tokio::time::sleep(Duration::from_millis(50)).await;
                            while fs_rx.try_recv().is_ok() {}
                            let _ = refresh_and_emit(&app_handle, &state).await;
                        }
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            cmd_begin_drag,
            cmd_finalize_drag_position,
            cmd_mark_detached,
            cmd_set_overlay_position,
            cmd_focus_active_session,
            cmd_focus_session_by_id,
            cmd_get_app_payload,
            cmd_open_accessibility_settings,
            cmd_open_pets_folder,
            cmd_pet_reaction,
            cmd_read_pet_sprite_data_url,
            cmd_reattach_overlay,
            cmd_set_language,
            cmd_set_pet_override,
            cmd_set_pet_scale,
            cmd_set_watch_apps,
            cmd_set_pet_hidden,
            cmd_set_tracked_app,
            cmd_show_settings,
            cmd_cursor_position_in_overlay,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Tauri application");
}

fn build_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let language = {
        let state = app.state::<AppState>();
        let model = state.model.blocking_lock();
        normalize_language(&model.config.language).to_string()
    };
    let is_korean = language == "ko";

    let show_pet = MenuItemBuilder::with_id(
        "show_pet",
        if is_korean { "펫 보이기/숨기기" } else { "Show / Hide Pet" },
    )
    .build(app)?;
    let attach_pet = MenuItemBuilder::with_id(
        "attach_pet",
        if is_korean { "Claude에 다시 붙이기" } else { "Attach to Claude" },
    )
    .build(app)?;
    let open_settings = MenuItemBuilder::with_id(
        "open_settings",
        if is_korean { "설정 열기" } else { "Open Settings" },
    )
    .build(app)?;
    let quit =
        MenuItemBuilder::with_id("quit", if is_korean { "종료" } else { "Quit" }).build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&show_pet, &attach_pet, &open_settings])
        .separator()
        .items(&[&quit])
        .build()?;

    let tray_icon = app
        .default_window_icon()
        .cloned()
        .expect("default bundle icon should exist");

    let _tray = TrayIconBuilder::with_id(TRAY_ID)
        .icon(tray_icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show_pet" => {
                // Compute the next state synchronously from the current
                // window visibility (the source of truth on macOS) and
                // route through `apply_pet_hidden`.  Using an absolute
                // setter — rather than `!model.config.pet_hidden` — keeps
                // concurrent toggles consistent: if cmd_set_pet_hidden's
                // spawn is queued behind a refresh tick and the user
                // clicks the tray menu right after, both targets resolve
                // to a definite hidden value instead of racing on
                // whichever spawn writes first.
                let currently_visible = app
                    .get_webview_window(OVERLAY_WINDOW_LABEL)
                    .and_then(|w| w.is_visible().ok())
                    .unwrap_or(false);
                apply_pet_hidden(app, currently_visible);
            }
            "attach_pet" => {
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = handle.state::<AppState>();
                    {
                        let mut model = state.model.lock().await;
                        model.config.detached = false;
                        let _ = persist_config(&state.config_path, &model.config);
                    }
                    let _ = refresh_and_emit(&handle, &state).await;
                });
            }
            "open_settings" => {
                if let Some(window) = app.get_webview_window(SETTINGS_WINDOW_LABEL) {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { .. } = event {
                let app = tray.app_handle();
                let currently_visible = app
                    .get_webview_window(OVERLAY_WINDOW_LABEL)
                    .and_then(|w| w.is_visible().ok())
                    .unwrap_or(false);
                apply_pet_hidden(app, currently_visible);
            }
        })
        .build(app)?;

    Ok(())
}

async fn refresh_and_emit(app: &AppHandle, state: &AppState) -> Result<(), String> {
    let (payload, pending_notifications) = {
        let mut model = state.model.lock().await;
        let previously_seen_session_ids: HashSet<String> =
            model.prev_in_progress.keys().cloned().collect();
        let mut payload = rebuild_payload(&mut model, &state.config_path)?;
        let new_expected_pos = sync_overlay_window(app, &mut model.config, &state.config_path, &payload.overlay);
        if new_expected_pos.is_some() {
            model.expected_attached_position = new_expected_pos;
        }
        payload.overlay.cards_below = compute_cards_below(app);
        if !payload.overlay.permission_granted && !model.onboarding_shown {
            if let Some(window) = app.get_webview_window(SETTINGS_WINDOW_LABEL) {
                let _ = window.show();
                let _ = window.set_focus();
            }
            model.onboarding_shown = true;
        }
        let pending_notifications = collect_notification_transitions(
            &payload,
            &previously_seen_session_ids,
            &mut model.last_notified_state,
            current_time_millis(),
        );
        model.current_payload = Some(payload.clone());
        (payload, pending_notifications)
    };
    if !pending_notifications.is_empty() {
        tauri::async_runtime::spawn_blocking(move || {
            dispatch_session_notifications(pending_notifications);
        });
    }
    app.emit(APP_EVENT, payload).map_err(|e| e.to_string())
}

/// Returns true when the pet sits close enough to the top of its monitor that
/// the card stack would clip off-screen if rendered above the pet; in that
/// case the frontend flips the stack to render below.
fn compute_cards_below(app: &AppHandle) -> bool {
    let Some(window) = app.get_webview_window(OVERLAY_WINDOW_LABEL) else {
        return false;
    };
    let scale = window.scale_factor().unwrap_or(1.0).max(1.0);
    let Ok(physical) = window.outer_position() else {
        return false;
    };
    let window_y = (physical.y as f64 / scale).round() as i32;
    let pet_local_y = OVERLAY_HEIGHT - PET_BOX_BOTTOM - PET_BOX_HEIGHT;
    let pet_top_screen = window_y + pet_local_y;

    // Find the monitor the pet sits on (or nearest).
    let Ok(monitors) = window.available_monitors() else {
        return false;
    };
    let Ok(physical_x) = window.outer_position().map(|p| (p.x as f64 / scale).round() as i32) else {
        return false;
    };
    let pet_local_x = OVERLAY_WIDTH - PET_BOX_RIGHT - PET_BOX_WIDTH;
    let pet_cx = physical_x + pet_local_x + PET_BOX_WIDTH / 2;
    let pet_cy = pet_top_screen + PET_BOX_HEIGHT / 2;
    let monitor_top = monitors
        .iter()
        .find(|m| {
            let (mx, my, mw, mh) = logical_monitor_frame(m);
            pet_cx >= mx && pet_cx <= mx + mw && pet_cy >= my && pet_cy <= my + mh
        })
        .map(|m| logical_monitor_frame(m).1);
    let Some(top) = monitor_top else {
        return false;
    };

    // Card stack max height heuristic — 6 cards × ~80px + gaps + padding.
    const CARD_STACK_RESERVE: i32 = 520;
    pet_top_screen - top < CARD_STACK_RESERVE
}

fn rebuild_payload(model: &mut RuntimeModel, config_path: &Path) -> Result<AppPayload, String> {
    let mut sessions = read_claude_sessions()?;
    sessions.extend(read_codex_sessions()?);
    // Defensively deduplicate by session_id across both sources.  When two
    // entries share the same id, keep the one with the latest last_activity_at;
    // prefer non-empty cwd / previews and the larger completed_turns count.
    sessions = dedup_sessions_by_id(sessions);
    // Second-stage dedup: for Codex sessions that share the same (app_kind, cwd)
    // — different rollout-*.jsonl files for the same workspace — keep only the
    // entry with the latest last_activity_at, merging previews and turns.
    // Sessions with an empty cwd are exempt (use session_id as key instead).
    sessions = dedup_sessions_by_workspace(sessions);
    sessions.sort_by(|a, b| b.last_activity_at.cmp(&a.last_activity_at));

    let codex_state = read_codex_global_state()?;
    let windows = query_supported_front_windows();

    // B-medium rule: clear sticky in_progress when the owning process is gone
    // or last activity is more than 3 minutes stale.
    sessions = clear_stale_in_progress(sessions, &windows);

    // v0.1.32: per-app watch filter is now enforced at the SOURCE (backend
    // payload) instead of only at the frontend `pickVisibleSessions` step.
    // Users reported that unchecking "Claude 세션 감시" failed to hide
    // Claude cards in practice — the frontend filter alone could not cover
    // every render path, and the active_session pipeline still resolved a
    // Claude session even when the user explicitly opted out.  Dropping
    // unwatched sessions here makes the contract authoritative: when
    // watch_claude=false, no Claude session can become the active session,
    // appear in the sessions list, or drive the pet sprite's state.
    {
        let watch_claude = model.config.watch_claude;
        let watch_codex = model.config.watch_codex;
        sessions.retain(|s| match s.app_kind {
            SessionApp::Claude => watch_claude,
            SessionApp::Codex => watch_codex,
        });
    }

    if let Some(frontmost_app) = windows.frontmost_app {
        model.last_focused_app = Some(frontmost_app);
    }

    let active_session = resolve_active_session(
        model,
        &sessions,
        &windows,
        &codex_state.active_workspace_roots,
    );

    // Prune & detect in_progress true→false transitions in one pass.  See
    // the function docs for the invariants this enforces.
    update_completion_tracking(
        &sessions,
        &mut model.dismissed_sessions,
        &mut model.completed_during_runtime,
        &mut model.prev_in_progress,
    );

    let turn_just_completed = detect_turn_completion(model, active_session.as_ref());
    // A turn is only "really" completed when the active session is not
    // currently in progress.  Codex emits nested `task_complete` events for
    // sub-tasks while the outer turn keeps running (in_progress=true); we
    // must not treat those as completion, otherwise the label flips to
    // "완료됨" while the assistant is still working.  Same guard applies to
    // Claude: a new user turn that arrives within the same refresh tick
    // should override the just-bumped assistant message count.
    let active_in_progress = active_session
        .as_ref()
        .map(|s| s.in_progress)
        .unwrap_or(false);
    let current_turn_completed = turn_just_completed && !active_in_progress;
    let pet_resolution = resolve_pet_selection(&mut model.config, &codex_state)?;
    let base_state = compute_base_state(active_session.as_ref(), current_turn_completed);

    // v0.1.30: scope expanded from `active_session` only to all sessions so
    // that a dismissed Codex card automatically reappears when that exact
    // session starts a new turn — even if the user is currently focused on a
    // different conversation.
    lift_dismiss_on_in_progress(&sessions, &mut model.dismissed_sessions);
    // Active-session-specific base_state lift kept as a secondary trigger so
    // that Claude sessions whose backend `in_progress` flag is stale but
    // whose computed `base_state` says Running/Waiting still have their
    // dismiss flag lifted.
    if let Some(ref session) = active_session {
        if matches!(base_state, BaseState::Running | BaseState::Waiting) {
            model.dismissed_sessions.remove(&session.session_id);
        }
    }
    let message_preview = read_message_preview(model, active_session.as_ref(), base_state);

    if current_turn_completed {
        model.override_animation = Some(AnimationLatch {
            state: PetAnimationState::Waving,
            until: Instant::now() + Duration::from_millis(WAVING_DURATION_MS),
        });
    } else if model.last_base_state.as_ref() != Some(&map_base_state(base_state)) {
        model.override_animation = Some(AnimationLatch {
            state: PetAnimationState::Jumping,
            until: Instant::now() + Duration::from_millis(JUMPING_DURATION_MS),
        });
    }
    // If a Waving latch is still pending from a previous tick but the active
    // session has re-entered in_progress, invalidate it so the label flips
    // back to "진행 중" immediately instead of waiting out the 2s window.
    if active_in_progress {
        if let Some(latch) = &model.override_animation {
            if matches!(latch.state, PetAnimationState::Waving) {
                model.override_animation = None;
            }
        }
    }
    model.last_base_state = Some(map_base_state(base_state));

    let effective_state = if let Some(latch) = &model.override_animation {
        if Instant::now() <= latch.until {
            latch.state.clone()
        } else {
            model.override_animation = None;
            map_base_state(base_state)
        }
    } else {
        map_base_state(base_state)
    };
    // When the pet would otherwise be Idle and no session anywhere is
    // running/waiting, swap to the Sleeping animation so the overlay looks
    // peaceful instead of static.
    let any_in_progress = sessions.iter().any(|s| s.in_progress);
    let effective_state = if effective_state == PetAnimationState::Idle && !any_in_progress {
        PetAnimationState::Sleeping
    } else {
        effective_state
    };
    let owner_frontmost = active_session
        .as_ref()
        .map(|session| is_app_frontmost(&windows, session.app_kind))
        .unwrap_or(false);

    // A session dismissed by the user click stays hidden until it re-enters
    // an in-progress state (the dismissed flag is cleared above when that happens).
    let is_dismissed = active_session
        .as_ref()
        .map(|s| model.dismissed_sessions.contains(&s.session_id))
        .unwrap_or(false);
    let show_card = !is_dismissed && should_show_card(&effective_state, owner_frontmost);

    // Log effective_state transitions only when state changes (not on every tick).
    if let Some(prev) = model.current_payload.as_ref() {
        if prev.overlay.effective_state != effective_state {
            eprintln!(
                "[State] {:?} -> {:?} (active={:?} in_progress={:?})",
                prev.overlay.effective_state,
                effective_state,
                active_session.as_ref().map(|s| &s.session_id),
                active_session.as_ref().map(|s| s.in_progress),
            );
        }
    }

    model.current_payload = None;
    persist_config(config_path, &model.config)?;

    Ok(AppPayload {
        codex_selected_pet_id: pet_resolution.codex_selected_pet_id,
        config: FrontendConfig {
            attached: !model.config.detached,
            language: normalize_language(&model.config.language).to_string(),
            pet_override_id: model.config.pet_override_id.clone(),
            pet_scale: model.config.pet_scale,
            tracked_app: normalize_tracked_app(&model.config.tracked_app).to_string(),
            watch_claude: model.config.watch_claude,
            watch_codex: model.config.watch_codex,
            pet_hidden: model.config.pet_hidden,
        },
        overlay: OverlaySnapshot {
            active_session,
            claude_frontmost: windows.claude.frontmost,
            codex_frontmost: windows.codex.frontmost,
            current_window_title: active_window_title(&windows, model, &sessions),
            effective_state,
            message_preview,
            permission_granted: windows.claude.permission_granted || windows.codex.permission_granted,
            pet: pet_resolution.effective_pet,
            sessions,
            show_card,
            state_label: state_label(base_state).to_string(),
            dismissed_session_ids: model.dismissed_sessions.iter().cloned().collect(),
            completed_runtime_session_ids: model
                .completed_during_runtime
                .iter()
                .cloned()
                .collect(),
            // Computed by `refresh_and_emit` based on the live overlay window
            // position; defaulted here.
            cards_below: false,
        },
        pets: list_custom_pets()?,
    })
}

fn resolve_active_session(
    model: &mut RuntimeModel,
    sessions: &[SessionSummary],
    windows: &FrontWindowState,
    codex_active_roots: &[String],
) -> Option<SessionSummary> {
    let tracked_app = select_tracked_app(model, sessions, windows);

    if let Some(app) = tracked_app {
        if let Some(session) = latest_session_for_app(sessions, app, windows, codex_active_roots) {
            return Some(session);
        }
    }

    sessions.first().cloned()
}

fn select_tracked_app(
    model: &RuntimeModel,
    sessions: &[SessionSummary],
    windows: &FrontWindowState,
) -> Option<SessionApp> {
    let tracked = normalize_tracked_app(&model.config.tracked_app);
    if tracked != "auto" {
        return Some(parse_app_kind(tracked));
    }

    if let Some(frontmost_app) = windows.frontmost_app {
        return Some(frontmost_app);
    }

    if let Some(last_focused_app) = model.last_focused_app {
        return Some(last_focused_app);
    }

    sessions.first().map(|session| session.app_kind)
}

fn latest_session_for_app(
    sessions: &[SessionSummary],
    app: SessionApp,
    windows: &FrontWindowState,
    codex_active_roots: &[String],
) -> Option<SessionSummary> {
    let candidates: Vec<SessionSummary> = sessions
        .iter()
        .filter(|session| session.app_kind == app)
        .cloned()
        .collect();

    if candidates.is_empty() {
        return None;
    }

    match app {
        SessionApp::Claude => {
            let current_title = windows.claude.title.as_deref();
            if let Some(title) = current_title {
                if let Some(session) = candidates.iter().find(|session| session.title == title) {
                    return Some(session.clone());
                }
            }
            candidates.first().cloned()
        }
        SessionApp::Codex => {
            if !codex_active_roots.is_empty() {
                if let Some(session) = candidates.iter().find(|session| {
                    codex_active_roots
                        .iter()
                        .any(|root| session.cwd.starts_with(root))
                }) {
                    return Some(session.clone());
                }
            }
            candidates.first().cloned()
        }
    }
}

fn parse_app_kind(value: &str) -> SessionApp {
    match value {
        "codex" => SessionApp::Codex,
        _ => SessionApp::Claude,
    }
}

fn is_app_frontmost(windows: &FrontWindowState, app: SessionApp) -> bool {
    match app {
        SessionApp::Claude => windows.claude.frontmost,
        SessionApp::Codex => windows.codex.frontmost,
    }
}

fn is_app_running(windows: &FrontWindowState, app: SessionApp) -> bool {
    match app {
        SessionApp::Claude => windows.claude_running,
        SessionApp::Codex => windows.codex_running,
    }
}

/// Prunes runtime-state maps for session ids that no longer exist in the
/// latest sessions list, then detects `in_progress` `true → false`
/// transitions and updates `completed_during_runtime` so the frontend can
/// surface a persistent "완료" card.
///
/// Invariants the tests in this module enforce:
///   * A session that is observed `in_progress=true` and later observed
///     `in_progress=false` (during the same Pet Companion run) ends up in
///     `completed_during_runtime`.
///   * A session that is *only* observed `in_progress=false` (never true)
///     does NOT end up in `completed_during_runtime` — its card relies on
///     the recency window instead.
///   * Re-entering `in_progress=true` clears the runtime-completion marker
///     so the next true→false transition can re-insert it.
///   * `dismissed_sessions`, `completed_during_runtime`, and
///     `prev_in_progress` never accumulate stale entries for session ids
///     that have disappeared (archived, deleted on disk, etc.).
fn update_completion_tracking(
    sessions: &[SessionSummary],
    dismissed_sessions: &mut HashSet<String>,
    completed_during_runtime: &mut HashSet<String>,
    prev_in_progress: &mut HashMap<String, bool>,
) {
    let live_ids: HashSet<&str> = sessions.iter().map(|s| s.session_id.as_str()).collect();
    dismissed_sessions.retain(|id| live_ids.contains(id.as_str()));
    completed_during_runtime.retain(|id| live_ids.contains(id.as_str()));
    prev_in_progress.retain(|id, _| live_ids.contains(id.as_str()));

    for session in sessions {
        let prev = prev_in_progress.get(&session.session_id).copied();
        if prev == Some(true) && !session.in_progress {
            completed_during_runtime.insert(session.session_id.clone());
        }
        if session.in_progress {
            completed_during_runtime.remove(&session.session_id);
        }
        prev_in_progress.insert(session.session_id.clone(), session.in_progress);
    }
}

/// Lifts the dismissed flag for any session that is currently `in_progress=true`.
///
/// CLAUDE.md policy: only a `false → true` `in_progress` transition unblocks a
/// dismissed card.  This is the per-session enforcement of that rule, applied
/// to every session in the list (not just the active one).
fn lift_dismiss_on_in_progress(
    sessions: &[SessionSummary],
    dismissed_sessions: &mut HashSet<String>,
) {
    for session in sessions {
        if session.in_progress && dismissed_sessions.contains(&session.session_id) {
            dismissed_sessions.remove(&session.session_id);
        }
    }
}

/// Apply B-medium rule: force `in_progress = false` when the owning process
/// is not running, or when `last_activity_at` is more than 3 minutes stale.
fn clear_stale_in_progress(sessions: Vec<SessionSummary>, windows: &FrontWindowState) -> Vec<SessionSummary> {
    const STALE_THRESHOLD_MS: u64 = 180_000; // 3 minutes
    let now_ms = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    sessions.into_iter().map(|mut session| {
        if !session.in_progress {
            return session;
        }
        let app_running = is_app_running(windows, session.app_kind);
        if !app_running {
            eprintln!(
                "[InProgress] session {} forced idle: app {:?} not running",
                session.session_id, session.app_kind
            );
            session.in_progress = false;
            return session;
        }
        let age_ms = now_ms.saturating_sub(session.last_activity_at);
        if age_ms > STALE_THRESHOLD_MS {
            eprintln!(
                "[InProgress] session {} forced idle: last_activity {}ms ago (>{} ms threshold)",
                session.session_id, age_ms, STALE_THRESHOLD_MS
            );
            let _ = std::fs::write(
                "/tmp/pet-companion-stale.log",
                format!(
                    "session={} forced idle: age_ms={} threshold={}\n",
                    session.session_id, age_ms, STALE_THRESHOLD_MS
                ),
            );
            session.in_progress = false;
        }
        session
    }).collect()
}

fn active_window_title(
    windows: &FrontWindowState,
    model: &RuntimeModel,
    sessions: &[SessionSummary],
) -> Option<String> {
    let app = select_tracked_app(model, sessions, windows)?;
    match app {
        SessionApp::Claude => windows.claude.title.clone(),
        SessionApp::Codex => windows.codex.title.clone(),
    }
}

fn decode_window_info(value: Option<&Value>, permission_granted: bool) -> ClaudeWindowInfo {
    let title = value
        .and_then(|item| item.get("title"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|item| !item.is_empty());

    ClaudeWindowInfo {
        frontmost: value
            .and_then(|item| item.get("frontmost"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        permission_granted,
        title,
        x: value
            .and_then(|item| item.get("x"))
            .and_then(Value::as_i64)
            .unwrap_or(0) as i32,
        y: value
            .and_then(|item| item.get("y"))
            .and_then(Value::as_i64)
            .unwrap_or(0) as i32,
        width: value
            .and_then(|item| item.get("width"))
            .and_then(Value::as_i64)
            .unwrap_or(0) as i32,
        height: value
            .and_then(|item| item.get("height"))
            .and_then(Value::as_i64)
            .unwrap_or(0) as i32,
    }
}

fn detect_turn_completion(model: &mut RuntimeModel, active_session: Option<&SessionSummary>) -> bool {
    let Some(session) = active_session else {
        return false;
    };
    let Some(completed_turns) = session.completed_turns else {
        return false;
    };

    let previous = model
        .last_completed_turns
        .insert(session.session_id.clone(), completed_turns);

    previous.is_some() && previous.unwrap_or(0) < completed_turns
}

fn compute_base_state(active_session: Option<&SessionSummary>, completed: bool) -> BaseState {
    if completed {
        return BaseState::Completed;
    }

    let Some(session) = active_session else {
        return BaseState::Idle;
    };

    if session.app_kind == SessionApp::Codex {
        if session.in_progress {
            return BaseState::Running;
        }
        if session.completed_preview.is_some() {
            return BaseState::Completed;
        }
    }

    let now_ms = current_time_millis();
    let elapsed = now_ms.saturating_sub(session.last_activity_at);

    if elapsed <= RUNNING_THRESHOLD_MS {
        BaseState::Running
    } else if elapsed <= WAITING_THRESHOLD_MS {
        BaseState::Waiting
    } else {
        BaseState::Idle
    }
}

fn map_base_state(state: BaseState) -> PetAnimationState {
    match state {
        BaseState::Idle => PetAnimationState::Idle,
        BaseState::Running => PetAnimationState::Running,
        BaseState::Waiting => PetAnimationState::Waiting,
        BaseState::Completed => PetAnimationState::Waving,
    }
}

fn state_label(state: BaseState) -> &'static str {
    match state {
        BaseState::Idle => "Idle",
        BaseState::Running => "Running",
        BaseState::Waiting => "Waiting",
        BaseState::Completed => "Completed",
    }
}

fn session_visual_state_for_notification(
    session: &SessionSummary,
    active_session_id: Option<&str>,
    overlay_effective_state: &PetAnimationState,
    completed_runtime_session_ids: &HashSet<&str>,
    now_ms: u64,
) -> PetAnimationState {
    if active_session_id == Some(session.session_id.as_str()) {
        return overlay_effective_state.clone();
    }
    if session.in_progress {
        let age_ms = now_ms.saturating_sub(session.last_activity_at);
        return if age_ms > WAITING_THRESHOLD_MS {
            PetAnimationState::Waiting
        } else {
            PetAnimationState::Running
        };
    }
    if completed_runtime_session_ids.contains(session.session_id.as_str()) {
        return PetAnimationState::Waving;
    }
    PetAnimationState::Idle
}

fn notification_state_key(state: &PetAnimationState) -> Option<&'static str> {
    match state {
        PetAnimationState::Waiting => Some("waiting"),
        PetAnimationState::Waving => Some("waving"),
        _ => None,
    }
}

fn notification_body(state: &PetAnimationState) -> Option<&'static str> {
    match state {
        PetAnimationState::Waiting => Some("입력이 필요합니다"),
        PetAnimationState::Waving => Some("작업이 완료됐습니다"),
        _ => None,
    }
}

fn app_display_name(app: SessionApp) -> &'static str {
    match app {
        SessionApp::Claude => "Claude",
        SessionApp::Codex => "Codex",
    }
}

fn project_label_from_cwd(cwd: &str) -> Option<String> {
    let path = Path::new(cwd);
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .or_else(|| {
            cwd.rsplit('/')
                .find(|segment| !segment.is_empty())
                .map(str::to_string)
        })
}

fn notification_subtitle(session: &SessionSummary) -> String {
    let project = project_label_from_cwd(&session.cwd)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| session.title.clone());
    format!("{project} · {}", app_display_name(session.app_kind))
}

fn collect_notification_transitions(
    payload: &AppPayload,
    previously_seen_session_ids: &HashSet<String>,
    last_notified_state: &mut HashMap<String, String>,
    now_ms: u64,
) -> Vec<SessionNotification> {
    let live_ids: HashSet<&str> = payload
        .overlay
        .sessions
        .iter()
        .map(|session| session.session_id.as_str())
        .collect();
    last_notified_state.retain(|session_id, _| live_ids.contains(session_id.as_str()));

    let active_session_id = payload
        .overlay
        .active_session
        .as_ref()
        .map(|session| session.session_id.as_str());
    let completed_runtime_session_ids: HashSet<&str> = payload
        .overlay
        .completed_runtime_session_ids
        .iter()
        .map(String::as_str)
        .collect();

    let mut notifications = Vec::new();
    for session in &payload.overlay.sessions {
        let state = session_visual_state_for_notification(
            session,
            active_session_id,
            &payload.overlay.effective_state,
            &completed_runtime_session_ids,
            now_ms,
        );

        if matches!(state, PetAnimationState::Running) {
            last_notified_state.remove(&session.session_id);
            continue;
        }

        let Some(state_key) = notification_state_key(&state) else {
            continue;
        };

        if !previously_seen_session_ids.contains(&session.session_id) {
            last_notified_state.insert(session.session_id.clone(), state_key.to_string());
            continue;
        }

        if last_notified_state
            .get(&session.session_id)
            .map(String::as_str)
            == Some(state_key)
        {
            continue;
        }

        last_notified_state.insert(session.session_id.clone(), state_key.to_string());
        if let Some(body) = notification_body(&state) {
            notifications.push(SessionNotification {
                session_id: session.session_id.clone(),
                state,
                subtitle: notification_subtitle(session),
                body: body.to_string(),
            });
        }
    }

    notifications
}

fn should_show_card(state: &PetAnimationState, claude_frontmost: bool) -> bool {
    if !claude_frontmost {
        return true;
    }

    matches!(
        state,
        PetAnimationState::Running
            | PetAnimationState::Waiting
            | PetAnimationState::Jumping
    )
}

fn read_message_preview(
    model: &mut RuntimeModel,
    active_session: Option<&SessionSummary>,
    base_state: BaseState,
) -> Option<String> {
    let session = active_session?;

    if session.app_kind == SessionApp::Codex {
        let selected = match base_state {
            BaseState::Running | BaseState::Waiting => {
                session.user_preview.clone().or_else(|| session.assistant_preview.clone())
            }
            BaseState::Completed | BaseState::Idle => session
                .completed_preview
                .clone()
                .or_else(|| session.assistant_preview.clone())
                .or_else(|| session.user_preview.clone()),
        }?;
        return Some(truncate_preview(&selected, 110));
    }

    let cli_session_id = session.cli_session_id.as_deref()?;
    let transcript_path = transcript_path_for(model, cli_session_id)?;
    let text = fs::read_to_string(transcript_path).ok()?;

    let mut latest_user: Option<String> = None;
    let mut latest_assistant: Option<String> = None;

    for line in text.lines().rev() {
        if latest_user.is_some() && latest_assistant.is_some() {
            break;
        }

        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };

        let kind = value.get("type").and_then(Value::as_str);
        if kind != Some("user") && kind != Some("assistant") {
            continue;
        }

        let Some(role) = value
            .get("message")
            .and_then(|message| message.get("role"))
            .and_then(Value::as_str)
        else {
            continue;
        };

        let Some(content_value) = value
            .get("message")
            .and_then(|message| message.get("content"))
        else {
            continue;
        };

        let Some(preview) = extract_preview_text(content_value) else {
            continue;
        };

        if role == "assistant" && latest_assistant.is_none() {
            latest_assistant = Some(preview.clone());
        }
        if role == "user" && latest_user.is_none() {
            latest_user = Some(preview);
        }
    }

    let selected = match base_state {
        BaseState::Running | BaseState::Waiting => latest_user.or(latest_assistant),
        BaseState::Completed | BaseState::Idle => latest_assistant.or(latest_user),
    }?;

    Some(truncate_preview(&selected, 110))
}

fn transcript_path_for(model: &mut RuntimeModel, cli_session_id: &str) -> Option<PathBuf> {
    if let Some(path) = model.transcript_paths.get(cli_session_id) {
        return Some(path.clone());
    }

    let root = home_dir().ok()?.join(".claude").join("projects");
    if !root.exists() {
        return None;
    }

    let file_name = format!("{cli_session_id}.jsonl");
    let mut found: Option<PathBuf> = None;
    visit_matching_files(&root, "", ".jsonl", &mut |path| {
        if path.file_name().and_then(|value| value.to_str()) == Some(file_name.as_str()) {
            found = Some(path.to_path_buf());
        }
        Ok(())
    })
    .ok()?;

    if let Some(path) = &found {
        model
            .transcript_paths
            .insert(cli_session_id.to_string(), path.clone());
    }

    found
}

fn extract_preview_text(content: &Value) -> Option<String> {
    match content {
        Value::String(text) => sanitize_preview(text),
        Value::Array(items) => {
            let mut parts = Vec::new();
            for item in items {
                let item_type = item.get("type").and_then(Value::as_str);
                if item_type == Some("text") {
                    if let Some(text) = item.get("text").and_then(Value::as_str) {
                        parts.push(text.to_string());
                    }
                }
            }
            sanitize_preview(&parts.join(" "))
        }
        _ => None,
    }
}

fn sanitize_preview(text: &str) -> Option<String> {
    let cleaned = text
        .replace('\n', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn truncate_preview(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }

    let truncated: String = text.chars().take(max_chars).collect();
    format!("{truncated}…")
}

fn default_language() -> String {
    "ko".to_string()
}

fn default_tracked_app() -> String {
    "auto".to_string()
}

fn default_pet_scale() -> f32 {
    1.0
}

fn default_watch_app() -> bool {
    true
}

fn normalize_language(language: &str) -> &'static str {
    match language {
        "en" => "en",
        _ => "ko",
    }
}

fn normalize_tracked_app(value: &str) -> &'static str {
    match value {
        "claude" => "claude",
        "codex" => "codex",
        _ => "auto",
    }
}

fn resolve_pet_selection(
    config: &mut PersistedConfig,
    codex_state: &CodexGlobalState,
) -> Result<PetResolution, String> {
    let pets = list_custom_pets()?;
    let by_id: HashMap<String, PetDescriptor> = pets
        .iter()
        .cloned()
        .map(|pet| (pet.id.clone(), pet))
        .collect();

    if let Some(override_id) = &config.pet_override_id {
        if let Some(pet) = by_id.get(override_id) {
            config.last_custom_pet_id = Some(pet.id.clone());
            return Ok(PetResolution {
                codex_selected_pet_id: codex_state.selected_custom_pet_id.clone(),
                effective_pet: pet.clone(),
            });
        }
        config.pet_override_id = None;
    }

    let codex_selected_pet_id = codex_state.selected_custom_pet_id.clone();
    if let Some(selected_id) = &codex_selected_pet_id {
        if let Some(pet) = by_id.get(selected_id) {
            config.last_custom_pet_id = Some(pet.id.clone());
            return Ok(PetResolution {
                codex_selected_pet_id,
                effective_pet: pet.clone(),
            });
        }
    }

    if let Some(last_id) = &config.last_custom_pet_id {
        if let Some(pet) = by_id.get(last_id) {
            return Ok(PetResolution {
                codex_selected_pet_id,
                effective_pet: pet.clone(),
            });
        }
    }

    let fallback = by_id
        .get(FALLBACK_PET_ID)
        .cloned()
        .or_else(|| pets.first().cloned())
        .ok_or_else(|| "No custom pets were found under ~/.codex/pets.".to_string())?;

    config.last_custom_pet_id = Some(fallback.id.clone());

    Ok(PetResolution {
        codex_selected_pet_id,
        effective_pet: fallback,
    })
}

fn read_codex_global_state() -> Result<CodexGlobalState, String> {
    let home = home_dir()?;
    let path = home.join(".codex").join(".codex-global-state.json");
    if !path.exists() {
        return Ok(CodexGlobalState::default());
    }
    let value: Value =
        serde_json::from_str(&fs::read_to_string(&path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let persisted = value
        .get("electron-persisted-atom-state")
        .unwrap_or(&value);
    let selected = persisted
        .get("selected-avatar-id")
        .and_then(Value::as_str)
        .map(|raw| raw.to_string());
    let roots = value
        .get("active-workspace-roots")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(CodexGlobalState {
        active_workspace_roots: roots,
        selected_custom_pet_id: selected
            .and_then(|raw| raw.strip_prefix("custom:").map(|id| id.to_string())),
    })
}

fn read_claude_sessions() -> Result<Vec<SessionSummary>, String> {
    // Real session data lives in ~/.claude/projects/<project-dir>/<uuid>.jsonl.
    // The old path (~/Library/Application Support/Claude/claude-code-sessions) does
    // not exist on this machine; all reads from that path returned an empty list.
    let root = home_dir()?.join(".claude").join("projects");

    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut jsonl_paths: Vec<PathBuf> = Vec::new();
    visit_matching_files(&root, "", ".jsonl", &mut |path| {
        jsonl_paths.push(path.to_path_buf());
        Ok(())
    })?;

    // Sort newest-modified first so the `take` below keeps recent sessions.
    jsonl_paths.sort_by_key(|p| {
        fs::metadata(p).and_then(|m| m.modified()).ok()
    });
    jsonl_paths.reverse();

    let mut sessions = Vec::new();
    for path in jsonl_paths.into_iter().take(64) {
        if let Some(summary) = read_claude_session_file(&path)? {
            sessions.push(summary);
        }
    }

    // The same logical sessionId can appear in multiple jsonl files (e.g.
    // different project dirs or duplicated transcripts).  Deduplicate here so
    // the frontend never receives duplicate keys for the same session.
    sessions = dedup_sessions_by_id(sessions);
    sessions.sort_by(|a, b| b.last_activity_at.cmp(&a.last_activity_at));
    Ok(sessions)
}

fn read_claude_session_file(path: &Path) -> Result<Option<SessionSummary>, String> {
    // The CLI session id is the file stem (uuid without .jsonl extension).
    let cli_session_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_string)
        .unwrap_or_default();

    if cli_session_id.is_empty() {
        return Ok(None);
    }

    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;

    // Each line is an independent JSON object. We scan all lines to extract:
    //   - sessionId  (field present on most lines)
    //   - cwd        (field present on most lines)
    //   - `customTitle` from `type:"custom-title"` lines (latest wins) — this is
    //     the human-curated title shown in Claude Desktop's session sidebar
    //   - first user message text  → fallback title
    //   - last assistant message preview
    //   - last user message preview
    //   - lastActivityAt from the latest `timestamp` field (ISO-8601)
    //   - completedTurns = number of assistant messages
    let mut session_id: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut custom_title: Option<String> = None;
    let mut first_user_text: Option<String> = None;
    let mut latest_user: Option<String> = None;
    let mut latest_assistant: Option<String> = None;
    let mut last_activity_at: u64 = 0;
    let mut completed_turns: u64 = 0;
    let mut latest_user_at: i64 = 0;
    let mut latest_assistant_at: i64 = 0;

    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };

        // Pick up sessionId / cwd from any line that has them.
        if session_id.is_none() {
            if let Some(id) = value.get("sessionId").and_then(Value::as_str) {
                session_id = Some(id.to_string());
            }
        }
        if cwd.is_none() {
            if let Some(dir) = value.get("cwd").and_then(Value::as_str) {
                cwd = Some(dir.to_string());
            }
        }

        // Track last activity from any ISO timestamp.
        if let Some(ts) = value.get("timestamp").and_then(Value::as_str) {
            if let Some(ms) = parse_iso_timestamp(ts) {
                last_activity_at = last_activity_at.max(ms);
            }
        }

        let msg_type = value.get("type").and_then(Value::as_str);

        // Capture user-set Claude Desktop session title.  Multiple `custom-title`
        // lines can appear per session as the user renames; latest line wins.
        if msg_type == Some("custom-title") {
            if let Some(t) = value.get("customTitle").and_then(Value::as_str) {
                if let Some(cleaned) = sanitize_preview(t) {
                    custom_title = Some(cleaned);
                }
            }
        }

        // Track per-role timestamps for in_progress detection.
        if let Some(ts) = value.get("timestamp").and_then(Value::as_str) {
            if let Some(ms) = parse_iso_timestamp(ts) {
                let ms_i64 = ms as i64;
                if msg_type == Some("user") {
                    latest_user_at = latest_user_at.max(ms_i64);
                } else if msg_type == Some("assistant") {
                    latest_assistant_at = latest_assistant_at.max(ms_i64);
                }
            }
        }

        // User messages: extract text content for title / preview.
        if msg_type == Some("user") {
            if let Some(content) = value
                .get("message")
                .and_then(|m| m.get("content"))
            {
                if let Some(preview) = extract_preview_text(content) {
                    if first_user_text.is_none() {
                        first_user_text = Some(preview.clone());
                    }
                    latest_user = Some(preview);
                }
            }
        }

        // Assistant messages: count turns and capture preview.
        if msg_type == Some("assistant") {
            completed_turns += 1;
            if let Some(content) = value
                .get("message")
                .and_then(|m| m.get("content"))
            {
                if let Some(preview) = extract_preview_text(content) {
                    latest_assistant = Some(preview);
                }
            }
        }
    }

    // in_progress: user sent a message more recently than the last assistant reply.
    // If both are 0 (no parseable timestamps), 0 > 0 is false → not in progress.
    let in_progress = latest_user_at > latest_assistant_at;

    let cwd_str = cwd.unwrap_or_default();

    // Need at least a session id to be useful.
    let effective_session_id = match session_id {
        Some(id) => id,
        None => {
            if cwd_str.is_empty() {
                return Ok(None);
            }
            cli_session_id.clone()
        }
    };

    // Title priority:
    //   1. user-set `custom-title` (Claude Desktop sidebar label, latest line wins)
    //   2. first user message text
    //   3. cwd basename
    let title = custom_title
        .clone()
        .filter(|t| !t.trim().is_empty())
        .or_else(|| first_user_text.clone().filter(|t| !t.trim().is_empty()))
        .unwrap_or_else(|| {
            Path::new(&cwd_str)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Claude session".to_string())
        });

    Ok(Some(SessionSummary {
        app_kind: SessionApp::Claude,
        cli_session_id: Some(cli_session_id),
        completed_preview: latest_assistant.clone(),
        completed_turns: Some(completed_turns),
        cwd: cwd_str,
        in_progress,
        is_archived: false,
        last_activity_at,
        session_id: effective_session_id,
        title,
        // user_preview = most recent user message (for in-progress card preview)
        // first_user_text is kept only for title derivation above
        user_preview: latest_user,
        assistant_preview: latest_assistant,
    }))
}

fn read_codex_sessions() -> Result<Vec<SessionSummary>, String> {
    let root = home_dir()?.join(".codex").join("sessions");
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    visit_matching_files(&root, "rollout-", ".jsonl", &mut |path| {
        files.push(path.to_path_buf());
        Ok(())
    })?;
    files.sort_by_key(|path| fs::metadata(path).and_then(|meta| meta.modified()).ok());
    files.reverse();

    let mut sessions = Vec::new();
    for path in files.into_iter().take(48) {
        if let Some(summary) = read_codex_session_file(&path)? {
            sessions.push(summary);
        }
    }

    sessions.sort_by(|a, b| b.last_activity_at.cmp(&a.last_activity_at));
    Ok(sessions)
}

fn read_codex_session_file(path: &Path) -> Result<Option<SessionSummary>, String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;

    let mut session_id: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut latest_user: Option<String> = None;
    let mut latest_assistant: Option<String> = None;
    let mut latest_completed: Option<String> = None;
    let mut last_activity_at: u64 = 0;
    let mut in_progress = false;
    let mut completed_turns = 0u64;

    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };

        let timestamp = value
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_iso_timestamp)
            .unwrap_or(0);
        if timestamp > 0 {
            last_activity_at = last_activity_at.max(timestamp);
        }

        match value.get("type").and_then(Value::as_str) {
            Some("session_meta") => {
                if let Some(payload) = value.get("payload") {
                    session_id = payload
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .or(session_id);
                    cwd = payload
                        .get("cwd")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .or(cwd);
                }
            }
            Some("event_msg") => {
                let Some(payload) = value.get("payload") else {
                    continue;
                };
                match payload.get("type").and_then(Value::as_str) {
                    Some("task_started") => in_progress = true,
                    Some("task_complete") => {
                        in_progress = false;
                        completed_turns += 1;
                        if let Some(text) = payload.get("last_agent_message").and_then(Value::as_str)
                        {
                            latest_completed = sanitize_preview(text);
                        }
                    }
                    Some("user_message") => {
                        if let Some(text) = payload.get("message").and_then(Value::as_str) {
                            latest_user = sanitize_preview(text);
                        }
                    }
                    Some("agent_message") => {
                        if let Some(text) = payload.get("message").and_then(Value::as_str) {
                            latest_assistant = sanitize_preview(text);
                        }
                    }
                    _ => {}
                }
            }
            Some("response_item") => {
                let Some(payload) = value.get("payload") else {
                    continue;
                };
                if payload.get("type").and_then(Value::as_str) == Some("message")
                    && payload.get("role").and_then(Value::as_str) == Some("assistant")
                {
                    if let Some(content) = payload.get("content").and_then(Value::as_array) {
                        for item in content {
                            if item.get("type").and_then(Value::as_str) == Some("output_text") {
                                if let Some(text) = item.get("text").and_then(Value::as_str) {
                                    latest_assistant = sanitize_preview(text);
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    let cwd = cwd.unwrap_or_default();
    if cwd.is_empty() && session_id.is_none() {
        return Ok(None);
    }

    let title = latest_user
        .clone()
        .or_else(|| latest_completed.clone())
        .unwrap_or_else(|| {
            Path::new(&cwd)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Codex session".to_string())
        });

    Ok(Some(SessionSummary {
        app_kind: SessionApp::Codex,
        cli_session_id: None,
        completed_preview: latest_completed,
        completed_turns: Some(completed_turns),
        cwd,
        in_progress,
        is_archived: false,
        last_activity_at,
        session_id: session_id.unwrap_or_else(|| path.to_string_lossy().into_owned()),
        title,
        user_preview: latest_user,
        assistant_preview: latest_assistant,
    }))
}

/// Deduplicate a list of `SessionSummary` values by `session_id`.
/// When two entries share the same id the merge strategy is:
///   - keep the latest `last_activity_at`
///   - prefer non-empty `cwd`, `user_preview`, `assistant_preview`, `completed_preview`
///   - keep the larger `completed_turns`
///   - `in_progress` is true if either entry has it true
fn dedup_sessions_by_id(sessions: Vec<SessionSummary>) -> Vec<SessionSummary> {
    let mut map: HashMap<String, SessionSummary> = HashMap::new();
    for session in sessions {
        let id = session.session_id.clone();
        if let Some(existing) = map.get_mut(&id) {
            // Merge in place: existing keeps priority, incoming fills gaps.
            existing.last_activity_at = existing.last_activity_at.max(session.last_activity_at);
            if existing.cwd.is_empty() && !session.cwd.is_empty() {
                existing.cwd = session.cwd;
            }
            if existing.user_preview.is_none() && session.user_preview.is_some() {
                existing.user_preview = session.user_preview;
            }
            if existing.assistant_preview.is_none() && session.assistant_preview.is_some() {
                existing.assistant_preview = session.assistant_preview;
            }
            if existing.completed_preview.is_none() && session.completed_preview.is_some() {
                existing.completed_preview = session.completed_preview;
            }
            let existing_turns = existing.completed_turns.unwrap_or(0);
            let incoming_turns = session.completed_turns.unwrap_or(0);
            if incoming_turns > existing_turns {
                existing.completed_turns = Some(incoming_turns);
            }
            existing.in_progress = existing.in_progress || session.in_progress;
        } else {
            map.insert(id, session);
        }
    }
    map.into_values().collect()
}

/// Second-stage dedup: group sessions by `(app_kind, cwd)` and collapse
/// duplicates that arise when multiple rollout-*.jsonl files exist for the same
/// logical workspace.  Sessions with an empty `cwd` are kept as-is (they are
/// keyed by `session_id` so they pass through individually).
fn dedup_sessions_by_workspace(sessions: Vec<SessionSummary>) -> Vec<SessionSummary> {
    // key: (app_kind_discriminant, workspace_key)
    // workspace_key = cwd when non-empty, otherwise session_id
    let mut map: HashMap<(u8, String), SessionSummary> = HashMap::new();
    for session in sessions {
        let app_disc: u8 = match session.app_kind {
            SessionApp::Claude => 0,
            SessionApp::Codex => 1,
        };
        let workspace_key = if session.cwd.is_empty() {
            session.session_id.clone()
        } else {
            session.cwd.clone()
        };
        let key = (app_disc, workspace_key);
        if let Some(existing) = map.get_mut(&key) {
            if session.last_activity_at > existing.last_activity_at {
                // Incoming is newer: take its id/title/timestamp, merge the rest.
                let merged_turns = session.completed_turns.unwrap_or(0)
                    .max(existing.completed_turns.unwrap_or(0));
                let merged_in_progress = session.in_progress || existing.in_progress;
                let merged_user = session.user_preview.clone()
                    .or_else(|| existing.user_preview.clone());
                let merged_assistant = session.assistant_preview.clone()
                    .or_else(|| existing.assistant_preview.clone());
                let merged_completed = session.completed_preview.clone()
                    .or_else(|| existing.completed_preview.clone());
                let merged_cli = session.cli_session_id.clone()
                    .or_else(|| existing.cli_session_id.clone());
                *existing = SessionSummary {
                    last_activity_at: session.last_activity_at,
                    session_id: session.session_id,
                    title: session.title,
                    completed_turns: Some(merged_turns),
                    in_progress: merged_in_progress,
                    user_preview: merged_user,
                    assistant_preview: merged_assistant,
                    completed_preview: merged_completed,
                    cwd: session.cwd,
                    app_kind: session.app_kind,
                    cli_session_id: merged_cli,
                    is_archived: session.is_archived || existing.is_archived,
                };
            } else {
                // Existing is newer: just fill gaps.
                existing.completed_turns = Some(
                    existing.completed_turns.unwrap_or(0)
                        .max(session.completed_turns.unwrap_or(0)),
                );
                existing.in_progress = existing.in_progress || session.in_progress;
                if existing.user_preview.is_none() {
                    existing.user_preview = session.user_preview;
                }
                if existing.assistant_preview.is_none() {
                    existing.assistant_preview = session.assistant_preview;
                }
                if existing.completed_preview.is_none() {
                    existing.completed_preview = session.completed_preview;
                }
            }
        } else {
            map.insert(key, session);
        }
    }
    map.into_values().collect()
}

fn list_custom_pets() -> Result<Vec<PetDescriptor>, String> {
    let root = home_dir()?.join(".codex").join("pets");
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut pets = Vec::new();
    let entries = fs::read_dir(root).map_err(|e| e.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let manifest_path = path.join("pet.json");
        if !manifest_path.exists() {
            continue;
        }
        let manifest_text = fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?;
        let manifest: PetManifest =
            serde_json::from_str(&manifest_text).map_err(|e| e.to_string())?;
        let sprite_sheet_path = path.join(manifest.spritesheet_path);
        if !sprite_sheet_path.exists() {
            continue;
        }
        pets.push(PetDescriptor {
            description: manifest.description,
            display_name: manifest.display_name,
            id: manifest.id,
            source: "custom".to_string(),
            sprite_sheet_path: sprite_sheet_path.to_string_lossy().into_owned(),
        });
    }
    pets.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    Ok(pets)
}

fn query_supported_front_windows() -> FrontWindowState {
    let script = r#"
const app = Application("System Events");
app.includeStandardAdditions = true;
const result = {
  permissionGranted: true,
  frontmostApp: null,
  claudeRunning: false,
  codexRunning: false,
  claude: { frontmost: false, title: null, x: 0, y: 0, width: 0, height: 0 },
  codex: { frontmost: false, title: null, x: 0, y: 0, width: 0, height: 0 }
};

try {
  // Detect running processes (does not require Accessibility, just System Events).
  const allProcs = app.processes.whose({ backgroundOnly: false });
  const procNames = [];
  for (let i = 0; i < allProcs.length; i++) {
    try { procNames.push(String(allProcs[i].name())); } catch(_) {}
  }
  result.claudeRunning = procNames.indexOf("Claude") !== -1;
  result.codexRunning = procNames.indexOf("Codex") !== -1 || procNames.indexOf("Codex CLI") !== -1;

  const appNames = [["Claude", "claude"], ["Codex", "codex"]];
  for (const [processName, key] of appNames) {
    const proc = app.processes.byName(processName);
    if (!proc.exists()) continue;
    const frontmost = proc.frontmost();
    result[key].frontmost = !!frontmost;
    if (frontmost) {
      result.frontmostApp = key;
    }
    const windows = proc.windows();
    if (windows.length > 0) {
      const front = windows[0];
      result[key].title = String(front.name() || "");
      const pos = front.position();
      const size = front.size();
      result[key].x = Number(pos[0]);
      result[key].y = Number(pos[1]);
      result[key].width = Number(size[0]);
      result[key].height = Number(size[1]);
    }
  }
  console.log(JSON.stringify(result));
} catch (err) {
  console.log(JSON.stringify({ permissionGranted: false, claudeRunning: false, codexRunning: false }));
}
"#;

    let output = Command::new("osascript")
        .arg("-l")
        .arg("JavaScript")
        .arg("-e")
        .arg(script)
        .output();

    let Ok(output) = output else {
        return FrontWindowState::default();
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let Ok(value) = serde_json::from_str::<Value>(stdout.trim()) else {
        return FrontWindowState::default();
    };

    let permission_granted = value
        .get("permissionGranted")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let frontmost_app = value
        .get("frontmostApp")
        .and_then(Value::as_str)
        .map(parse_app_kind);
    let claude_running = value
        .get("claudeRunning")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let codex_running = value
        .get("codexRunning")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    eprintln!("[ProcessDetect] claudeRunning={}, codexRunning={}", claude_running, codex_running);
    // Write to a temp file so harness-captured runs can still be verified.
    let _ = std::fs::write(
        "/tmp/pet-companion-detect.log",
        format!("claudeRunning={} codexRunning={}\n", claude_running, codex_running),
    );

    FrontWindowState {
        claude: decode_window_info(value.get("claude"), permission_granted),
        codex: decode_window_info(value.get("codex"), permission_granted),
        frontmost_app,
        claude_running,
        codex_running,
    }
}

/// Synchronise the overlay window position.
/// Returns the logical (x, y) that was written via set_position, or None if the
/// window was not moved (e.g. detached mode — caller should not update
/// expected_attached_position in that case).
fn sync_overlay_window(
    app: &AppHandle,
    config: &mut PersistedConfig,
    _config_path: &Path,
    overlay: &OverlaySnapshot,
) -> Option<(i32, i32)> {
    let Some(window) = app.get_webview_window(OVERLAY_WINDOW_LABEL) else {
        return None;
    };

    // Detached mode: position is wholly user-controlled (set via
    // cmd_set_overlay_position).  No automatic repositioning — what the user
    // dragged stays where they dragged it, no surprise teleports.  The
    // startup pass (position_overlay_at_startup) is the only safety net.
    if config.detached {
        return None;
    }

    // Attached mode — try to follow the owner app window if permission allows.
    let windows = query_supported_front_windows();

    let Some(active_session) = &overlay.active_session else {
        // No active session: keep overlay at safe home.
        return Some(move_overlay_to_safe_home(&window));
    };

    let current_window = match active_session.app_kind {
        SessionApp::Claude => &windows.claude,
        SessionApp::Codex => &windows.codex,
    };
    let owner_frontmost = matches!(windows.frontmost_app, Some(app) if app == active_session.app_kind);

    if owner_frontmost
        && current_window.permission_granted
        && current_window.width > 0
        && current_window.height > 0
    {
        let x = current_window.x + current_window.width - OVERLAY_WIDTH - ATTACHED_MARGIN_X;
        let y = current_window.y + current_window.height - OVERLAY_HEIGHT - ATTACHED_MARGIN_Y;
        let (next_x, next_y) = clamp_overlay_position(&window, config.pet_scale, x, y);
        let _ = window.set_position(LogicalPosition::new(next_x, next_y));
        Some((next_x, next_y))
    } else {
        // Permission denied or app not frontmost: keep overlay at safe home so
        // the pet is always visible at the bottom-right corner of the primary monitor.
        Some(move_overlay_to_safe_home(&window))
    }
}

fn focus_claude_app() -> Result<(), String> {
    bring_app_forward("Claude");
    Ok(())
}

/// Fast app activation: single `osascript` invocation that combines activate
/// + unhide.  `tell application X to activate` will launch the app if it's not
/// running, bring it foreground, and unhide hidden windows in one Apple Event.
/// The `if running` guard on `System Events` ensures we don't trigger a launch
/// via System Events if the app is unknown.
fn bring_app_forward(app_name: &str) {
    let esc = escape_applescript(app_name);
    let script = format!(
        r#"tell application "{esc}" to activate
tell application "System Events"
  if exists process "{esc}" then set visible of process "{esc}" to true
end tell"#
    );
    let out = Command::new("osascript").arg("-e").arg(&script).output();
    if let Ok(o) = &out {
        if !o.status.success() {
            let err = String::from_utf8_lossy(&o.stderr);
            eprintln!("[focus] activate '{app_name}' failed: {}", err.trim());
        }
    }
}

fn focus_claude_window_by_title(_cwd: &str, _title: &str) -> Result<(), String> {
    bring_app_forward("Claude");
    Ok(())
}

fn focus_codex_app() -> Result<(), String> {
    bring_app_forward("Codex");
    Ok(())
}

/// Deep-link to a specific Codex conversation.  Codex registers
/// `codex://threads/<id>` which both raises the app and switches to that thread
/// in one call — no AppleScript, no mutex, no extra unhide step.
fn open_codex_thread(session_id: &str) {
    let url = format!("codex://threads/{session_id}");
    let out = Command::new("open").arg(&url).status();
    if let Ok(s) = out {
        if !s.success() {
            eprintln!("[focus] open '{url}' exited with {s}");
        }
    } else if let Err(e) = out {
        eprintln!("[focus] open '{url}' failed: {e}");
    }
    // `open codex://…` triggers the URL handler but does not always raise the
    // window to the front when Codex is hidden or backgrounded.  Follow up
    // with an explicit activate so the thread becomes visible immediately.
    bring_app_forward("Codex");
}

fn focus_codex_window_by_title(_cwd: &str, _title: &str) -> Result<(), String> {
    bring_app_forward("Codex");
    Ok(())
}

#[allow(dead_code)]
fn escape_applescript(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn frontmost_app_name_via_jxa() -> Result<String, String> {
    let script = r#"
try {
  const name = Application("System Events").frontmostApplication().name();
  console.log(String(name || ""));
} catch (err) {
  console.log(`ERROR:${String(err)}`);
}
"#;

    let output = Command::new("osascript")
        .arg("-l")
        .arg("JavaScript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if let Some(err) = stdout.strip_prefix("ERROR:") {
        return Err(err.trim().to_string());
    }

    Ok(stdout)
}

#[cfg(target_os = "macos")]
fn frontmost_app_name_via_nsworkspace() -> Option<String> {
    use std::ffi::CStr;
    use std::os::raw::c_char;

    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        let workspace: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
        if workspace.is_null() {
            return None;
        }
        let frontmost_app: *mut Object = msg_send![workspace, frontmostApplication];
        if frontmost_app.is_null() {
            return None;
        }
        let localized_name: *mut Object = msg_send![frontmost_app, localizedName];
        if localized_name.is_null() {
            return None;
        }
        let utf8: *const c_char = msg_send![localized_name, UTF8String];
        if utf8.is_null() {
            return None;
        }
        Some(CStr::from_ptr(utf8).to_string_lossy().into_owned())
    }
}

#[cfg(not(target_os = "macos"))]
fn frontmost_app_name_via_nsworkspace() -> Option<String> {
    None
}

fn is_pet_companion_frontmost() -> Result<bool, String> {
    match frontmost_app_name_via_jxa() {
        Ok(name) => Ok(name == "Pet Companion"),
        Err(jxa_err) => {
            if let Some(name) = frontmost_app_name_via_nsworkspace() {
                eprintln!("[notify] JXA frontmost check failed, used NSWorkspace fallback: {jxa_err}");
                return Ok(name == "Pet Companion");
            }
            Err(jxa_err)
        }
    }
}

fn send_session_notification(notification: &SessionNotification) {
    let title = escape_applescript("Pet Companion");
    let subtitle = escape_applescript(&notification.subtitle);
    let body = escape_applescript(&notification.body);
    let script = format!(
        r#"display notification "{body}" with title "{title}" subtitle "{subtitle}""#
    );
    match Command::new("osascript").arg("-e").arg(&script).output() {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            eprintln!(
                "[notify] notification failed session_id={} state={:?}: {}",
                notification.session_id,
                notification.state,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Err(err) => {
            eprintln!(
                "[notify] notification command failed session_id={} state={:?}: {}",
                notification.session_id, notification.state, err
            );
        }
    }
}

fn dispatch_session_notifications(notifications: Vec<SessionNotification>) {
    match is_pet_companion_frontmost() {
        Ok(true) => {}
        Ok(false) => {
            for notification in &notifications {
                send_session_notification(notification);
            }
        }
        Err(err) => {
            eprintln!("[notify] frontmost check failed: {err}");
        }
    }
}

fn open_url(url: &str) -> Result<(), String> {
    Command::new("open")
        .arg(url)
        .status()
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn open_path(path: &Path) -> Result<(), String> {
    Command::new("open")
        .arg(path)
        .status()
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn current_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn parse_iso_timestamp(value: &str) -> Option<u64> {
    let dt = time::OffsetDateTime::parse(value, &Rfc3339).ok()?;
    Some(dt.unix_timestamp_nanos() as u64 / 1_000_000)
}

fn home_dir() -> Result<PathBuf, String> {
    env::var("HOME")
        .map(PathBuf::from)
        .map_err(|e| format!("HOME is not set: {e}"))
}

fn app_support_dir() -> Result<PathBuf, String> {
    Ok(home_dir()?
        .join("Library")
        .join("Application Support")
        .join("ClaudePetCompanion"))
}

fn load_config(path: &Path) -> Result<PersistedConfig, String> {
    if !path.exists() {
        return Ok(PersistedConfig::default());
    }
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

fn persist_config(path: &Path, config: &PersistedConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())
}

fn position_overlay_at_startup(app: &mut tauri::App) {
    let Some(window) = app.get_webview_window(OVERLAY_WINDOW_LABEL) else {
        return;
    };

    let state = app.state::<AppState>();
    let mut model = state.model.blocking_lock();
    let pet_scale = model.config.pet_scale;
    if let Some(pos) = &model.config.detached_position {
        let (next_x, next_y) = clamp_overlay_position(&window, pet_scale, pos.x, pos.y);
        let _ = window.set_position(LogicalPosition::new(next_x, next_y));
        model.config.detached_position = Some(SavedPosition { x: next_x, y: next_y });
        let _ = persist_config(&state.config_path, &model.config);
        return;
    }

    if let Ok(Some(monitor)) = window.current_monitor() {
        let (mx, my, width, height) = logical_monitor_frame(&monitor);
        let x = mx + width - OVERLAY_WIDTH - ATTACHED_MARGIN_X;
        let y = my + height - OVERLAY_HEIGHT - ATTACHED_MARGIN_Y;
        let (next_x, next_y) = clamp_overlay_position(&window, pet_scale, x, y);
        let _ = window.set_position(LogicalPosition::new(next_x, next_y));
    } else {
        // Startup — expected_attached_position will be written by the first
        // sync_overlay_window call; discard the return value here.
        let _ = move_overlay_to_safe_home(&window);
    }
}

fn move_overlay_to_safe_home(window: &tauri::WebviewWindow) -> (i32, i32) {
    let (x, y) = safe_home_position(window);
    let _ = window.set_position(LogicalPosition::new(x, y));
    (x, y)
}

fn safe_home_position(window: &tauri::WebviewWindow) -> (i32, i32) {
    let position_from_frame = |x: i32, y: i32, width: i32, height: i32| -> (i32, i32) {
        // Keep the overlay inside the monitor's visible work area:
        //   • top  — reserve room for the macOS menu bar
        //   • bottom — ATTACHED_MARGIN_Y keeps it off the dock edge
        //   • right  — ATTACHED_MARGIN_X keeps it off the screen edge
        let overlay_x = (x + width - OVERLAY_WIDTH - ATTACHED_MARGIN_X)
            .clamp(x, x + width - OVERLAY_WIDTH);
        let min_y = y + MACOS_MENU_BAR_HEIGHT;
        let max_y = y + height - OVERLAY_HEIGHT - ATTACHED_MARGIN_Y;
        let overlay_y = (y + height - OVERLAY_HEIGHT - ATTACHED_MARGIN_Y)
            .clamp(min_y, max_y.max(min_y));
        (overlay_x, overlay_y)
    };

    if let Ok(Some(monitor)) = window.primary_monitor() {
        let (x, y, width, height) = logical_monitor_frame(&monitor);
        return position_from_frame(x, y, width, height);
    }

    if let Ok(monitors) = window.available_monitors() {
        if let Some(monitor) = monitors.first() {
            let (x, y, width, height) = logical_monitor_frame(monitor);
            return position_from_frame(x, y, width, height);
        }
    }

    (ATTACHED_MARGIN_X, ATTACHED_MARGIN_Y + MACOS_MENU_BAR_HEIGHT)
}

fn clamp_overlay_position(
    window: &tauri::WebviewWindow,
    pet_scale: f32,
    x: i32,
    y: i32,
) -> (i32, i32) {
    let Ok(monitors) = window.available_monitors() else {
        return (x, y);
    };
    if monitors.is_empty() {
        return (x, y);
    }

    // CSS `transform: scale()` grows the pet symmetrically around its center,
    // so the visual rectangle widens by (scale-1) * size on each axis.  Clamp
    // against the *visual* rectangle so the pet never clips off-screen at
    // larger sprite sizes.
    let pet_scale = pet_scale.clamp(0.5, 2.0);
    let half_w = ((PET_BOX_WIDTH as f32 * pet_scale) * 0.5).round() as i32;
    let half_h = ((PET_BOX_HEIGHT as f32 * pet_scale) * 0.5).round() as i32;

    // Center of the pet sprite in screen coords.
    let pet_local_x = OVERLAY_WIDTH - PET_BOX_RIGHT - PET_BOX_WIDTH;
    let pet_local_y = OVERLAY_HEIGHT - PET_BOX_BOTTOM - PET_BOX_HEIGHT;
    let center_offset_x = pet_local_x + PET_BOX_WIDTH / 2;
    let center_offset_y = pet_local_y + PET_BOX_HEIGHT / 2;
    let pet_center_x = x + center_offset_x;
    let pet_center_y = y + center_offset_y;

    // Per-monitor clamping breaks on multi-monitor setups: if the user drags
    // the pet across a monitor boundary, pet_center temporarily falls into a
    // logical-coord gap between adjacent monitors (especially when scale
    // factors differ — each monitor's physical position divided by its own
    // scale produces non-contiguous logical frames).  The "closest monitor"
    // fallback then yanks the pet back into the source monitor, capping the
    // drag at that monitor's menu bar.
    //
    // Use the UNION bounding rect of every monitor's logical frame instead.
    // This lets the pet traverse all monitors freely.  The trade-off is that
    // in L-shaped arrangements the pet could enter a corner gap that isn't
    // covered by any monitor — acceptable for now; users won't typically
    // drag into a dead corner.
    let frames: Vec<(i32, i32, i32, i32)> =
        monitors.iter().map(logical_monitor_frame).collect();
    let union_min_x = frames.iter().map(|f| f.0).min().unwrap_or(0);
    let union_min_y = frames.iter().map(|f| f.1).min().unwrap_or(0);
    let union_max_x = frames.iter().map(|f| f.0 + f.2).max().unwrap_or(0);
    let union_max_y = frames.iter().map(|f| f.1 + f.3).max().unwrap_or(0);

    let min_cx = union_min_x + PET_VISIBLE_PADDING + half_w;
    let max_cx = union_max_x - PET_VISIBLE_PADDING - half_w;
    // Top axis drops PET_VISIBLE_PADDING so the pet can hug the menu bar.
    // We assume the topmost monitor carries macOS's menu bar.
    let min_cy = union_min_y + MACOS_MENU_BAR_HEIGHT + half_h;
    let max_cy = union_max_y - PET_VISIBLE_PADDING - half_h;

    let clamped_cx = pet_center_x.clamp(min_cx, max_cx.max(min_cx));
    let clamped_cy = pet_center_y.clamp(min_cy, max_cy.max(min_cy));

    (
        clamped_cx - center_offset_x,
        clamped_cy - center_offset_y,
    )
}

fn logical_monitor_frame(monitor: &tauri::Monitor) -> (i32, i32, i32, i32) {
    let scale = monitor.scale_factor().max(1.0);
    let position = monitor.position();
    let size = monitor.size();

    let x = (position.x as f64 / scale).round() as i32;
    let y = (position.y as f64 / scale).round() as i32;
    let width = (size.width as f64 / scale).round() as i32;
    let height = (size.height as f64 / scale).round() as i32;

    (x, y, width, height)
}

fn visit_matching_files(
    root: &Path,
    prefix: &str,
    suffix: &str,
    visitor: &mut dyn FnMut(&Path) -> Result<(), String>,
) -> Result<(), String> {
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            visit_matching_files(&path, prefix, suffix, visitor)?;
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if file_name.starts_with(prefix) && file_name.ends_with(suffix) {
            visitor(&path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Unit tests for the pure state-derivation helpers used by
    //! `rebuild_payload`.  These cover the three invariants the user-reported
    //! "completed card disappears" regression points at:
    //!
    //!   1. `update_completion_tracking` correctly detects `true → false`
    //!      `in_progress` transitions and surfaces them via
    //!      `completed_during_runtime`.
    //!   2. `lift_dismiss_on_in_progress` releases a dismissed card *only*
    //!      when the same session re-enters `in_progress` — never for
    //!      unrelated session activity.
    //!   3. `clear_stale_in_progress` forces `in_progress=false` for stale or
    //!      orphaned sessions, and the interaction with
    //!      `update_completion_tracking` does NOT spuriously promote a first
    //!      observation into the runtime-completed set.
    //!
    //! The fourth section documents an integration-level pitfall:
    //! `dedup_sessions_by_workspace` can swap `session_id` mid-stream when
    //! multiple rollout files share a `(app_kind, cwd)`, and our
    //! prev_in_progress map keys by the *post-dedup* id.  We assert the
    //! current behavior so a future regression in dedup ordering becomes
    //! visible in test output.

    use super::*;

    fn session(id: &str, in_progress: bool, last_activity_at: u64) -> SessionSummary {
        SessionSummary {
            app_kind: SessionApp::Claude,
            cli_session_id: None,
            completed_preview: None,
            completed_turns: None,
            cwd: format!("/tmp/{}", id),
            in_progress,
            is_archived: false,
            last_activity_at,
            session_id: id.to_string(),
            title: id.to_string(),
            user_preview: None,
            assistant_preview: None,
        }
    }

    fn windows_both_running() -> FrontWindowState {
        FrontWindowState {
            claude_running: true,
            codex_running: true,
            ..Default::default()
        }
    }

    fn notification_payload(
        active_session: Option<SessionSummary>,
        sessions: Vec<SessionSummary>,
        effective_state: PetAnimationState,
        completed_runtime_session_ids: &[&str],
    ) -> AppPayload {
        AppPayload {
            codex_selected_pet_id: None,
            config: FrontendConfig {
                attached: true,
                language: "ko".to_string(),
                manual_session_app: None,
                manual_session_id: None,
                pet_override_id: None,
                pet_scale: 1.0,
                tracked_app: "auto".to_string(),
            },
            overlay: OverlaySnapshot {
                active_session,
                claude_frontmost: false,
                codex_frontmost: false,
                current_window_title: None,
                effective_state,
                message_preview: None,
                manual_session_missing: false,
                manual_session_pinned: false,
                permission_granted: true,
                pet: PetDescriptor {
                    description: String::new(),
                    display_name: "Bori".to_string(),
                    id: "bori".to_string(),
                    source: "custom".to_string(),
                    sprite_sheet_path: String::new(),
                },
                sessions,
                show_card: true,
                state_label: "Idle".to_string(),
                dismissed_session_ids: vec![],
                completed_runtime_session_ids: completed_runtime_session_ids
                    .iter()
                    .map(|id| (*id).to_string())
                    .collect(),
                cards_below: false,
            },
            pets: vec![],
        }
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    // ---------- update_completion_tracking ----------

    #[test]
    fn update_completion_tracking_first_observation_does_not_mark_completed() {
        // A session observed for the first time as `in_progress=true` must
        // not be added to `completed_during_runtime`.  Only a *transition*
        // from true→false counts.
        let sessions = vec![session("s1", true, 1_000)];
        let mut dismissed = HashSet::new();
        let mut completed = HashSet::new();
        let mut prev = HashMap::new();

        update_completion_tracking(&sessions, &mut dismissed, &mut completed, &mut prev);

        assert!(completed.is_empty(), "first observation should not mark completed");
        assert_eq!(prev.get("s1"), Some(&true));
    }

    #[test]
    fn update_completion_tracking_first_observation_false_does_not_mark_completed() {
        // A session whose first observation is already `in_progress=false`
        // must not be marked completed — we never saw the "running" phase.
        // This is the guard against false positives on Pet Companion startup.
        let sessions = vec![session("s1", false, 1_000)];
        let mut dismissed = HashSet::new();
        let mut completed = HashSet::new();
        let mut prev = HashMap::new();

        update_completion_tracking(&sessions, &mut dismissed, &mut completed, &mut prev);

        assert!(completed.is_empty(), "first observation (false) must not mark completed");
        assert_eq!(prev.get("s1"), Some(&false));
    }

    #[test]
    fn update_completion_tracking_true_to_false_transition_marks_completed() {
        // The headline scenario: user sees a running session, the assistant
        // finishes, and the card must persist in "완료" state.
        let mut dismissed = HashSet::new();
        let mut completed = HashSet::new();
        let mut prev = HashMap::new();

        // Tick 1: in_progress = true
        update_completion_tracking(
            &[session("s1", true, 1_000)],
            &mut dismissed,
            &mut completed,
            &mut prev,
        );
        assert!(completed.is_empty());

        // Tick 2: in_progress = false (transition!)
        update_completion_tracking(
            &[session("s1", false, 2_000)],
            &mut dismissed,
            &mut completed,
            &mut prev,
        );
        assert!(completed.contains("s1"), "true→false transition must mark completed");
        assert_eq!(prev.get("s1"), Some(&false));
    }

    #[test]
    fn update_completion_tracking_reenter_in_progress_clears_completed() {
        let mut dismissed = HashSet::new();
        let mut completed = HashSet::new();
        let mut prev = HashMap::new();

        // Build up the completed-runtime marker.
        update_completion_tracking(&[session("s1", true, 1_000)], &mut dismissed, &mut completed, &mut prev);
        update_completion_tracking(&[session("s1", false, 2_000)], &mut dismissed, &mut completed, &mut prev);
        assert!(completed.contains("s1"));

        // New turn starts — the runtime-completed marker must be cleared so
        // the card flips back to "진행 중".
        update_completion_tracking(&[session("s1", true, 3_000)], &mut dismissed, &mut completed, &mut prev);
        assert!(!completed.contains("s1"), "re-entering in_progress must clear completed marker");
    }

    #[test]
    fn update_completion_tracking_prunes_disappeared_sessions() {
        let mut dismissed: HashSet<String> = vec!["gone".to_string()].into_iter().collect();
        let mut completed: HashSet<String> = vec!["gone".to_string()].into_iter().collect();
        let mut prev: HashMap<String, bool> = vec![("gone".to_string(), true)].into_iter().collect();

        let live_sessions = vec![session("s1", true, 1_000)];
        update_completion_tracking(&live_sessions, &mut dismissed, &mut completed, &mut prev);

        assert!(!dismissed.contains("gone"), "disappeared session must be pruned from dismissed");
        assert!(!completed.contains("gone"), "disappeared session must be pruned from completed");
        assert!(!prev.contains_key("gone"), "disappeared session must be pruned from prev_in_progress");
    }

    #[test]
    fn update_completion_tracking_session_id_swap_loses_prior_completion_marker() {
        // Documents a known interaction with `dedup_sessions_by_workspace`:
        // if a Codex workspace's rollout file rotates mid-stream, the
        // post-dedup `session_id` changes.  The old id is pruned from
        // `prev_in_progress`, so the *new* id starts at "first observation"
        // and a true→false transition observed on tick 2 of the new id is
        // detected *only if both ticks land on the same post-dedup id*.
        //
        // This test pins down the current behavior so a future refactor
        // doesn't accidentally start surfacing phantom completions.
        let mut dismissed = HashSet::new();
        let mut completed = HashSet::new();
        let mut prev = HashMap::new();

        // Old rollout id sees in_progress=true.
        update_completion_tracking(&[session("old-id", true, 1_000)], &mut dismissed, &mut completed, &mut prev);
        assert_eq!(prev.get("old-id"), Some(&true));

        // Next tick: dedup swapped to a new rollout id (different session_id),
        // and the new id reports in_progress=false.
        update_completion_tracking(&[session("new-id", false, 2_000)], &mut dismissed, &mut completed, &mut prev);

        // `new-id` was never seen before — no transition can be detected.
        // `old-id` is pruned.  Completed stays empty.
        assert!(completed.is_empty(), "session-id swap must NOT spuriously mark completed");
        assert!(!prev.contains_key("old-id"), "old id must be pruned");
        assert_eq!(prev.get("new-id"), Some(&false));
    }

    // ---------- lift_dismiss_on_in_progress ----------

    #[test]
    fn lift_dismiss_only_removes_in_progress_sessions() {
        let mut dismissed: HashSet<String> = vec!["a".to_string(), "b".to_string(), "c".to_string()]
            .into_iter()
            .collect();

        let sessions = vec![
            session("a", true, 1_000),   // should be lifted
            session("b", false, 1_000),  // stays dismissed
            session("c", false, 1_000),  // stays dismissed
        ];
        lift_dismiss_on_in_progress(&sessions, &mut dismissed);

        assert!(!dismissed.contains("a"), "in_progress=true must lift dismiss");
        assert!(dismissed.contains("b"), "in_progress=false must keep dismiss");
        assert!(dismissed.contains("c"), "in_progress=false must keep dismiss");
    }

    #[test]
    fn lift_dismiss_is_noop_when_session_not_dismissed() {
        let mut dismissed: HashSet<String> = HashSet::new();
        let sessions = vec![session("a", true, 1_000)];
        lift_dismiss_on_in_progress(&sessions, &mut dismissed);
        assert!(dismissed.is_empty());
    }

    #[test]
    fn lift_dismiss_handles_unrelated_in_progress_correctly() {
        // CLAUDE.md policy: only the *same* session re-entering in_progress
        // unblocks its dismiss.  Other sessions running do not affect it.
        let mut dismissed: HashSet<String> = vec!["dismissed-one".to_string()].into_iter().collect();
        let sessions = vec![
            session("dismissed-one", false, 1_000), // still completed/idle
            session("active-other", true, 2_000),   // unrelated running session
        ];
        lift_dismiss_on_in_progress(&sessions, &mut dismissed);
        assert!(
            dismissed.contains("dismissed-one"),
            "unrelated session running must not lift this session's dismiss"
        );
    }

    // ---------- clear_stale_in_progress ----------

    #[test]
    fn clear_stale_in_progress_keeps_recent_running_sessions() {
        let s = session("s1", true, now_ms()); // freshly active
        let result = clear_stale_in_progress(vec![s], &windows_both_running());
        assert!(result[0].in_progress, "recent in_progress must remain true");
    }

    #[test]
    fn clear_stale_in_progress_forces_false_when_app_not_running() {
        let s = session("s1", true, now_ms());
        let windows = FrontWindowState {
            claude_running: false,
            codex_running: true,
            ..Default::default()
        };
        let result = clear_stale_in_progress(vec![s], &windows);
        assert!(!result[0].in_progress, "must force false when owning app is not running");
    }

    #[test]
    fn clear_stale_in_progress_forces_false_when_activity_is_stale() {
        let stale_at = now_ms().saturating_sub(4 * 60 * 1000); // 4 minutes ago
        let s = session("s1", true, stale_at);
        let result = clear_stale_in_progress(vec![s], &windows_both_running());
        assert!(!result[0].in_progress, "must force false when activity is > 3 min stale");
    }

    #[test]
    fn clear_stale_in_progress_leaves_false_sessions_alone() {
        let s = session("s1", false, 1_000); // already idle, very old
        let result = clear_stale_in_progress(vec![s], &windows_both_running());
        assert!(!result[0].in_progress);
    }

    // ---------- integration: stale-clear then update_completion_tracking ----------

    #[test]
    fn first_tick_stale_session_does_not_become_runtime_completed() {
        // Scenario: Pet Companion starts up.  The very first observation of
        // a session is *already* stale (Pet Companion was launched after the
        // session had been idle for >3 minutes).  The B-medium rule forces
        // `in_progress=false`.  `update_completion_tracking` must NOT mark
        // this as a runtime completion — we never witnessed the true phase.
        let stale = session("s1", true, now_ms().saturating_sub(5 * 60 * 1000));
        let after_stale = clear_stale_in_progress(vec![stale], &windows_both_running());
        assert!(!after_stale[0].in_progress, "stale clear should have forced false");

        let mut dismissed = HashSet::new();
        let mut completed = HashSet::new();
        let mut prev = HashMap::new();
        update_completion_tracking(&after_stale, &mut dismissed, &mut completed, &mut prev);

        assert!(
            completed.is_empty(),
            "first-tick stale-cleared session must not be marked completed"
        );
        // prev is now `false`, which is correct — we have no evidence of a
        // running state, so a future false→false continuation does nothing.
        assert_eq!(prev.get("s1"), Some(&false));
    }

    #[test]
    fn observed_running_then_stale_clear_marks_completed() {
        // Scenario: We saw the session as running (tick 1).  On tick 2 the
        // session is now stale → B-medium forces `in_progress=false` →
        // `update_completion_tracking` MUST detect the transition and
        // surface a "완료" card.  This is the path that keeps the card
        // visible after the assistant goes idle.
        let mut dismissed = HashSet::new();
        let mut completed = HashSet::new();
        let mut prev = HashMap::new();

        // Tick 1: running, fresh.
        let t1 = session("s1", true, now_ms());
        let t1_post = clear_stale_in_progress(vec![t1], &windows_both_running());
        update_completion_tracking(&t1_post, &mut dismissed, &mut completed, &mut prev);
        assert_eq!(prev.get("s1"), Some(&true));
        assert!(completed.is_empty());

        // Tick 2: still flagged in_progress=true on disk, but
        // last_activity_at is now 4 minutes old → stale-clear forces false.
        let t2 = session("s1", true, now_ms().saturating_sub(4 * 60 * 1000));
        let t2_post = clear_stale_in_progress(vec![t2], &windows_both_running());
        assert!(!t2_post[0].in_progress);
        update_completion_tracking(&t2_post, &mut dismissed, &mut completed, &mut prev);

        assert!(
            completed.contains("s1"),
            "transition (running tick → stale-cleared tick) must mark completed"
        );
    }

    #[test]
    fn observed_running_then_app_quit_marks_completed() {
        // Variant of the above: Codex Desktop is force-quit mid-turn.
        // App-not-running force-clears in_progress.  The transition must
        // still flow into completed_during_runtime so the card stays visible.
        let mut dismissed = HashSet::new();
        let mut completed = HashSet::new();
        let mut prev = HashMap::new();

        let running_now = session("s1", true, now_ms());
        let t1 = clear_stale_in_progress(vec![running_now.clone()], &windows_both_running());
        update_completion_tracking(&t1, &mut dismissed, &mut completed, &mut prev);

        // App quit.
        let windows_app_down = FrontWindowState {
            claude_running: false,
            codex_running: true,
            ..Default::default()
        };
        let t2 = clear_stale_in_progress(vec![running_now], &windows_app_down);
        assert!(!t2[0].in_progress);
        update_completion_tracking(&t2, &mut dismissed, &mut completed, &mut prev);

        assert!(completed.contains("s1"));
    }

    // ---------- notifications ----------

    #[test]
    fn collect_notification_transitions_waiting_notifies_once() {
        let tracked = SessionSummary {
            app_kind: SessionApp::Claude,
            cli_session_id: None,
            completed_preview: None,
            completed_turns: None,
            cwd: "/tmp/my-project".to_string(),
            in_progress: true,
            is_archived: false,
            last_activity_at: now_ms().saturating_sub(31_000),
            session_id: "s1".to_string(),
            title: "Need input".to_string(),
            user_preview: None,
            assistant_preview: None,
        };
        let payload = notification_payload(
            Some(tracked.clone()),
            vec![tracked],
            PetAnimationState::Waiting,
            &[],
        );
        let mut last_notified = HashMap::new();
        let previously_seen: HashSet<String> = vec!["s1".to_string()].into_iter().collect();

        let first = collect_notification_transitions(
            &payload,
            &previously_seen,
            &mut last_notified,
            now_ms(),
        );
        assert_eq!(first.len(), 1, "waiting transition should notify once");
        assert_eq!(first[0].session_id, "s1");
        assert_eq!(first[0].state, PetAnimationState::Waiting);
        assert_eq!(first[0].subtitle, "my-project · Claude");
        assert_eq!(first[0].body, "입력이 필요합니다");
        assert_eq!(last_notified.get("s1").map(String::as_str), Some("waiting"));

        let second = collect_notification_transitions(
            &payload,
            &previously_seen,
            &mut last_notified,
            now_ms(),
        );
        assert!(
            second.is_empty(),
            "same waiting state on later ticks must not re-notify"
        );
    }

    #[test]
    fn collect_notification_transitions_first_observation_seeds_without_notification() {
        let tracked = SessionSummary {
            app_kind: SessionApp::Codex,
            cli_session_id: None,
            completed_preview: None,
            completed_turns: None,
            cwd: "/tmp/workspace-alpha".to_string(),
            in_progress: true,
            is_archived: false,
            last_activity_at: now_ms().saturating_sub(31_000),
            session_id: "s1".to_string(),
            title: "Need input".to_string(),
            user_preview: None,
            assistant_preview: None,
        };
        let payload = notification_payload(
            Some(tracked.clone()),
            vec![tracked],
            PetAnimationState::Waiting,
            &[],
        );
        let mut last_notified = HashMap::new();
        let previously_seen: HashSet<String> = HashSet::new();

        let notifications = collect_notification_transitions(
            &payload,
            &previously_seen,
            &mut last_notified,
            now_ms(),
        );

        assert!(
            notifications.is_empty(),
            "first observation should seed state without startup spam"
        );
        assert_eq!(last_notified.get("s1").map(String::as_str), Some("waiting"));
    }

    #[test]
    fn collect_notification_transitions_running_clears_and_reenables_waving() {
        let completed = SessionSummary {
            app_kind: SessionApp::Claude,
            cli_session_id: None,
            completed_preview: Some("Done".to_string()),
            completed_turns: Some(1),
            cwd: "/tmp/my-project".to_string(),
            in_progress: false,
            is_archived: false,
            last_activity_at: now_ms(),
            session_id: "s1".to_string(),
            title: "Done".to_string(),
            user_preview: None,
            assistant_preview: Some("Done".to_string()),
        };
        let completed_payload = notification_payload(
            Some(completed.clone()),
            vec![completed.clone()],
            PetAnimationState::Waving,
            &["s1"],
        );
        let mut last_notified = HashMap::new();
        let previously_seen: HashSet<String> = vec!["s1".to_string()].into_iter().collect();

        let first = collect_notification_transitions(
            &completed_payload,
            &previously_seen,
            &mut last_notified,
            now_ms(),
        );
        assert_eq!(first.len(), 1, "waving transition should notify");
        assert_eq!(first[0].state, PetAnimationState::Waving);
        assert_eq!(first[0].body, "작업이 완료됐습니다");
        assert_eq!(last_notified.get("s1").map(String::as_str), Some("waving"));

        let running = SessionSummary {
            in_progress: true,
            last_activity_at: now_ms(),
            completed_preview: None,
            completed_turns: Some(1),
            user_preview: Some("New task".to_string()),
            assistant_preview: None,
            ..completed.clone()
        };
        let running_payload = notification_payload(
            Some(running.clone()),
            vec![running],
            PetAnimationState::Running,
            &[],
        );

        let during_running = collect_notification_transitions(
            &running_payload,
            &previously_seen,
            &mut last_notified,
            now_ms(),
        );
        assert!(during_running.is_empty(), "running state itself must not notify");
        assert!(
            !last_notified.contains_key("s1"),
            "running should clear prior notified state so the next completion can notify again"
        );

        let second = collect_notification_transitions(
            &completed_payload,
            &previously_seen,
            &mut last_notified,
            now_ms(),
        );
        assert_eq!(
            second.len(),
            1,
            "after running clears the marker, the next waving transition must notify again"
        );
    }
}
