mod clipboard_history;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::{
    env, fs,
    os::windows::ffi::OsStrExt,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, LogicalSize, Manager, PhysicalPosition, Position, Size, WebviewWindow,
};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, WAIT_ABANDONED_0, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};
use windows::Win32::{
    Foundation::{POINT, RECT, RPC_E_CHANGED_MODE},
    Media::Audio::Endpoints::IAudioMeterInformation,
    Media::Audio::{
        eCommunications, eConsole, eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
    },
    System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
    },
    UI::{
        Input::KeyboardAndMouse::{
            keybd_event, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_MEDIA_NEXT_TRACK,
            VK_MEDIA_PLAY_PAUSE, VK_MEDIA_PREV_TRACK,
        },
        WindowsAndMessaging::{GetCursorPos, GetWindowRect},
    },
};

const WINDOW_LABEL: &str = "main";
const STAGE_WINDOW_WIDTH: f64 = 820.0;
const STAGE_WINDOW_HEIGHT: f64 = 460.0;
const DEFAULT_MARGIN_Y: f64 = 12.0;
const DEFAULT_SCALE: f64 = 1.0;
const COLLAPSED_ISLAND_WIDTH: f64 = 360.0;
const COLLAPSED_ISLAND_HEIGHT: f64 = 58.0;
const EXPANDED_ISLAND_WIDTH: f64 = 560.0;
const DEFAULT_EXPANDED_ISLAND_HEIGHT: f64 = 306.0;
const EXPANDED_ISLAND_HEIGHT_RANGE: f64 = 240.0;
const EXPANDED_RADIUS: f64 = 30.0;
const STAGE_WINDOW_PADDING_Y: f64 = 24.0;
const TUCKED_VISIBLE_EDGE_HEIGHT: f64 = 10.0;
const STARTUP_REGISTRY_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const STARTUP_REGISTRY_VALUE: &str = "FocuSD Island";
const AUDIO_ACTIVE_THRESHOLD: f32 = 0.000015;
const CREATE_NO_WINDOW: u32 = 0x08000000;
const AGENT_STATUS_FILE_NAME: &str = "agent-status.json";
const ISLAND_POSITION_FILE_NAME: &str = "island-position.json";
const CODEX_RUNNING_MARKER_FILE_NAME: &str = "agent-codex-running.flag";
const CODEX_RUNNING_HOLD_FILE_NAME: &str = "agent-codex-running-hold.flag";
const CLAUDE_CODE_RUNNING_MARKER_FILE_NAME: &str = "agent-claudeCode-running.flag";
const CLAUDE_CODE_RUNNING_HOLD_FILE_NAME: &str = "agent-claudeCode-running-hold.flag";
const AGENT_RUNNING_MARKER_PREFIX: &str = "agent-running-";
const AGENT_HOLD_MARKER_PREFIX: &str = "agent-hold-";
const AGENT_RUNNING_SCRIPT_FILE_NAME: &str = "focusd-agent-running.cmd";
const AGENT_HOOK_SCRIPT_FILE_NAME: &str = "focusd-agent-hook.cmd";
const AGENT_STATUS_SCRIPT_FILE_NAME: &str = "focusd-agent-status.ps1";
const AGENT_COMPLETED_MAX_RETENTION_MS: i64 = 24 * 60 * 60 * 1000;

struct AgentStatusMutexGuard(windows::Win32::Foundation::HANDLE);

impl Drop for AgentStatusMutexGuard {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.0).ok();
            CloseHandle(self.0).ok();
        }
    }
}

fn lock_agent_status_mutex() -> Result<AgentStatusMutexGuard, String> {
    let mutex_name = std::ffi::OsStr::new("FocuSD.AgentStatus")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let handle = unsafe { CreateMutexW(None, false, PCWSTR(mutex_name.as_ptr())) }
        .map_err(|error| format!("Failed to create agent status mutex: {error}"))?;
    let wait_result = unsafe { WaitForSingleObject(handle, 5_000) };
    if wait_result == WAIT_OBJECT_0 || wait_result == WAIT_ABANDONED_0 {
        return Ok(AgentStatusMutexGuard(handle));
    }

    unsafe {
        CloseHandle(handle).ok();
    }
    Err("Timed out waiting for the FocuSD agent status mutex.".to_string())
}
const FOCUSD_AGENT_HOOK_BLOCK_BEGIN: &str = "# BEGIN FocuSD Agent Status Hooks";
const FOCUSD_AGENT_HOOK_BLOCK_END: &str = "# END FocuSD Agent Status Hooks";
// Codex 配置使用注释保存 Hook 版本，启动时可据此把旧版内联命令升级到短 cmd 入口。
const FOCUSD_AGENT_HOOK_VERSION_MARKER: &str = "# FocuSD Agent Status Hooks Version: 2";
const FOCUSD_AGENT_HOOK_SIGNATURE: &str = "focusd-agent-";
const AGENT_RUNNING_SCRIPT: &str = include_str!("../../scripts/focusd-agent-running.cmd");
const AGENT_HOOK_SCRIPT: &str = include_str!("../../scripts/focusd-agent-hook.cmd");
const AGENT_STATUS_SCRIPT: &str = include_str!("../../scripts/focusd-agent-status.ps1");

static WINDOW_STATE: OnceLock<Mutex<IslandWindowState>> = OnceLock::new();

#[derive(Clone, Copy)]
enum IslandMode {
    Collapsed,
    Expanded,
}

impl IslandMode {
    fn from_value(value: &str) -> Result<Self, String> {
        match value {
            "collapsed" => Ok(Self::Collapsed),
            "expanded" => Ok(Self::Expanded),
            _ => Err(format!("Unsupported island mode: {value}")),
        }
    }

    fn base_size(self, expanded_height: f64) -> (f64, f64) {
        match self {
            Self::Collapsed => (COLLAPSED_ISLAND_WIDTH, COLLAPSED_ISLAND_HEIGHT),
            Self::Expanded => (EXPANDED_ISLAND_WIDTH, expanded_height),
        }
    }

    fn corner_radius(self) -> f64 {
        match self {
            Self::Collapsed => COLLAPSED_ISLAND_HEIGHT / 2.0,
            Self::Expanded => EXPANDED_RADIUS,
        }
    }
}

#[derive(Clone, Copy)]
struct IslandWindowState {
    mode: IslandMode,
    is_tucked: bool,
    size_scale: f64,
    margin_y: f64,
    expanded_height: f64,
    use_free_position: bool,
    free_x: i32,
    free_y: i32,
}

impl Default for IslandWindowState {
    fn default() -> Self {
        Self {
            mode: IslandMode::Collapsed,
            is_tucked: false,
            size_scale: DEFAULT_SCALE,
            margin_y: DEFAULT_MARGIN_Y,
            expanded_height: DEFAULT_EXPANDED_ISLAND_HEIGHT,
            use_free_position: false,
            free_x: 0,
            free_y: 0,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistedIslandPosition {
    #[serde(default)]
    use_free_position: bool,
    #[serde(default)]
    x: i32,
    #[serde(default)]
    y: i32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct IslandPositionSnapshot {
    use_free_position: bool,
    x: i32,
    y: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IslandLayout {
    size_scale: f64,
    margin_y: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SaveTodoMarkdownResult {
    file_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MediaState {
    available: bool,
    audio_active: bool,
    audio_peak: f32,
    playback_status: String,
    updated_at: i64,
}

impl Default for MediaState {
    fn default() -> Self {
        Self {
            available: false,
            audio_active: false,
            audio_peak: 0.0,
            playback_status: "unavailable".to_string(),
            updated_at: current_unix_millis(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentInstance {
    #[serde(default)]
    id: String,
    #[serde(default = "default_agent_provider")]
    provider: String,
    #[serde(default = "default_display_index")]
    display_index: u32,
    #[serde(default = "default_agent_phase")]
    phase: String,
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    updated_at: i64,
}

impl Default for AgentInstance {
    fn default() -> Self {
        Self {
            id: String::new(),
            provider: default_agent_provider(),
            display_index: default_display_index(),
            phase: default_agent_phase(),
            task_id: None,
            updated_at: 0,
            display_name: None,
        }
    }
}

#[derive(Default)]
struct CodexThreadNameCache {
    modified_at: Option<SystemTime>,
    names: std::collections::HashMap<String, String>,
}

static CODEX_THREAD_NAME_CACHE: OnceLock<Mutex<CodexThreadNameCache>> = OnceLock::new();
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentTaskStatus {
    #[serde(default = "default_agent_phase")]
    phase: String,
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    updated_at: i64,
}

impl Default for AgentTaskStatus {
    fn default() -> Self {
        Self {
            phase: default_agent_phase(),
            task_id: None,
            updated_at: 0,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistedAgentStatus {
    #[serde(default)]
    instances: Vec<AgentInstance>,
    /// Legacy single-provider fields kept for backward-compatible reads.
    #[serde(default)]
    codex: Option<AgentTaskStatus>,
    #[serde(default)]
    claude_code: Option<AgentTaskStatus>,
    #[serde(default)]
    updated_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentStatusSnapshot {
    instances: Vec<AgentInstance>,
    updated_at: i64,
    status_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentHooksInstallResult {
    scripts_dir: String,
    status_path: String,
    codex_config_path: String,
    claude_config_path: String,
    installed_at: i64,
}

fn default_agent_phase() -> String {
    "idle".to_string()
}

fn default_agent_provider() -> String {
    "codex".to_string()
}

fn default_display_index() -> u32 {
    1
}

#[tauri::command]
fn set_island_layout(app: AppHandle, layout: IslandLayout) -> Result<(), String> {
    let window = main_window(&app)?;
    let state = mutate_window_state(|state| {
        state.size_scale = layout.size_scale.clamp(0.75, 1.4);
        state.margin_y = layout.margin_y.clamp(0.0, 160.0);
        *state
    });
    apply_stage_geometry(&window, state)
}

#[tauri::command]
fn set_island_interaction(
    app: AppHandle,
    mode: String,
    size_scale: f64,
    margin_y: Option<f64>,
    expanded_height: Option<f64>,
    is_tucked: Option<bool>,
) -> Result<(), String> {
    let window = main_window(&app)?;
    let mode = IslandMode::from_value(&mode)?;
    let state = mutate_window_state(|state| {
        state.mode = mode;
        state.is_tucked = is_tucked.unwrap_or(false);
        state.size_scale = size_scale.clamp(0.75, 1.4);
        if let Some(margin_y) = margin_y {
            state.margin_y = margin_y.clamp(0.0, 160.0);
        }
        if let Some(expanded_height) = expanded_height {
            state.expanded_height = expanded_height.clamp(
                DEFAULT_EXPANDED_ISLAND_HEIGHT,
                DEFAULT_EXPANDED_ISLAND_HEIGHT + EXPANDED_ISLAND_HEIGHT_RANGE,
            );
        }
        *state
    });
    apply_stage_geometry(&window, state)
}

#[tauri::command]
fn set_island_free_position(
    app: AppHandle,
    x: i32,
    y: i32,
) -> Result<IslandPositionSnapshot, String> {
    let window = main_window(&app)?;
    let state = mutate_window_state(|state| {
        state.use_free_position = true;
        state.free_x = x;
        state.free_y = y;
        *state
    });
    persist_island_position(&app, &state)?;
    apply_stage_geometry(&window, state)?;
    Ok(island_position_snapshot(state))
}

#[tauri::command]
fn capture_island_free_position(app: AppHandle) -> Result<IslandPositionSnapshot, String> {
    let window = main_window(&app)?;
    let position = window
        .outer_position()
        .map_err(|error| format!("Failed to read island position: {error}"))?;
    set_island_free_position(app, position.x, position.y)
}

#[tauri::command]
fn reset_island_position(app: AppHandle) -> Result<IslandPositionSnapshot, String> {
    let window = main_window(&app)?;
    let state = mutate_window_state(|state| {
        state.use_free_position = false;
        state.free_x = 0;
        state.free_y = 0;
        *state
    });
    persist_island_position(&app, &state)?;
    apply_stage_geometry(&window, state)?;
    Ok(island_position_snapshot(state))
}

#[tauri::command]
fn get_island_position(app: AppHandle) -> Result<IslandPositionSnapshot, String> {
    let state = read_window_state();
    let _ = app;
    Ok(island_position_snapshot(state))
}

#[tauri::command]
fn minimize_island(app: AppHandle) -> Result<(), String> {
    hide_island(&app);
    Ok(())
}

#[tauri::command]
fn show_ready_island(app: AppHandle) -> Result<(), String> {
    show_island(&app)
}

#[tauri::command]
fn get_launch_at_startup() -> Result<bool, String> {
    let mut command = Command::new("reg");
    let status = command
        .creation_flags(CREATE_NO_WINDOW)
        .args(["query", STARTUP_REGISTRY_KEY, "/v", STARTUP_REGISTRY_VALUE])
        .status()
        .map_err(|error| format!("Failed to query startup registry: {error}"))?;

    Ok(status.success())
}

#[tauri::command]
fn set_launch_at_startup(enabled: bool) -> Result<(), String> {
    let status = if enabled {
        let current_exe = std::env::current_exe()
            .map_err(|error| format!("Failed to resolve current executable: {error}"))?;
        let startup_value = format!("\"{}\"", current_exe.display());

        let mut command = Command::new("reg");
        command
            .creation_flags(CREATE_NO_WINDOW)
            .args([
                "add",
                STARTUP_REGISTRY_KEY,
                "/v",
                STARTUP_REGISTRY_VALUE,
                "/t",
                "REG_SZ",
                "/d",
            ])
            .arg(startup_value)
            .arg("/f")
            .status()
    } else {
        let mut command = Command::new("reg");
        command
            .creation_flags(CREATE_NO_WINDOW)
            .args([
                "delete",
                STARTUP_REGISTRY_KEY,
                "/v",
                STARTUP_REGISTRY_VALUE,
                "/f",
            ])
            .status()
    }
    .map_err(|error| format!("Failed to update startup registry: {error}"))?;

    if status.success() {
        Ok(())
    } else {
        Err("Startup registry command failed.".to_string())
    }
}

#[tauri::command]
fn save_todo_markdown(
    directory: String,
    date: String,
    content: String,
) -> Result<SaveTodoMarkdownResult, String> {
    let directory = directory.trim();

    if directory.is_empty() {
        return Err("Todo save path is empty.".to_string());
    }

    if !date.chars().all(|ch| ch.is_ascii_digit() || ch == '-') {
        return Err("Todo date contains invalid filename characters.".to_string());
    }

    let directory_path = PathBuf::from(directory);
    fs::create_dir_all(&directory_path).map_err(|error| error.to_string())?;

    let file_path = directory_path.join(format!("{date}.md"));
    fs::write(&file_path, content).map_err(|error| error.to_string())?;

    Ok(SaveTodoMarkdownResult {
        file_path: file_path.to_string_lossy().to_string(),
    })
}

#[tauri::command]
fn get_default_todo_save_directory() -> Result<String, String> {
    let home_dir = windows_home_dir()?;
    Ok(home_dir
        .join("Documents")
        .join("FocuSD")
        .to_string_lossy()
        .to_string())
}

#[tauri::command]
fn get_agent_status(app: AppHandle) -> Result<AgentStatusSnapshot, String> {
    let app_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Failed to resolve app data directory: {error}"))?;
    fs::create_dir_all(&app_dir)
        .map_err(|error| format!("Failed to create app data directory: {error}"))?;

    let status_path = app_dir.join(AGENT_STATUS_FILE_NAME);
    let status_path_display = status_path.to_string_lossy().to_string();
    let mut snapshot = match fs::read_to_string(&status_path) {
        Ok(content) => match serde_json::from_str::<PersistedAgentStatus>(&content) {
            Ok(persisted) => agent_status_snapshot_from_persisted(persisted, status_path_display),
            Err(_) => default_agent_status_snapshot(status_path_display),
        },
        Err(_) => default_agent_status_snapshot(status_path_display),
    };
    apply_agent_running_markers(&app_dir, &mut snapshot);
    apply_codex_thread_names(&mut snapshot);

    Ok(snapshot)
}

#[tauri::command]
fn clear_agent_status(
    app: AppHandle,
    provider: Option<String>,
    instance_id: Option<String>,
) -> Result<AgentStatusSnapshot, String> {
    let app_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Failed to resolve app data directory: {error}"))?;
    fs::create_dir_all(&app_dir)
        .map_err(|error| format!("Failed to create app data directory: {error}"))?;

    let status_path = app_dir.join(AGENT_STATUS_FILE_NAME);
    let status_path_display = status_path.to_string_lossy().to_string();
    let _status_mutex = lock_agent_status_mutex()?;
    let mut persisted = match fs::read_to_string(&status_path) {
        Ok(content) => serde_json::from_str::<PersistedAgentStatus>(&content)
            .unwrap_or_else(|_| default_persisted_agent_status()),
        Err(_) => default_persisted_agent_status(),
    };

    let now = current_unix_millis();
    let provider = provider.unwrap_or_default();
    let instance_id = instance_id.unwrap_or_default();

    if !instance_id.is_empty() {
        let target_provider = if provider.is_empty() {
            None
        } else {
            Some(provider.as_str())
        };
        persisted.instances.retain(|instance| {
            let same_id = instance.id == instance_id;
            let same_provider = target_provider
                .map(|value| instance.provider == value)
                .unwrap_or(true);
            !(same_id && same_provider)
        });
        remove_instance_marker_files(&app_dir, &provider, &instance_id);
        if provider == "codex" || provider.is_empty() {
            if instance_id == "legacy" || instance_id == "legacy-codex" {
                remove_agent_marker_files(
                    &app_dir,
                    CODEX_RUNNING_MARKER_FILE_NAME,
                    CODEX_RUNNING_HOLD_FILE_NAME,
                );
            }
        }
        if provider == "claudeCode" || provider.is_empty() {
            if instance_id == "legacy" || instance_id == "legacy-claudeCode" {
                remove_agent_marker_files(
                    &app_dir,
                    CLAUDE_CODE_RUNNING_MARKER_FILE_NAME,
                    CLAUDE_CODE_RUNNING_HOLD_FILE_NAME,
                );
            }
        }
    } else {
        match provider.as_str() {
            "codex" | "claudeCode" => {
                persisted
                    .instances
                    .retain(|instance| instance.provider != provider);
                remove_provider_marker_files(&app_dir, &provider);
                if provider == "codex" {
                    remove_agent_marker_files(
                        &app_dir,
                        CODEX_RUNNING_MARKER_FILE_NAME,
                        CODEX_RUNNING_HOLD_FILE_NAME,
                    );
                } else {
                    remove_agent_marker_files(
                        &app_dir,
                        CLAUDE_CODE_RUNNING_MARKER_FILE_NAME,
                        CLAUDE_CODE_RUNNING_HOLD_FILE_NAME,
                    );
                }
            }
            "" => {
                persisted.instances.clear();
                remove_all_agent_marker_files(&app_dir);
            }
            _ => {
                return Err("Unsupported agent provider.".to_string());
            }
        }
    }

    persisted.codex = None;
    persisted.claude_code = None;
    persisted.updated_at = now;
    let content = serde_json::to_string_pretty(&persisted)
        .map_err(|error| format!("Failed to serialize agent status: {error}"))?;
    write_text_file(&status_path, &content)?;

    let mut snapshot = agent_status_snapshot_from_persisted(persisted, status_path_display);
    apply_agent_running_markers(&app_dir, &mut snapshot);
    apply_codex_thread_names(&mut snapshot);
    Ok(snapshot)
}

#[tauri::command]
fn install_agent_status_hooks(app: AppHandle) -> Result<AgentHooksInstallResult, String> {
    let app_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Failed to resolve app data directory: {error}"))?;
    fs::create_dir_all(&app_dir)
        .map_err(|error| format!("Failed to create app data directory: {error}"))?;

    install_agent_hook_scripts(&app_dir)?;

    let hook_script_path = app_dir.join(AGENT_HOOK_SCRIPT_FILE_NAME);
    let home_dir = windows_home_dir()?;
    let codex_config_path = home_dir.join(".codex").join("config.toml");
    let claude_config_path = home_dir.join(".claude").join("settings.json");

    install_codex_status_hooks(&codex_config_path, &hook_script_path)?;
    install_claude_code_status_hooks(&claude_config_path, &hook_script_path)?;

    Ok(AgentHooksInstallResult {
        scripts_dir: app_dir.to_string_lossy().to_string(),
        status_path: app_dir
            .join(AGENT_STATUS_FILE_NAME)
            .to_string_lossy()
            .to_string(),
        codex_config_path: codex_config_path.to_string_lossy().to_string(),
        claude_config_path: claude_config_path.to_string_lossy().to_string(),
        installed_at: current_unix_millis(),
    })
}

fn managed_codex_hook_has_current_entry(content: &str) -> bool {
    // Codex 使用独立的 TOML 管理块。版本标记可以确保旧版内联 PowerShell
    // 或未带版本标记的过渡版本在新程序启动时被确定性地重写。
    content.contains(FOCUSD_AGENT_HOOK_BLOCK_BEGIN)
        && content.contains(FOCUSD_AGENT_HOOK_VERSION_MARKER)
        && content.contains(AGENT_HOOK_SCRIPT_FILE_NAME)
        && content.contains("cmd.exe /d /s /c")
}

fn managed_claude_hook_has_current_entry(content: &str) -> bool {
    // Claude settings.json 会把 command 和 args 分成不同字段，因此按整个
    // JSON 文本检查短脚本、FocuSD 标记和 cmd.exe 启动器，不要求它们同一行。
    content.contains(AGENT_HOOK_SCRIPT_FILE_NAME)
        && content.contains(FOCUSD_AGENT_HOOK_SIGNATURE)
        && content.contains("\"command\": \"cmd.exe\"")
}
fn refresh_agent_status_hooks_if_installed(app: AppHandle) -> Result<(), String> {
    let app_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Failed to resolve app data directory: {error}"))?;
    let home_dir = windows_home_dir()?;
    let codex_config_path = home_dir.join(".codex").join("config.toml");
    let claude_config_path = home_dir.join(".claude").join("settings.json");

    let codex_content = fs::read_to_string(&codex_config_path).unwrap_or_default();
    let claude_content = fs::read_to_string(&claude_config_path).unwrap_or_default();
    let codex_installed = codex_content.contains(FOCUSD_AGENT_HOOK_BLOCK_BEGIN)
        || codex_content.contains(FOCUSD_AGENT_HOOK_SIGNATURE);
    let claude_installed = claude_content.contains(FOCUSD_AGENT_HOOK_SIGNATURE);

    if !codex_installed && !claude_installed {
        return Ok(());
    }

    fs::create_dir_all(&app_dir)
        .map_err(|error| format!("Failed to create app data directory: {error}"))?;

    let expected_running_script = normalize_windows_line_endings(AGENT_RUNNING_SCRIPT);
    let expected_hook_script = normalize_windows_line_endings(AGENT_HOOK_SCRIPT);
    let expected_status_script = normalize_windows_line_endings(AGENT_STATUS_SCRIPT);
    let running_script_outdated = fs::read_to_string(app_dir.join(AGENT_RUNNING_SCRIPT_FILE_NAME))
        .map(|content| content != expected_running_script)
        .unwrap_or(true);
    let hook_script_outdated = fs::read_to_string(app_dir.join(AGENT_HOOK_SCRIPT_FILE_NAME))
        .map(|content| content != expected_hook_script)
        .unwrap_or(true);
    let status_script_outdated = fs::read_to_string(app_dir.join(AGENT_STATUS_SCRIPT_FILE_NAME))
        .map(|content| content != expected_status_script)
        .unwrap_or(true);

    if running_script_outdated || hook_script_outdated || status_script_outdated {
        install_agent_hook_scripts(&app_dir)?;
    }

    let hook_script_path = app_dir.join(AGENT_HOOK_SCRIPT_FILE_NAME);
    let codex_uses_legacy_command =
        codex_installed && codex_content.contains(AGENT_RUNNING_SCRIPT_FILE_NAME);
    let claude_uses_legacy_command =
        claude_installed && claude_content.contains(AGENT_RUNNING_SCRIPT_FILE_NAME);
    let codex_needs_hook_upgrade = codex_installed
        && (codex_uses_legacy_command || !managed_codex_hook_has_current_entry(&codex_content));
    let claude_needs_hook_upgrade = claude_installed
        && (claude_uses_legacy_command || !managed_claude_hook_has_current_entry(&claude_content));
    if codex_needs_hook_upgrade {
        remove_provider_marker_files(&app_dir, "codex");
        remove_agent_marker_files(
            &app_dir,
            CODEX_RUNNING_MARKER_FILE_NAME,
            CODEX_RUNNING_HOLD_FILE_NAME,
        );
        install_codex_status_hooks(&codex_config_path, &hook_script_path)?;
    }
    if claude_needs_hook_upgrade {
        remove_provider_marker_files(&app_dir, "claudeCode");
        remove_agent_marker_files(
            &app_dir,
            CLAUDE_CODE_RUNNING_MARKER_FILE_NAME,
            CLAUDE_CODE_RUNNING_HOLD_FILE_NAME,
        );
        install_claude_code_status_hooks(&claude_config_path, &hook_script_path)?;
    }

    Ok(())
}

#[tauri::command]
fn get_media_state() -> MediaState {
    read_media_state()
}

#[tauri::command]
fn get_audio_level() -> AudioLevel {
    let peak = read_system_audio_peak_window(1, Duration::ZERO).unwrap_or(0.0);

    AudioLevel {
        active: peak > AUDIO_ACTIVE_THRESHOLD,
        peak,
        updated_at: current_unix_millis(),
    }
}

#[tauri::command]
fn media_play_pause() {
    send_media_key(VK_MEDIA_PLAY_PAUSE);
}

#[tauri::command]
fn media_next() {
    send_media_key(VK_MEDIA_NEXT_TRACK);
}

#[tauri::command]
fn media_previous() {
    send_media_key(VK_MEDIA_PREV_TRACK);
}

fn read_media_state() -> MediaState {
    let audio_peak = read_system_audio_peak_window(3, Duration::from_millis(6)).unwrap_or(0.0);
    let audio_active = audio_peak > AUDIO_ACTIVE_THRESHOLD;

    MediaState {
        available: audio_active,
        audio_active,
        audio_peak,
        playback_status: if audio_active {
            "playing"
        } else {
            "unavailable"
        }
        .to_string(),
        updated_at: current_unix_millis(),
    }
}

fn default_agent_status_snapshot(status_path: String) -> AgentStatusSnapshot {
    AgentStatusSnapshot {
        instances: Vec::new(),
        updated_at: current_unix_millis(),
        status_path,
    }
}

fn default_persisted_agent_status() -> PersistedAgentStatus {
    PersistedAgentStatus {
        instances: Vec::new(),
        codex: None,
        claude_code: None,
        updated_at: current_unix_millis(),
    }
}

fn agent_status_snapshot_from_persisted(
    persisted: PersistedAgentStatus,
    status_path: String,
) -> AgentStatusSnapshot {
    let mut instances = persisted
        .instances
        .into_iter()
        .map(normalize_agent_instance)
        .filter(|instance| !instance.id.is_empty())
        .collect::<Vec<_>>();

    if instances.is_empty() {
        if let Some(legacy) = persisted.codex {
            let normalized = normalize_agent_task_status(legacy);
            if normalized.phase != "idle" && normalized.phase != "completed" {
                instances.push(AgentInstance {
                    id: "legacy-codex".to_string(),
                    provider: "codex".to_string(),
                    display_index: 1,
                    phase: normalized.phase,
                    task_id: normalized.task_id,
                    updated_at: normalized.updated_at,
                    display_name: None,
                });
            }
        }
        if let Some(legacy) = persisted.claude_code {
            let normalized = normalize_agent_task_status(legacy);
            if normalized.phase != "idle" && normalized.phase != "completed" {
                instances.push(AgentInstance {
                    id: "legacy-claudeCode".to_string(),
                    provider: "claudeCode".to_string(),
                    display_index: 1,
                    phase: normalized.phase,
                    task_id: normalized.task_id,
                    updated_at: normalized.updated_at,
                    display_name: None,
                });
            }
        }
    }

    AgentStatusSnapshot {
        instances,
        updated_at: if persisted.updated_at > 0 {
            persisted.updated_at
        } else {
            current_unix_millis()
        },
        status_path,
    }
}

fn apply_agent_running_markers(app_dir: &Path, snapshot: &mut AgentStatusSnapshot) {
    let now = current_unix_millis();
    let mut marker_keys = std::collections::HashSet::new();

    if let Ok(entries) = fs::read_dir(app_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let file_name = match path.file_name().and_then(|name| name.to_str()) {
                Some(name) => name.to_string(),
                None => continue,
            };

            if let Some((provider, instance_id)) =
                parse_agent_marker_file_name(&file_name, AGENT_RUNNING_MARKER_PREFIX, ".flag")
            {
                marker_keys.insert(format!("{provider}:{instance_id}"));
                upsert_running_instance(
                    snapshot,
                    &provider,
                    &instance_id,
                    "running",
                    file_modified_unix_millis(&path).unwrap_or(now),
                );
                continue;
            }

            if let Some((provider, instance_id)) =
                parse_agent_marker_file_name(&file_name, AGENT_HOLD_MARKER_PREFIX, ".flag")
            {
                let visible_until = fs::read_to_string(&path)
                    .ok()
                    .and_then(|content| content.trim().parse::<i64>().ok())
                    .unwrap_or(0);
                if visible_until > now {
                    marker_keys.insert(format!("{provider}:{instance_id}"));
                    upsert_running_instance(snapshot, &provider, &instance_id, "running", now);
                } else {
                    fs::remove_file(&path).ok();
                }
            }
        }
    }

    apply_legacy_provider_marker(
        app_dir,
        snapshot,
        &mut marker_keys,
        "codex",
        "legacy-codex",
        CODEX_RUNNING_MARKER_FILE_NAME,
        CODEX_RUNNING_HOLD_FILE_NAME,
        now,
    );
    apply_legacy_provider_marker(
        app_dir,
        snapshot,
        &mut marker_keys,
        "claudeCode",
        "legacy-claudeCode",
        CLAUDE_CODE_RUNNING_MARKER_FILE_NAME,
        CLAUDE_CODE_RUNNING_HOLD_FILE_NAME,
        now,
    );

    snapshot.instances.retain(|instance| {
        let key = format!("{}:{}", instance.provider, instance.id);
        if instance.phase == "running" || instance.phase == "stale" {
            return marker_keys.contains(&key)
                || instance.phase == "stale"
                || instance.id.starts_with("legacy");
        }

        if instance.phase == "completed" {
            return instance.updated_at > 0
                && now.saturating_sub(instance.updated_at) < AGENT_COMPLETED_MAX_RETENTION_MS;
        }

        matches!(instance.phase.as_str(), "failed")
    });

    snapshot.instances.sort_by(|left, right| {
        left.provider
            .cmp(&right.provider)
            .then_with(|| left.display_index.cmp(&right.display_index))
    });
    snapshot.updated_at = now;
}

fn apply_codex_thread_names(snapshot: &mut AgentStatusSnapshot) {
    let session_ids = snapshot
        .instances
        .iter()
        .filter(|instance| instance.provider == "codex")
        .filter_map(|instance| instance.id.strip_prefix("session-"))
        .map(str::to_string)
        .collect::<std::collections::HashSet<_>>();
    if session_ids.is_empty() {
        return;
    }

    let Ok(home_dir) = windows_home_dir() else {
        return;
    };

    let index_path = home_dir.join(".codex").join("session_index.jsonl");
    let names = load_codex_thread_names(&index_path);
    for instance in &mut snapshot.instances {
        if instance.provider != "codex" {
            continue;
        }
        let Some(session_id) = instance.id.strip_prefix("session-") else {
            continue;
        };
        instance.display_name = names.get(session_id).cloned();
    }
}

fn load_codex_thread_names(index_path: &Path) -> std::collections::HashMap<String, String> {
    let modified_at = fs::metadata(index_path)
        .and_then(|metadata| metadata.modified())
        .ok();

    let cache_cell =
        CODEX_THREAD_NAME_CACHE.get_or_init(|| Mutex::new(CodexThreadNameCache::default()));
    if let Ok(cache) = cache_cell.lock() {
        if modified_at.is_some() && cache.modified_at == modified_at {
            return cache.names.clone();
        }
    }

    let names = fs::read_to_string(index_path)
        .map(|content| parse_all_codex_thread_names(&content))
        .unwrap_or_default();

    if let Ok(mut cache) = cache_cell.lock() {
        cache.modified_at = modified_at;
        cache.names = names.clone();
    }

    names
}

/// 只解析 Codex 会话索引元数据；倒序读取保证重复记录使用最新标题。
fn parse_all_codex_thread_names(content: &str) -> std::collections::HashMap<String, String> {
    let mut names = std::collections::HashMap::<String, String>::new();
    for line in content.lines().rev() {
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(session_id) = record.get("id").and_then(Value::as_str) else {
            continue;
        };
        if names.contains_key(session_id) {
            continue;
        }
        let Some(thread_name) = record.get("thread_name").and_then(Value::as_str) else {
            continue;
        };
        let thread_name = thread_name.trim();
        if thread_name.is_empty() {
            continue;
        }
        names.insert(session_id.to_string(), thread_name.to_string());
    }
    names
}
fn apply_legacy_provider_marker(
    app_dir: &Path,
    snapshot: &mut AgentStatusSnapshot,
    marker_keys: &mut std::collections::HashSet<String>,
    provider: &str,
    instance_id: &str,
    running_file_name: &str,
    hold_file_name: &str,
    now: i64,
) {
    let running_path = app_dir.join(running_file_name);
    if running_path.is_file() {
        marker_keys.insert(format!("{provider}:{instance_id}"));
        upsert_running_instance(
            snapshot,
            provider,
            instance_id,
            "running",
            file_modified_unix_millis(&running_path).unwrap_or(now),
        );
        return;
    }

    let hold_path = app_dir.join(hold_file_name);
    let visible_until = fs::read_to_string(&hold_path)
        .ok()
        .and_then(|content| content.trim().parse::<i64>().ok())
        .unwrap_or(0);
    if visible_until > now {
        marker_keys.insert(format!("{provider}:{instance_id}"));
        upsert_running_instance(snapshot, provider, instance_id, "running", now);
    } else {
        fs::remove_file(hold_path).ok();
    }
}

fn parse_agent_marker_file_name(
    file_name: &str,
    prefix: &str,
    suffix: &str,
) -> Option<(String, String)> {
    if !file_name.starts_with(prefix) || !file_name.ends_with(suffix) {
        return None;
    }

    let body = &file_name[prefix.len()..file_name.len() - suffix.len()];
    if let Some(instance_id) = body.strip_prefix("claudeCode-") {
        if instance_id.is_empty() {
            return None;
        }
        return Some(("claudeCode".to_string(), instance_id.to_string()));
    }
    if let Some(instance_id) = body.strip_prefix("codex-") {
        if instance_id.is_empty() {
            return None;
        }
        return Some(("codex".to_string(), instance_id.to_string()));
    }
    None
}

fn upsert_running_instance(
    snapshot: &mut AgentStatusSnapshot,
    provider: &str,
    instance_id: &str,
    phase: &str,
    updated_at: i64,
) {
    if let Some(existing) = snapshot
        .instances
        .iter_mut()
        .find(|instance| instance.provider == provider && instance.id == instance_id)
    {
        existing.phase = phase.to_string();
        existing.updated_at = updated_at;
        return;
    }

    let display_index = next_display_index(&snapshot.instances, provider);
    snapshot.instances.push(AgentInstance {
        id: instance_id.to_string(),
        provider: provider.to_string(),
        display_index,
        phase: phase.to_string(),
        task_id: None,
        updated_at,
        display_name: None,
    });
}

fn next_display_index(instances: &[AgentInstance], provider: &str) -> u32 {
    let mut used = instances
        .iter()
        .filter(|instance| instance.provider == provider)
        .map(|instance| instance.display_index)
        .collect::<Vec<_>>();
    used.sort_unstable();
    let mut index = 1;
    for value in used {
        if value == index {
            index += 1;
        } else if value > index {
            break;
        }
    }
    index
}

fn remove_agent_marker_files(app_dir: &Path, running_file_name: &str, hold_file_name: &str) {
    fs::remove_file(app_dir.join(running_file_name)).ok();
    fs::remove_file(app_dir.join(hold_file_name)).ok();
}

fn remove_instance_marker_files(app_dir: &Path, provider: &str, instance_id: &str) {
    if provider.is_empty() {
        for candidate in ["codex", "claudeCode"] {
            let running = format!("{AGENT_RUNNING_MARKER_PREFIX}{candidate}-{instance_id}.flag");
            let hold = format!("{AGENT_HOLD_MARKER_PREFIX}{candidate}-{instance_id}.flag");
            fs::remove_file(app_dir.join(running)).ok();
            fs::remove_file(app_dir.join(hold)).ok();
        }
        return;
    }

    let running = format!("{AGENT_RUNNING_MARKER_PREFIX}{provider}-{instance_id}.flag");
    let hold = format!("{AGENT_HOLD_MARKER_PREFIX}{provider}-{instance_id}.flag");
    fs::remove_file(app_dir.join(running)).ok();
    fs::remove_file(app_dir.join(hold)).ok();
}

fn remove_provider_marker_files(app_dir: &Path, provider: &str) {
    if let Ok(entries) = fs::read_dir(app_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            let running_prefix = format!("{AGENT_RUNNING_MARKER_PREFIX}{provider}-");
            let hold_prefix = format!("{AGENT_HOLD_MARKER_PREFIX}{provider}-");
            if (name.starts_with(&running_prefix) || name.starts_with(&hold_prefix))
                && name.ends_with(".flag")
            {
                fs::remove_file(path).ok();
            }
        }
    }
}

fn remove_all_agent_marker_files(app_dir: &Path) {
    remove_provider_marker_files(app_dir, "codex");
    remove_provider_marker_files(app_dir, "claudeCode");
    remove_agent_marker_files(
        app_dir,
        CODEX_RUNNING_MARKER_FILE_NAME,
        CODEX_RUNNING_HOLD_FILE_NAME,
    );
    remove_agent_marker_files(
        app_dir,
        CLAUDE_CODE_RUNNING_MARKER_FILE_NAME,
        CLAUDE_CODE_RUNNING_HOLD_FILE_NAME,
    );
}

fn normalize_agent_task_status(mut status: AgentTaskStatus) -> AgentTaskStatus {
    if !matches!(
        status.phase.as_str(),
        "idle" | "running" | "completed" | "failed" | "stale"
    ) {
        status.phase = default_agent_phase();
    }

    status
}

fn normalize_agent_instance(mut instance: AgentInstance) -> AgentInstance {
    if instance.provider != "codex" && instance.provider != "claudeCode" {
        instance.provider = default_agent_provider();
    }
    if instance.display_index == 0 {
        instance.display_index = 1;
    }
    if !matches!(
        instance.phase.as_str(),
        "idle" | "running" | "completed" | "failed" | "stale"
    ) {
        instance.phase = default_agent_phase();
    }
    instance
}

fn install_agent_hook_scripts(app_dir: &Path) -> Result<(), String> {
    write_text_file(
        &app_dir.join(AGENT_RUNNING_SCRIPT_FILE_NAME),
        &normalize_windows_line_endings(AGENT_RUNNING_SCRIPT),
    )?;
    write_text_file(
        &app_dir.join(AGENT_HOOK_SCRIPT_FILE_NAME),
        &normalize_windows_line_endings(AGENT_HOOK_SCRIPT),
    )?;
    write_text_file(
        &app_dir.join(AGENT_STATUS_SCRIPT_FILE_NAME),
        &normalize_windows_line_endings(AGENT_STATUS_SCRIPT),
    )
}

fn install_codex_status_hooks(config_path: &Path, hook_script_path: &Path) -> Result<(), String> {
    let content = fs::read_to_string(config_path).unwrap_or_default();
    let content = remove_managed_codex_hook_block(&content);
    let block = build_codex_hook_block(hook_script_path);
    let mut next_content = content.trim_end().to_string();
    if !next_content.is_empty() {
        next_content.push_str("\n\n");
    }
    next_content.push_str(&block);

    write_text_file(config_path, &next_content)
}

fn install_claude_code_status_hooks(
    config_path: &Path,
    hook_script_path: &Path,
) -> Result<(), String> {
    let mut config = match fs::read_to_string(config_path) {
        Ok(content) if !content.trim().is_empty() => serde_json::from_str::<Value>(&content)
            .map_err(|error| format!("Failed to parse Claude Code settings.json: {error}"))?,
        _ => json!({}),
    };

    let Some(root) = config.as_object_mut() else {
        return Err("Claude Code settings.json must contain a JSON object.".to_string());
    };

    let hooks = root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    if !hooks.is_object() {
        *hooks = Value::Object(Map::new());
    }
    let hooks = hooks
        .as_object_mut()
        .ok_or_else(|| "Failed to prepare Claude Code hooks object.".to_string())?;

    install_claude_code_hook_event(
        hooks,
        "UserPromptSubmit",
        claude_code_running_hook_entry(hook_script_path),
    );
    install_claude_code_hook_event(
        hooks,
        "PreToolUse",
        claude_code_match_all_hook_entry(claude_code_running_hook_entry(hook_script_path)),
    );
    install_claude_code_hook_event(
        hooks,
        "Stop",
        claude_code_status_hook_entry(hook_script_path, "completed"),
    );
    install_claude_code_hook_event(
        hooks,
        "StopFailure",
        claude_code_status_hook_entry(hook_script_path, "failed"),
    );

    let json = serde_json::to_string_pretty(&config)
        .map_err(|error| format!("Failed to serialize Claude Code settings.json: {error}"))?;
    write_text_file(config_path, &json)
}

fn install_claude_code_hook_event(hooks: &mut Map<String, Value>, event_name: &str, entry: Value) {
    let mut entries = hooks
        .remove(event_name)
        .and_then(|value| match value {
            Value::Array(entries) => Some(entries),
            _ => None,
        })
        .unwrap_or_default()
        .into_iter()
        .filter_map(remove_managed_claude_code_hooks)
        .collect::<Vec<_>>();

    entries.push(entry);
    hooks.insert(event_name.to_string(), Value::Array(entries));
}

fn remove_managed_claude_code_hooks(mut entry: Value) -> Option<Value> {
    let Value::Object(entry_object) = &mut entry else {
        return Some(entry);
    };

    let Some(hooks_value) = entry_object.get_mut("hooks") else {
        return Some(entry);
    };

    let Value::Array(hooks) = hooks_value else {
        return Some(entry);
    };

    hooks.retain(|hook| !value_contains_focusd_hook_signature(hook));
    if hooks.is_empty() {
        None
    } else {
        Some(entry)
    }
}

fn value_contains_focusd_hook_signature(value: &Value) -> bool {
    match value {
        Value::String(text) => text.contains(FOCUSD_AGENT_HOOK_SIGNATURE),
        Value::Array(values) => values.iter().any(value_contains_focusd_hook_signature),
        Value::Object(values) => values.values().any(value_contains_focusd_hook_signature),
        _ => false,
    }
}

fn claude_code_match_all_hook_entry(mut entry: Value) -> Value {
    if let Value::Object(object) = &mut entry {
        object.insert("matcher".to_string(), Value::String("*".to_string()));
    }

    entry
}

fn claude_code_running_hook_entry(script_path: &Path) -> Value {
    claude_code_status_hook_entry(script_path, "running")
}

fn claude_code_status_hook_entry(script_path: &Path, phase: &str) -> Value {
    claude_code_hook_entry(
        "cmd.exe",
        vec![
            "/d".to_string(),
            "/s".to_string(),
            "/c".to_string(),
            agent_hook_command_argument(script_path, "claudeCode", phase),
        ],
        5,
    )
}

fn claude_code_hook_entry(command: &str, args: Vec<String>, timeout: i64) -> Value {
    json!({
        "hooks": [
            {
                "type": "command",
                "command": command,
                "args": args,
                "timeout": timeout
            }
        ]
    })
}

fn build_codex_hook_block(hook_script_path: &Path) -> String {
    let submit_command = agent_hook_command(hook_script_path, "codex", "running");
    let stop_command = agent_hook_command(hook_script_path, "codex", "completed");

    format!(
        r#"{begin}
{version_marker}

[[hooks.UserPromptSubmit]]
[[hooks.UserPromptSubmit.hooks]]
type = "command"
command = {submit_command}
command_windows = {submit_command}
timeout = 5
statusMessage = "Updating FocuSD agent status"

[[hooks.Stop]]
[[hooks.Stop.hooks]]
type = "command"
command = {stop_command}
command_windows = {stop_command}
timeout = 5
statusMessage = "Updating FocuSD agent status"

{end}"#,
        begin = FOCUSD_AGENT_HOOK_BLOCK_BEGIN,
        end = FOCUSD_AGENT_HOOK_BLOCK_END,
        version_marker = FOCUSD_AGENT_HOOK_VERSION_MARKER,
        submit_command = toml_basic_string(&submit_command),
        stop_command = toml_basic_string(&stop_command),
    )
}

fn remove_managed_codex_hook_block(content: &str) -> String {
    let mut remaining = content;
    let mut next_content = String::new();

    while let Some(start) = remaining.find(FOCUSD_AGENT_HOOK_BLOCK_BEGIN) {
        next_content.push_str(&remaining[..start]);
        let after_begin = &remaining[start..];
        let Some(end) = after_begin.find(FOCUSD_AGENT_HOOK_BLOCK_END) else {
            remaining = "";
            break;
        };

        remaining = &after_begin[end + FOCUSD_AGENT_HOOK_BLOCK_END.len()..];
        if let Some(stripped) = remaining.strip_prefix("\r\n") {
            remaining = stripped;
        } else if let Some(stripped) = remaining.strip_prefix('\n') {
            remaining = stripped;
        }
    }

    next_content.push_str(remaining);
    remove_legacy_codex_focusd_hooks(&next_content)
}

fn remove_legacy_codex_focusd_hooks(content: &str) -> String {
    let lines = content.lines().collect::<Vec<_>>();
    let mut next_lines: Vec<&str> = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let trimmed = lines[index].trim();
        if let Some(hook_path) = codex_hook_event_path(trimmed) {
            let start = index;
            index += 1;

            while index < lines.len() && !lines[index].trim().starts_with('[') {
                index += 1;
            }

            let metadata_end = index;
            let mut kept_child_ranges = Vec::new();
            let mut removed_managed_child = false;

            while index < lines.len() {
                let candidate = lines[index].trim();
                if !is_codex_nested_hook_header_for(candidate, &hook_path) {
                    break;
                }

                let child_start = index;
                index += 1;
                while index < lines.len() && !lines[index].trim().starts_with('[') {
                    index += 1;
                }

                let child_block = lines[child_start..index].join("\n");
                if child_block.contains(FOCUSD_AGENT_HOOK_SIGNATURE) {
                    removed_managed_child = true;
                } else {
                    kept_child_ranges.push(child_start..index);
                }
            }

            if !removed_managed_child {
                next_lines.extend_from_slice(&lines[start..index]);
                continue;
            }

            if kept_child_ranges.is_empty() {
                continue;
            }

            next_lines.extend_from_slice(&lines[start..metadata_end]);
            for range in kept_child_ranges {
                next_lines.extend_from_slice(&lines[range]);
            }
            continue;
        }

        next_lines.push(lines[index]);
        index += 1;
    }

    next_lines.join("\n")
}

fn codex_hook_event_path(header: &str) -> Option<String> {
    if !header.starts_with("[[hooks.") || !header.ends_with("]]") {
        return None;
    }

    let hook_path = header.strip_prefix("[[")?.strip_suffix("]]")?;
    if hook_path.ends_with(".hooks") {
        return None;
    }

    Some(hook_path.to_string())
}

fn is_codex_nested_hook_header_for(header: &str, hook_path: &str) -> bool {
    header == format!("[[{hook_path}.hooks]]")
}

fn agent_hook_command_argument(script_path: &Path, provider: &str, phase: &str) -> String {
    format!(
        "\"\"{}\" {} {}\"",
        script_path.to_string_lossy(),
        provider,
        phase
    )
}

fn agent_hook_command(script_path: &Path, provider: &str, phase: &str) -> String {
    format!(
        "cmd.exe /d /s /c {}",
        agent_hook_command_argument(script_path, provider, phase)
    )
}

fn toml_basic_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

fn write_text_file(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create {}: {error}", parent.display()))?;
    }

    let temporary_path = path.with_extension("tmp");
    fs::write(&temporary_path, content)
        .map_err(|error| format!("Failed to write {}: {error}", temporary_path.display()))?;
    fs::rename(&temporary_path, path)
        .or_else(|_| {
            fs::remove_file(path).ok();
            fs::rename(&temporary_path, path)
        })
        .map_err(|error| format!("Failed to replace {}: {error}", path.display()))
}

fn normalize_windows_line_endings(content: &str) -> String {
    content.replace("\r\n", "\n").replace('\n', "\r\n")
}
fn windows_home_dir() -> Result<PathBuf, String> {
    if let Ok(user_profile) = env::var("USERPROFILE") {
        let user_profile = user_profile.trim();
        if !user_profile.is_empty() {
            return Ok(PathBuf::from(user_profile));
        }
    }

    match (env::var("HOMEDRIVE"), env::var("HOMEPATH")) {
        (Ok(home_drive), Ok(home_path)) if !home_drive.is_empty() && !home_path.is_empty() => {
            Ok(PathBuf::from(format!("{home_drive}{home_path}")))
        }
        _ => Err("Failed to resolve the Windows user profile directory.".to_string()),
    }
}

fn read_system_audio_peak_window(samples: usize, delay: Duration) -> Result<f32, String> {
    unsafe {
        let did_initialize_com = initialize_com_for_audio();
        let peak_result = (|| {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(media_error)?;
            let mut meters: Vec<IAudioMeterInformation> = Vec::new();

            for role in [eMultimedia, eConsole, eCommunications] {
                if let Ok(device) = enumerator.GetDefaultAudioEndpoint(eRender, role) {
                    if let Ok(meter) = device.Activate(CLSCTX_ALL, None) {
                        meters.push(meter);
                    }
                }
            }

            if meters.is_empty() {
                return Err("No default render audio endpoint was available.".to_string());
            }

            let mut peak = 0.0_f32;

            for sample_index in 0..samples.max(1) {
                for meter in &meters {
                    if let Ok(value) = meter.GetPeakValue() {
                        peak = peak.max(value);
                    }
                }

                if !delay.is_zero() && sample_index + 1 < samples {
                    thread::sleep(delay);
                }
            }

            Ok(peak.clamp(0.0, 1.0))
        })();

        if did_initialize_com {
            CoUninitialize();
        }

        peak_result
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AudioLevel {
    active: bool,
    peak: f32,
    updated_at: i64,
}

fn initialize_com_for_audio() -> bool {
    unsafe {
        let result = CoInitializeEx(None, COINIT_MULTITHREADED);

        if result == RPC_E_CHANGED_MODE {
            false
        } else {
            result.is_ok()
        }
    }
}

fn send_media_key(key: VIRTUAL_KEY) {
    let key_code = key.0 as u8;

    unsafe {
        keybd_event(key_code, 0, KEYBD_EVENT_FLAGS(0), 0);
        thread::sleep(Duration::from_millis(18));
        keybd_event(key_code, 0, KEYEVENTF_KEYUP, 0);
    }
}

fn file_modified_unix_millis(path: &Path) -> Option<i64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    system_time_to_unix_millis(modified)
}

fn current_unix_millis() -> i64 {
    system_time_to_unix_millis(SystemTime::now()).unwrap_or_default()
}

fn system_time_to_unix_millis(time: SystemTime) -> Option<i64> {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .ok()
}

fn media_error(error: windows::core::Error) -> String {
    format!("Windows media session error: {error}")
}

fn main_window(app: &AppHandle) -> Result<WebviewWindow, String> {
    app.get_webview_window(WINDOW_LABEL)
        .ok_or_else(|| "Main island window was not found.".to_string())
}

fn show_island(app: &AppHandle) -> Result<(), String> {
    let window = main_window(app)?;
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())?;
    Ok(())
}

fn hide_island(app: &AppHandle) {
    if let Ok(window) = main_window(app) {
        let _ = window.hide();
    }
}

fn window_state() -> &'static Mutex<IslandWindowState> {
    WINDOW_STATE.get_or_init(|| Mutex::new(IslandWindowState::default()))
}

fn mutate_window_state(
    update: impl FnOnce(&mut IslandWindowState) -> IslandWindowState,
) -> IslandWindowState {
    let mut state = window_state().lock().expect("window state poisoned");
    update(&mut state)
}

fn read_window_state() -> IslandWindowState {
    *window_state().lock().expect("window state poisoned")
}

fn apply_stage_geometry(window: &WebviewWindow, state: IslandWindowState) -> Result<(), String> {
    let (_, base_height) = state.mode.base_size(state.expanded_height);
    let stage_height =
        STAGE_WINDOW_HEIGHT.max((base_height * state.size_scale).ceil() + STAGE_WINDOW_PADDING_Y);

    window
        .set_size(Size::Logical(LogicalSize::new(
            STAGE_WINDOW_WIDTH,
            stage_height,
        )))
        .map_err(|error| error.to_string())?;

    let (x, y) = if state.use_free_position {
        free_position_for_state(window, state)?
    } else {
        centered_top_position(window, state)?
    };

    window
        .set_position(Position::Physical(PhysicalPosition::new(x, y)))
        .map_err(|error| error.to_string())
}

fn centered_top_position(
    window: &WebviewWindow,
    state: IslandWindowState,
) -> Result<(i32, i32), String> {
    let monitor = window
        .primary_monitor()
        .map_err(|error| error.to_string())?
        .or(window
            .current_monitor()
            .map_err(|error| error.to_string())?)
        .ok_or_else(|| "No monitor is available for island positioning.".to_string())?;

    let scale = monitor.scale_factor();
    let monitor_position = monitor.position();
    let monitor_size = monitor.size();
    let physical_width = (STAGE_WINDOW_WIDTH * scale).round() as i32;
    let physical_top_offset = if matches!(state.mode, IslandMode::Collapsed) && state.is_tucked {
        -((COLLAPSED_ISLAND_HEIGHT * state.size_scale - TUCKED_VISIBLE_EDGE_HEIGHT).max(0.0)
            * scale)
            .round() as i32
    } else {
        (state.margin_y * scale).round() as i32
    };
    let x = monitor_position.x + ((monitor_size.width as i32 - physical_width) / 2);
    let y = monitor_position.y + physical_top_offset;
    Ok((x, y))
}

fn free_position_for_state(
    window: &WebviewWindow,
    state: IslandWindowState,
) -> Result<(i32, i32), String> {
    let x = state.free_x;
    if !(matches!(state.mode, IslandMode::Collapsed) && state.is_tucked) {
        return Ok((x, state.free_y));
    }

    let monitor = window
        .available_monitors()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|monitor| {
            let position = monitor.position();
            let size = monitor.size();
            let right = position.x + size.width as i32;
            let bottom = position.y + size.height as i32;
            x >= position.x && x < right && state.free_y >= position.y && state.free_y < bottom
        })
        .or(window
            .current_monitor()
            .map_err(|error| error.to_string())?)
        .or(window
            .primary_monitor()
            .map_err(|error| error.to_string())?)
        .ok_or_else(|| "No monitor is available for island positioning.".to_string())?;

    let scale = monitor.scale_factor();
    let monitor_position = monitor.position();
    let tucked_offset = -((COLLAPSED_ISLAND_HEIGHT * state.size_scale - TUCKED_VISIBLE_EDGE_HEIGHT)
        .max(0.0)
        * scale)
        .round() as i32;
    Ok((x, monitor_position.y + tucked_offset))
}

fn island_position_snapshot(state: IslandWindowState) -> IslandPositionSnapshot {
    IslandPositionSnapshot {
        use_free_position: state.use_free_position,
        x: state.free_x,
        y: state.free_y,
    }
}

fn island_position_path(app: &AppHandle) -> Result<PathBuf, String> {
    let app_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Failed to resolve app data directory: {error}"))?;
    fs::create_dir_all(&app_dir)
        .map_err(|error| format!("Failed to create app data directory: {error}"))?;
    Ok(app_dir.join(ISLAND_POSITION_FILE_NAME))
}

fn persist_island_position(app: &AppHandle, state: &IslandWindowState) -> Result<(), String> {
    let path = island_position_path(app)?;
    let payload = PersistedIslandPosition {
        use_free_position: state.use_free_position,
        x: state.free_x,
        y: state.free_y,
    };
    let content = serde_json::to_string_pretty(&payload)
        .map_err(|error| format!("Failed to serialize island position: {error}"))?;
    write_text_file(&path, &content)
}

fn load_island_position_into_state(app: &AppHandle) {
    let Ok(path) = island_position_path(app) else {
        return;
    };
    let Ok(content) = fs::read_to_string(path) else {
        return;
    };
    let Ok(persisted) = serde_json::from_str::<PersistedIslandPosition>(&content) else {
        return;
    };
    if !persisted.use_free_position {
        return;
    }
    let _ = mutate_window_state(|state| {
        state.use_free_position = true;
        state.free_x = persisted.x;
        state.free_y = persisted.y;
        *state
    });
}

fn start_cursor_passthrough_loop(window: WebviewWindow) {
    thread::spawn(move || {
        let mut ignoring_cursor = false;

        loop {
            let should_ignore = !cursor_is_inside_island(&window);

            if should_ignore != ignoring_cursor {
                if window.set_ignore_cursor_events(should_ignore).is_ok() {
                    ignoring_cursor = should_ignore;
                }
            }

            thread::sleep(Duration::from_millis(12));
        }
    });
}

fn cursor_is_inside_island(window: &WebviewWindow) -> bool {
    let hwnd = match window.hwnd() {
        Ok(hwnd) => hwnd,
        Err(_) => return true,
    };
    let mut window_rect = RECT::default();
    let mut cursor = POINT::default();

    if unsafe { GetWindowRect(hwnd, &mut window_rect) }.is_err() {
        return true;
    }

    if unsafe { GetCursorPos(&mut cursor) }.is_err() {
        return true;
    }

    let window_width = (window_rect.right - window_rect.left).max(1) as f64;
    let physical_scale = window_width / STAGE_WINDOW_WIDTH;
    let local_x = (cursor.x - window_rect.left) as f64;
    let local_y = (cursor.y - window_rect.top) as f64;
    let state = read_window_state();
    let (base_width, base_height) = state.mode.base_size(state.expanded_height);
    let island_width = base_width * state.size_scale * physical_scale;
    let island_height = base_height * state.size_scale * physical_scale;
    let island_left = (window_width - island_width) / 2.0;
    let island_top = 0.0;
    let radius = state.mode.corner_radius() * state.size_scale * physical_scale;

    point_in_rounded_rect(
        local_x,
        local_y,
        island_left,
        island_top,
        island_width,
        island_height,
        radius,
    )
}

fn point_in_rounded_rect(
    x: f64,
    y: f64,
    left: f64,
    top: f64,
    width: f64,
    height: f64,
    radius: f64,
) -> bool {
    let right = left + width;
    let bottom = top + height;

    if x < left || x > right || y < top || y > bottom {
        return false;
    }

    let radius = radius.min(width / 2.0).min(height / 2.0);
    let center_x = if x < left + radius {
        left + radius
    } else if x > right - radius {
        right - radius
    } else {
        x
    };
    let center_y = if y < top + radius {
        top + radius
    } else if y > bottom - radius {
        bottom - radius
    } else {
        y
    };
    let dx = x - center_x;
    let dy = y - center_y;

    (dx * dx) + (dy * dy) <= radius * radius
}

fn build_tray(app: &App) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, "show", "Show Island", true, None::<&str>)?;
    let hide_item = MenuItem::with_id(app, "hide", "Hide Island", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &hide_item, &quit_item])?;

    let mut tray = TrayIconBuilder::new()
        .tooltip("FocuSD Island")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                let _ = show_island(app);
            }
            "hide" => hide_island(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = show_island(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }

    tray.build(app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_thread_index_uses_latest_exact_session_title() {
        let session_id = "11111111-2222-3333-4444-555555555555";
        let index = format!(
            "{{\"id\":\"{session_id}\",\"thread_name\":\"旧标题\"}}\n             not-json\n             {{\"id\":\"other-session\",\"thread_name\":\"其他标题\"}}\n             {{\"id\":\"{session_id}\",\"thread_name\":\"  最新标题  \"}}"
        );

        let names = parse_all_codex_thread_names(&index);

        assert_eq!(names.get(session_id).map(String::as_str), Some("最新标题"));
        assert_eq!(names.len(), 2);
    }

    #[test]
    fn generated_hooks_use_short_cmd_entry_points() {
        let script_path = Path::new(r"C:\FocuSD\focusd-agent-hook.cmd");
        let codex_command = agent_hook_command(script_path, "codex", "running");
        assert!(codex_command.contains("focusd-agent-hook.cmd"));
        assert!(codex_command.contains("codex"));
        assert!(codex_command.contains("running"));
        assert!(codex_command.contains("cmd.exe /d /s /c"));

        let claude_entry = claude_code_status_hook_entry(script_path, "running");
        assert_eq!(
            claude_entry["hooks"][0]["command"].as_str(),
            Some("cmd.exe")
        );
        let args = claude_entry["hooks"][0]["args"]
            .as_array()
            .expect("Claude hook args should be an array");
        assert_eq!(args[0].as_str(), Some("/d"));
        assert_eq!(args[1].as_str(), Some("/s"));
        assert_eq!(args[2].as_str(), Some("/c"));
        let command = args[3]
            .as_str()
            .expect("Claude hook should include the cmd command string");
        assert!(command.contains("focusd-agent-hook.cmd"));
        assert!(command.contains("claudeCode"));
        assert!(command.contains("running"));
    }

    #[test]
    fn generated_codex_hooks_include_upgrade_version_marker() {
        let block = build_codex_hook_block(Path::new(r"C:\FocuSD\focusd-agent-hook.cmd"));
        assert!(block.contains(FOCUSD_AGENT_HOOK_VERSION_MARKER));
        assert!(block.contains(AGENT_HOOK_SCRIPT_FILE_NAME));
        assert!(managed_codex_hook_has_current_entry(&block));
    }

    #[test]
    fn managed_hook_upgrade_rejects_legacy_inline_entry() {
        let legacy = r#"
# BEGIN FocuSD Agent Status Hooks
command = "powershell.exe -NoProfile -ExecutionPolicy Bypass -Command ConvertFrom-Json"
command = "C:\Users\Test\focusd-agent-status.ps1"
# END FocuSD Agent Status Hooks
"#;

        assert!(!managed_codex_hook_has_current_entry(legacy));
        assert!(!managed_claude_hook_has_current_entry(legacy));
    }

    #[test]
    fn managed_hook_upgrade_ignores_unrelated_hook_entry() {
        assert!(!managed_codex_hook_has_current_entry(
            "command = \"other-hook.cmd\" codex running"
        ));
        assert!(!managed_claude_hook_has_current_entry(
            "command = \"focusd-agent-hook.cmd\" codex running"
        ));
    }
}
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let _ = show_island(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            build_tray(app)?;
            if let Err(error) = clipboard_history::init(app.handle()) {
                eprintln!("failed to initialize clipboard history: {error}");
            }
            load_island_position_into_state(app.handle());
            let hook_app = app.handle().clone();
            thread::spawn(move || {
                if let Err(error) = refresh_agent_status_hooks_if_installed(hook_app) {
                    eprintln!("failed to refresh installed agent status hooks: {error}");
                }
            });

            if let Ok(window) = main_window(app.handle()) {
                let state = read_window_state();
                if let Err(error) = apply_stage_geometry(&window, state) {
                    eprintln!("failed to size and position island window: {error}");
                }
                start_cursor_passthrough_loop(window);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            set_island_layout,
            set_island_interaction,
            set_island_free_position,
            capture_island_free_position,
            reset_island_position,
            get_island_position,
            save_todo_markdown,
            get_default_todo_save_directory,
            show_ready_island,
            minimize_island,
            get_launch_at_startup,
            set_launch_at_startup,
            get_agent_status,
            clear_agent_status,
            install_agent_status_hooks,
            get_media_state,
            get_audio_level,
            media_play_pause,
            media_next,
            media_previous,
            clipboard_history::get_clipboard_history,
            clipboard_history::set_clipboard_history_settings,
            clipboard_history::copy_clipboard_history_item,
            clipboard_history::toggle_clipboard_history_favorite,
            clipboard_history::delete_clipboard_history_item,
            clipboard_history::clear_clipboard_history
        ])
        .run(tauri::generate_context!())
        .expect("error while running FocuSD Island");
}

/*
=== 修改记录 ===
[修改编号]: 1
[修改日期]: 2026-07-21
[修改类型]: 新增功能
[主要内容]:
- Agent 状态改为多实例 instances 模型，扫描 per-instance running/hold marker
- clear_agent_status 支持按 instanceId/provider 清除
- 新增自由定位命令与 island-position.json 持久化
- 折叠窗口默认宽度调整，兼容 legacy 单 marker
[修改目的]:
- 支持多窗口 Agent 同时工作与胶囊任意拖动
[影响范围]:
- lib.rs 状态读写、窗口几何、Tauri commands

编号2：修改
主要修改内容：Hook 改用 stdin session_id 精确配对；completed 实例最多保留24小时；仅在脚本变更或旧命令存在时自动升级既有 Hook；运行 marker 不再因长时间未更新而转为 stale。
修改目的：修复并发 Agent 状态互相覆盖，保证数小时长任务持续显示运行中，并避免每次启动重复重写 Hook 信任配置。

编号3：新增/修改
主要修改内容：Codex 实例按 session_id 只读匹配 session_index.jsonl 的 thread_name；旧 Hook 按 Provider 独立升级并清理随机幽灵 marker。
修改目的：展开 Agent 后显示自动对话标题，并避免中断后继续对话时旧红灯残留或新增重复灯。

编号4：修改
主要修改内容：Codex/Claude Hook 顶层按 UTF-8 解析 stdin，将安全化 instanceId 与 turn_id 按既有位置参数传入；旧 Hook 按 Provider 自动升级并清理 marker。
修改目的：规避 Windows PowerShell stdin 编码与脚本参数绑定差异，彻底避免随机实例 ID、重复灯和幽灵红灯。
\n编号5：新增/修改\n主要修改内容：清除状态与 Hook 共用命名互斥锁；Codex 标题索引按文件修改时间缓存；Hook 升级判断仅认可包含 FocuSD 标记和 -HookResponse 的行。\n修改目的：消除清除竞态，降低标题刷新开销，并避免无关 Hook 误阻止旧 FocuSD Hook 升级。\n
编号6：新增/修改
主要修改内容：新增统一 focusd-agent-hook.cmd，Codex/Claude Hook 改为调用短 cmd 命令；Hook 不再输出内联 HookResponse JSON。
修改目的：规避 Codex 宿主执行长段内联 PowerShell 时的兼容性错误，保持 session_id、turn_id、多 Agent 和完成状态逻辑不变。
编号7：修改
主要修改内容：放宽 managed_hook_has_current_entry 的升级判断，不再要求 focusd-agent-hook.cmd 与 cmd.exe 出现在同一行。
修改目的：兼容 Claude settings.json 的分行字段，避免新 Hook 被误判为旧 Hook 而反复重写或状态失效。

编号8：修改
主要修改内容：为 Codex Hook 增加版本标记，拆分 Codex/Claude 的当前入口判断，并补充旧版内联 Hook 升级回归测试。
修改目的：确保旧安装包留下的长内联 PowerShell Hook 会在新版应用启动时确定性升级为短 cmd 入口，避免继续出现 hook exited with code 1。

编号9：修复
主要修改内容：Hook 配置生成统一传入 focusd-agent-hook.cmd，不再把 status.ps1 直接写入 Codex/Claude Hook 命令；增加实际生成路径断言。
修改目的：修复安装后 Hook 仍直接执行 PowerShell 状态脚本导致的 code 1，确保宿主先经过可容错的统一入口解析 stdin。*/
