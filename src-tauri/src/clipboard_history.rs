use arboard::{Clipboard, ImageData};
use base64::{engine::general_purpose, Engine as _};
use image::{imageops::FilterType, DynamicImage, ImageFormat, RgbaImage};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    borrow::Cow,
    collections::HashMap,
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    sync::{
        mpsc::{self, SyncSender},
        Mutex, OnceLock,
    },
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager};
use windows::{
    core::w,
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
        System::{
            DataExchange::{AddClipboardFormatListener, RemoveClipboardFormatListener},
            LibraryLoader::GetModuleHandleW,
        },
        UI::{
            Input::KeyboardAndMouse::{
                RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL,
                MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
            },
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostMessageW,
                RegisterClassW, TranslateMessage, HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WINDOW_STYLE,
                WM_APP, WM_CLIPBOARDUPDATE, WM_HOTKEY, WNDCLASSW,
            },
        },
    },
};

const HISTORY_FILE_NAME: &str = "clipboard-history.json";
const IMAGE_DIRECTORY_NAME: &str = "clipboard-images";
const HISTORY_CHANGED_EVENT: &str = "clipboard-history-changed";
const ISLAND_SHORTCUT_EVENT: &str = "island-page-shortcut";
const MAIN_WINDOW_LABEL: &str = "main";
const DEFAULT_MAX_ITEMS: usize = 30;
const DEFAULT_CLIPBOARD_SHORTCUT: &str = "Ctrl+X";
const DEFAULT_AGENT_SHORTCUT: &str = "Ctrl+Q";
const DEFAULT_TODO_SHORTCUT: &str = "Ctrl+Alt+T";
const DEFAULT_MUSIC_SHORTCUT: &str = "Ctrl+Alt+M";
const MIN_MAX_ITEMS: usize = 5;
const MAX_MAX_ITEMS: usize = 200;
const SHORT_DUPLICATE_WINDOW_MS: i64 = 2_000;
const MAX_IMAGE_PNG_BYTES: usize = 10 * 1024 * 1024;
const THUMBNAIL_MAX_SIDE: u32 = 128;
const POLL_INTERVAL: Duration = Duration::from_secs(2);
const CLIPBOARD_WRITE_RETRY_ATTEMPTS: usize = 8;
const CLIPBOARD_WRITE_RETRY_DELAY: Duration = Duration::from_millis(35);
const AGENT_HOTKEY_ID: i32 = 0x4643;
const MUSIC_HOTKEY_ID: i32 = 0x4644;
const TODO_HOTKEY_ID: i32 = 0x4645;
const CLIPBOARD_HOTKEY_ID: i32 = 0x4646;
const HOTKEY_REFRESH_MESSAGE: u32 = WM_APP + 0x51;

static CLIPBOARD_HISTORY: OnceLock<ClipboardHistoryService> = OnceLock::new();
static CLIPBOARD_NOTIFY_TX: OnceLock<Mutex<Option<SyncSender<()>>>> = OnceLock::new();
static CLIPBOARD_HOTKEY_WINDOW: OnceLock<Mutex<Option<isize>>> = OnceLock::new();
static REGISTERED_HOTKEYS: OnceLock<Mutex<HashMap<i32, String>>> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardHistorySnapshot {
    settings: ClipboardHistorySettings,
    items: Vec<ClipboardHistoryItem>,
    clipboard_shortcut_registered: bool,
    agent_shortcut_registered: bool,
    todo_shortcut_registered: bool,
    music_shortcut_registered: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardHistorySettings {
    enabled: bool,
    capture_images: bool,
    max_items: usize,
    #[serde(default = "default_clipboard_shortcut")]
    shortcut: String,
    #[serde(default = "default_agent_shortcut")]
    agent_shortcut: String,
    #[serde(default = "default_todo_shortcut")]
    todo_shortcut: String,
    #[serde(default = "default_music_shortcut")]
    music_shortcut: String,
}

impl Default for ClipboardHistorySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            capture_images: true,
            max_items: DEFAULT_MAX_ITEMS,
            shortcut: default_clipboard_shortcut(),
            agent_shortcut: default_agent_shortcut(),
            todo_shortcut: default_todo_shortcut(),
            music_shortcut: default_music_shortcut(),
        }
    }
}

impl ClipboardHistorySettings {
    fn normalized(mut self) -> Self {
        self.max_items = self.max_items.clamp(MIN_MAX_ITEMS, MAX_MAX_ITEMS);
        self.shortcut = parse_shortcut_binding(&self.shortcut)
            .unwrap_or_else(default_clipboard_shortcut_binding)
            .label;
        self.agent_shortcut = parse_shortcut_binding(&self.agent_shortcut)
            .unwrap_or_else(default_agent_shortcut_binding)
            .label;
        self.todo_shortcut = parse_shortcut_binding(&self.todo_shortcut)
            .unwrap_or_else(default_todo_shortcut_binding)
            .label;
        self.music_shortcut = parse_shortcut_binding(&self.music_shortcut)
            .unwrap_or_else(default_music_shortcut_binding)
            .label;
        self
    }
}

struct ShortcutBinding {
    label: String,
    modifiers: u32,
    key_code: u32,
}

fn default_clipboard_shortcut() -> String {
    DEFAULT_CLIPBOARD_SHORTCUT.to_string()
}

fn default_agent_shortcut() -> String {
    DEFAULT_AGENT_SHORTCUT.to_string()
}

fn default_todo_shortcut() -> String {
    DEFAULT_TODO_SHORTCUT.to_string()
}

fn default_music_shortcut() -> String {
    DEFAULT_MUSIC_SHORTCUT.to_string()
}

fn default_clipboard_shortcut_binding() -> ShortcutBinding {
    parse_shortcut_binding(DEFAULT_CLIPBOARD_SHORTCUT)
        .expect("default clipboard shortcut should be valid")
}

fn default_agent_shortcut_binding() -> ShortcutBinding {
    parse_shortcut_binding(DEFAULT_AGENT_SHORTCUT).expect("default Agent shortcut should be valid")
}

fn default_todo_shortcut_binding() -> ShortcutBinding {
    parse_shortcut_binding(DEFAULT_TODO_SHORTCUT).expect("default Todo shortcut should be valid")
}

fn default_music_shortcut_binding() -> ShortcutBinding {
    parse_shortcut_binding(DEFAULT_MUSIC_SHORTCUT).expect("default Music shortcut should be valid")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ClipboardHistoryItemKind {
    Text,
    Image,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardHistoryItem {
    id: String,
    kind: ClipboardHistoryItemKind,
    hash: String,
    created_at: i64,
    copied_at: i64,
    #[serde(default)]
    favorite: bool,
    preview: String,
    text: Option<String>,
    image: Option<ClipboardHistoryImage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardHistoryImage {
    width: usize,
    height: usize,
    byte_size: u64,
    original_path: String,
    thumbnail_path: String,
    thumbnail_data_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedClipboardHistory {
    settings: ClipboardHistorySettings,
    items: Vec<ClipboardHistoryItem>,
}

impl Default for PersistedClipboardHistory {
    fn default() -> Self {
        Self {
            settings: ClipboardHistorySettings::default(),
            items: Vec::new(),
        }
    }
}

struct ClipboardHistoryState {
    settings: ClipboardHistorySettings,
    items: Vec<ClipboardHistoryItem>,
    last_capture_hash: Option<String>,
}

struct ClipboardHistoryService {
    app: AppHandle,
    history_path: PathBuf,
    image_dir: PathBuf,
    state: Mutex<ClipboardHistoryState>,
}

enum ClipboardCapture {
    Text {
        hash: String,
        text: String,
        preview: String,
    },
    Image {
        hash: String,
        width: usize,
        height: usize,
        rgba: Vec<u8>,
    },
}

#[tauri::command]
pub fn get_clipboard_history() -> Result<ClipboardHistorySnapshot, String> {
    service()?.snapshot()
}

#[tauri::command]
pub fn set_clipboard_history_settings(
    settings: ClipboardHistorySettings,
) -> Result<ClipboardHistorySnapshot, String> {
    service()?.set_settings(settings)
}

#[tauri::command]
pub fn copy_clipboard_history_item(id: String) -> Result<ClipboardHistorySnapshot, String> {
    service()?.copy_item(&id)
}

#[tauri::command]
pub fn toggle_clipboard_history_favorite(id: String) -> Result<ClipboardHistorySnapshot, String> {
    service()?.toggle_favorite(&id)
}

#[tauri::command]
pub fn delete_clipboard_history_item(id: String) -> Result<ClipboardHistorySnapshot, String> {
    service()?.delete_item(&id)
}

#[tauri::command]
pub fn clear_clipboard_history() -> Result<ClipboardHistorySnapshot, String> {
    service()?.clear()
}

pub fn init(app: &AppHandle) -> Result<(), String> {
    if CLIPBOARD_HISTORY.get().is_some() {
        return Ok(());
    }

    let app_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Failed to resolve app data directory: {error}"))?;
    let history_path = app_dir.join(HISTORY_FILE_NAME);
    let image_dir = app_dir.join(IMAGE_DIRECTORY_NAME);

    fs::create_dir_all(&image_dir)
        .map_err(|error| format!("Failed to create clipboard history directory: {error}"))?;

    let persisted = load_history(&history_path);
    let service = ClipboardHistoryService {
        app: app.clone(),
        history_path,
        image_dir,
        state: Mutex::new(ClipboardHistoryState {
            settings: persisted.settings.normalized(),
            items: persisted.items,
            last_capture_hash: None,
        }),
    };

    let _ = CLIPBOARD_HISTORY.set(service);
    start_workers();
    notify_clipboard_changed();
    Ok(())
}

fn service() -> Result<&'static ClipboardHistoryService, String> {
    CLIPBOARD_HISTORY
        .get()
        .ok_or_else(|| "Clipboard history service is not initialized.".to_string())
}

fn load_history(history_path: &Path) -> PersistedClipboardHistory {
    fs::read_to_string(history_path)
        .ok()
        .and_then(|content| serde_json::from_str::<PersistedClipboardHistory>(&content).ok())
        .map(|mut history| {
            history.settings = history.settings.normalized();
            history.items.retain(|item| match item.kind {
                ClipboardHistoryItemKind::Text => {
                    item.text.as_ref().is_some_and(|text| !text.is_empty())
                }
                ClipboardHistoryItemKind::Image => item.image.is_some(),
            });
            history
        })
        .unwrap_or_default()
}

impl ClipboardHistoryService {
    fn snapshot(&self) -> Result<ClipboardHistorySnapshot, String> {
        let state = self.state.lock().map_err(|error| error.to_string())?;
        Ok(Self::snapshot_locked(&state))
    }

    fn set_settings(
        &self,
        settings: ClipboardHistorySettings,
    ) -> Result<ClipboardHistorySnapshot, String> {
        let mut state = self.state.lock().map_err(|error| error.to_string())?;
        state.settings = settings.normalized();
        self.enforce_limit_locked(&mut state);
        self.persist_locked(&state)?;
        let snapshot = Self::snapshot_locked(&state);
        drop(state);

        self.emit_changed();
        request_hotkey_refresh();
        Ok(snapshot)
    }

    fn copy_item(&self, id: &str) -> Result<ClipboardHistorySnapshot, String> {
        let item = {
            let state = self.state.lock().map_err(|error| error.to_string())?;
            state
                .items
                .iter()
                .find(|item| item.id == id)
                .cloned()
                .ok_or_else(|| "Clipboard history item was not found.".to_string())?
        };

        match item.kind {
            ClipboardHistoryItemKind::Text => {
                let text = item
                    .text
                    .clone()
                    .ok_or_else(|| "Clipboard text item has no text.".to_string())?;
                write_clipboard_text(&text)?;
            }
            ClipboardHistoryItemKind::Image => {
                let image = item
                    .image
                    .as_ref()
                    .ok_or_else(|| "Clipboard image item has no image.".to_string())?;
                let rgba = image::open(&image.original_path)
                    .map_err(|error| format!("Failed to load clipboard image: {error}"))?
                    .to_rgba8();
                write_clipboard_image(rgba.width() as usize, rgba.height() as usize, &rgba)?;
            }
        }

        let mut state = self.state.lock().map_err(|error| error.to_string())?;
        state.last_capture_hash = Some(item.hash.clone());
        if let Some(index) = state
            .items
            .iter()
            .position(|history_item| history_item.hash == item.hash)
        {
            let mut moved_item = state.items.remove(index);
            moved_item.copied_at = current_unix_millis();
            state.items.insert(0, moved_item);
            self.persist_locked(&state)?;
        }
        let snapshot = Self::snapshot_locked(&state);
        drop(state);

        self.emit_changed();
        Ok(snapshot)
    }

    fn toggle_favorite(&self, id: &str) -> Result<ClipboardHistorySnapshot, String> {
        let mut state = self.state.lock().map_err(|error| error.to_string())?;
        let item = state
            .items
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or_else(|| "Clipboard history item was not found.".to_string())?;
        item.favorite = !item.favorite;
        self.persist_locked(&state)?;
        let snapshot = Self::snapshot_locked(&state);
        drop(state);

        self.emit_changed();
        Ok(snapshot)
    }

    fn delete_item(&self, id: &str) -> Result<ClipboardHistorySnapshot, String> {
        let mut state = self.state.lock().map_err(|error| error.to_string())?;
        if let Some(index) = state.items.iter().position(|item| item.id == id) {
            let item = state.items.remove(index);
            self.remove_item_files(&item);
            self.persist_locked(&state)?;
        }
        let snapshot = Self::snapshot_locked(&state);
        drop(state);

        self.emit_changed();
        Ok(snapshot)
    }

    fn clear(&self) -> Result<ClipboardHistorySnapshot, String> {
        let mut state = self.state.lock().map_err(|error| error.to_string())?;
        for item in state.items.iter().filter(|item| !item.favorite) {
            self.remove_item_files(item);
        }
        state.items.retain(|item| item.favorite);
        self.persist_locked(&state)?;
        let snapshot = Self::snapshot_locked(&state);
        drop(state);

        self.emit_changed();
        Ok(snapshot)
    }

    fn capture_current_clipboard(&self) {
        let settings = match self.state.lock() {
            Ok(state) => state.settings.clone(),
            Err(error) => {
                eprintln!("failed to lock clipboard history state: {error}");
                return;
            }
        };

        if !settings.enabled {
            return;
        }

        let capture = match read_clipboard_capture(settings.capture_images) {
            Ok(Some(capture)) => capture,
            Ok(None) => return,
            Err(error) => {
                eprintln!("failed to read clipboard: {error}");
                return;
            }
        };

        if let Err(error) = self.insert_capture(capture) {
            eprintln!("failed to store clipboard history item: {error}");
        }
    }

    fn insert_capture(&self, capture: ClipboardCapture) -> Result<(), String> {
        let now = current_unix_millis();
        let hash = capture.hash().to_string();
        let mut state = self.state.lock().map_err(|error| error.to_string())?;

        if state.last_capture_hash.as_deref() == Some(hash.as_str()) {
            return Ok(());
        }
        state.last_capture_hash = Some(hash.clone());

        if let Some(first) = state.items.first() {
            if first.hash == hash
                && now.saturating_sub(first.copied_at) <= SHORT_DUPLICATE_WINDOW_MS
            {
                return Ok(());
            }
        }

        if let Some(index) = state.items.iter().position(|item| item.hash == hash) {
            let mut item = state.items.remove(index);
            item.copied_at = now;
            state.items.insert(0, item);
            self.persist_locked(&state)?;
            drop(state);
            self.emit_changed();
            return Ok(());
        }

        let item = match capture {
            ClipboardCapture::Text {
                hash,
                text,
                preview,
            } => ClipboardHistoryItem {
                id: create_item_id(now, &hash),
                kind: ClipboardHistoryItemKind::Text,
                hash,
                created_at: now,
                copied_at: now,
                favorite: false,
                preview,
                text: Some(text),
                image: None,
            },
            ClipboardCapture::Image {
                hash,
                width,
                height,
                rgba,
            } => {
                let image = self.persist_image(&hash, width, height, rgba)?;
                ClipboardHistoryItem {
                    id: create_item_id(now, &hash),
                    kind: ClipboardHistoryItemKind::Image,
                    hash,
                    created_at: now,
                    copied_at: now,
                    favorite: false,
                    preview: format!("{width} x {height} image"),
                    text: None,
                    image: Some(image),
                }
            }
        };

        state.items.insert(0, item);
        self.enforce_limit_locked(&mut state);
        self.persist_locked(&state)?;
        drop(state);

        self.emit_changed();
        Ok(())
    }

    fn persist_image(
        &self,
        hash: &str,
        width: usize,
        height: usize,
        rgba: Vec<u8>,
    ) -> Result<ClipboardHistoryImage, String> {
        let rgba_image = RgbaImage::from_raw(width as u32, height as u32, rgba)
            .ok_or_else(|| "Clipboard image data is invalid.".to_string())?;
        let original_bytes = encode_png(&rgba_image)?;

        if original_bytes.len() > MAX_IMAGE_PNG_BYTES {
            return Err("Clipboard image is larger than the history limit.".to_string());
        }

        let thumbnail = make_thumbnail(&rgba_image);
        let thumbnail_bytes = encode_png(&thumbnail)?;
        let safe_hash = hash.chars().take(24).collect::<String>();
        let original_path = self.image_dir.join(format!("{safe_hash}.png"));
        let thumbnail_path = self.image_dir.join(format!("{safe_hash}-thumb.png"));

        fs::write(&original_path, &original_bytes)
            .map_err(|error| format!("Failed to write clipboard image: {error}"))?;
        fs::write(&thumbnail_path, &thumbnail_bytes)
            .map_err(|error| format!("Failed to write clipboard image thumbnail: {error}"))?;

        Ok(ClipboardHistoryImage {
            width,
            height,
            byte_size: original_bytes.len() as u64,
            original_path: original_path.to_string_lossy().to_string(),
            thumbnail_path: thumbnail_path.to_string_lossy().to_string(),
            thumbnail_data_url: Some(format!(
                "data:image/png;base64,{}",
                general_purpose::STANDARD.encode(thumbnail_bytes)
            )),
        })
    }

    fn enforce_limit_locked(&self, state: &mut ClipboardHistoryState) {
        while state.items.len() > state.settings.max_items {
            let removal_index = state
                .items
                .iter()
                .rposition(|item| !item.favorite)
                .or_else(|| state.items.len().checked_sub(1));
            let Some(removal_index) = removal_index else {
                break;
            };

            let item = state.items.remove(removal_index);
            self.remove_item_files(&item);
        }
    }

    fn persist_locked(&self, state: &ClipboardHistoryState) -> Result<(), String> {
        let history = PersistedClipboardHistory {
            settings: state.settings.clone(),
            items: state.items.clone(),
        };
        let content = serde_json::to_string_pretty(&history)
            .map_err(|error| format!("Failed to serialize clipboard history: {error}"))?;
        fs::write(&self.history_path, content)
            .map_err(|error| format!("Failed to write clipboard history: {error}"))
    }

    fn remove_item_files(&self, item: &ClipboardHistoryItem) {
        if let Some(image) = &item.image {
            let _ = fs::remove_file(&image.original_path);
            let _ = fs::remove_file(&image.thumbnail_path);
        }
    }

    fn snapshot_locked(state: &ClipboardHistoryState) -> ClipboardHistorySnapshot {
        ClipboardHistorySnapshot {
            settings: state.settings.clone(),
            items: state.items.clone(),
            clipboard_shortcut_registered: shortcut_is_registered(&state.settings.shortcut),
            agent_shortcut_registered: shortcut_is_registered(&state.settings.agent_shortcut),
            todo_shortcut_registered: shortcut_is_registered(&state.settings.todo_shortcut),
            music_shortcut_registered: shortcut_is_registered(&state.settings.music_shortcut),
        }
    }

    fn emit_changed(&self) {
        let _ = self.app.emit(HISTORY_CHANGED_EVENT, ());
    }

    fn focus_main_window(&self) {
        if let Some(window) = self.app.get_webview_window(MAIN_WINDOW_LABEL) {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }

    fn emit_page_shortcut(&self, shortcut: &str) {
        self.focus_main_window();
        let _ = self.app.emit(ISLAND_SHORTCUT_EVENT, shortcut.to_string());
    }
}

impl ClipboardCapture {
    fn hash(&self) -> &str {
        match self {
            Self::Text { hash, .. } | Self::Image { hash, .. } => hash,
        }
    }
}

fn read_clipboard_capture(capture_images: bool) -> Result<Option<ClipboardCapture>, String> {
    let mut clipboard = Clipboard::new().map_err(|error| error.to_string())?;

    if let Ok(text) = clipboard.get_text() {
        if !text.trim().is_empty() {
            let hash = hash_bytes([b"text:", text.as_bytes()].concat().as_slice());
            return Ok(Some(ClipboardCapture::Text {
                hash,
                preview: preview_text(&text),
                text,
            }));
        }
    }

    if capture_images {
        if let Ok(image) = clipboard.get_image() {
            let mut hash_input = Vec::with_capacity(16 + image.bytes.len());
            hash_input.extend_from_slice(&image.width.to_le_bytes());
            hash_input.extend_from_slice(&image.height.to_le_bytes());
            hash_input.extend_from_slice(&image.bytes);
            let hash = hash_bytes(&hash_input);

            return Ok(Some(ClipboardCapture::Image {
                hash,
                width: image.width,
                height: image.height,
                rgba: image.bytes.into_owned(),
            }));
        }
    }

    Ok(None)
}

fn write_clipboard_text(text: &str) -> Result<(), String> {
    retry_clipboard_write("set clipboard text", || {
        Clipboard::new()
            .map_err(|error| format!("Failed to open clipboard: {error}"))?
            .set_text(text.to_owned())
            .map_err(|error| format!("Failed to set clipboard text: {error}"))
    })
}

fn write_clipboard_image(width: usize, height: usize, image: &RgbaImage) -> Result<(), String> {
    retry_clipboard_write("set clipboard image", || {
        Clipboard::new()
            .map_err(|error| format!("Failed to open clipboard: {error}"))?
            .set_image(ImageData {
                width,
                height,
                bytes: Cow::Owned(image.as_raw().clone()),
            })
            .map_err(|error| format!("Failed to set clipboard image: {error}"))
    })
}

fn retry_clipboard_write(
    action: &str,
    mut write: impl FnMut() -> Result<(), String>,
) -> Result<(), String> {
    let mut last_error = None;

    for attempt in 0..CLIPBOARD_WRITE_RETRY_ATTEMPTS {
        match write() {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = Some(error);

                if attempt + 1 < CLIPBOARD_WRITE_RETRY_ATTEMPTS {
                    thread::sleep(CLIPBOARD_WRITE_RETRY_DELAY);
                }
            }
        }
    }

    Err(format!(
        "Failed to {action} after {CLIPBOARD_WRITE_RETRY_ATTEMPTS} attempts: {}",
        last_error.unwrap_or_else(|| "unknown error".to_string())
    ))
}

fn encode_png(image: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image.clone())
        .write_to(&mut bytes, ImageFormat::Png)
        .map_err(|error| format!("Failed to encode clipboard image: {error}"))?;
    Ok(bytes.into_inner())
}

fn make_thumbnail(image: &RgbaImage) -> RgbaImage {
    let width = image.width().max(1);
    let height = image.height().max(1);
    let scale = (THUMBNAIL_MAX_SIDE as f32 / width.max(height) as f32).min(1.0);
    let thumbnail_width = ((width as f32 * scale).round() as u32).max(1);
    let thumbnail_height = ((height as f32 * scale).round() as u32).max(1);

    image::imageops::resize(
        image,
        thumbnail_width,
        thumbnail_height,
        FilterType::Lanczos3,
    )
}

fn hash_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn preview_text(text: &str) -> String {
    const PREVIEW_LIMIT: usize = 140;
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");

    if collapsed.chars().count() <= PREVIEW_LIMIT {
        return collapsed;
    }

    let mut preview = collapsed.chars().take(PREVIEW_LIMIT).collect::<String>();
    preview.push_str("...");
    preview
}

fn create_item_id(timestamp: i64, hash: &str) -> String {
    let short_hash = hash.chars().take(12).collect::<String>();
    format!("{timestamp}-{short_hash}")
}

fn start_workers() {
    // Clipboard reads can block while another process owns the clipboard. Keep at most one
    // pending read so a busy clipboard cannot build an unbounded backlog of work.
    let (tx, rx) = mpsc::sync_channel::<()>(1);
    let _ = CLIPBOARD_NOTIFY_TX.set(Mutex::new(Some(tx)));

    thread::spawn(move || {
        while rx.recv().is_ok() {
            if let Ok(service) = service() {
                service.capture_current_clipboard();
            }
        }
    });

    thread::spawn(|| {
        if let Err(error) = run_windows_clipboard_listener() {
            eprintln!("clipboard listener fell back to polling only: {error}");

            loop {
                thread::sleep(POLL_INTERVAL);
                notify_clipboard_changed();
            }
        }
    });
}

fn request_hotkey_refresh() {
    let hwnd = CLIPBOARD_HOTKEY_WINDOW
        .get()
        .and_then(|window| window.lock().ok())
        .and_then(|window| *window)
        .map(|window| HWND(window as *mut core::ffi::c_void));

    if let Some(hwnd) = hwnd {
        unsafe {
            let _ = PostMessageW(Some(hwnd), HOTKEY_REFRESH_MESSAGE, WPARAM(0), LPARAM(0));
        }
    }
}

fn notify_clipboard_changed() {
    if let Some(sender) = CLIPBOARD_NOTIFY_TX
        .get()
        .and_then(|sender| sender.lock().ok())
        .and_then(|sender| sender.as_ref().cloned())
    {
        let _ = sender.try_send(());
    }
}

fn run_windows_clipboard_listener() -> Result<(), String> {
    unsafe {
        let class_name = w!("FocuSDClipboardHistoryListener");
        let module = GetModuleHandleW(None).map_err(|error| error.to_string())?;
        let instance = HINSTANCE(module.0);
        let window_class = WNDCLASSW {
            lpfnWndProc: Some(clipboard_window_proc),
            hInstance: instance,
            lpszClassName: class_name,
            ..Default::default()
        };

        if RegisterClassW(&window_class) == 0 {
            return Err("Failed to register clipboard listener window class.".to_string());
        }

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            w!("FocuSD Clipboard History Listener"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(instance),
            None,
        )
        .map_err(|error| error.to_string())?;

        AddClipboardFormatListener(hwnd).map_err(|error| error.to_string())?;
        if let Some(window) = CLIPBOARD_HOTKEY_WINDOW.get() {
            if let Ok(mut stored_window) = window.lock() {
                *stored_window = Some(hwnd.0 as isize);
            }
        } else {
            let _ = CLIPBOARD_HOTKEY_WINDOW.set(Mutex::new(Some(hwnd.0 as isize)));
        }
        refresh_registered_hotkey(hwnd);

        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).into() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }

        unregister_current_hotkeys(hwnd);
        let _ = RemoveClipboardFormatListener(hwnd);
    }

    Ok(())
}

unsafe extern "system" fn clipboard_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_CLIPBOARDUPDATE {
        notify_clipboard_changed();
        return LRESULT(0);
    }

    if message == WM_HOTKEY {
        if let Some(shortcut) = registered_shortcut_for_id(wparam.0 as i32) {
            if let Ok(service) = service() {
                service.emit_page_shortcut(&shortcut);
            }
            return LRESULT(0);
        }
    }

    if message == HOTKEY_REFRESH_MESSAGE {
        refresh_registered_hotkey(hwnd);
        return LRESULT(0);
    }

    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

fn registered_hotkeys() -> &'static Mutex<HashMap<i32, String>> {
    REGISTERED_HOTKEYS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn registered_shortcut_for_id(id: i32) -> Option<String> {
    registered_hotkeys()
        .lock()
        .ok()
        .and_then(|hotkeys| hotkeys.get(&id).cloned())
}

fn shortcut_is_registered(shortcut: &str) -> bool {
    registered_hotkeys()
        .lock()
        .map(|hotkeys| hotkeys.values().any(|label| label == shortcut))
        .unwrap_or(false)
}

fn unique_shortcut_candidates(settings: &ClipboardHistorySettings) -> Vec<(i32, ShortcutBinding)> {
    // 注册顺序与前端轮换顺序保持一致；重复组合键只保留第一个原生热键 ID。
    let candidates = [
        (
            AGENT_HOTKEY_ID,
            parse_shortcut_binding(&settings.agent_shortcut)
                .unwrap_or_else(default_agent_shortcut_binding),
        ),
        (
            MUSIC_HOTKEY_ID,
            parse_shortcut_binding(&settings.music_shortcut)
                .unwrap_or_else(default_music_shortcut_binding),
        ),
        (
            TODO_HOTKEY_ID,
            parse_shortcut_binding(&settings.todo_shortcut)
                .unwrap_or_else(default_todo_shortcut_binding),
        ),
        (
            CLIPBOARD_HOTKEY_ID,
            parse_shortcut_binding(&settings.shortcut)
                .unwrap_or_else(default_clipboard_shortcut_binding),
        ),
    ];
    let mut unique: Vec<(i32, ShortcutBinding)> = Vec::new();

    for candidate in candidates {
        if unique
            .iter()
            .any(|(_, binding)| binding.label == candidate.1.label)
        {
            continue;
        }
        unique.push(candidate);
    }

    unique
}

fn refresh_registered_hotkey(hwnd: HWND) {
    unsafe {
        unregister_current_hotkeys(hwnd);

        let settings = service()
            .ok()
            .and_then(|service| service.snapshot().ok())
            .map(|snapshot| snapshot.settings)
            .unwrap_or_default();

        for (id, binding) in unique_shortcut_candidates(&settings) {
            match RegisterHotKey(
                Some(hwnd),
                id,
                HOT_KEY_MODIFIERS(binding.modifiers | MOD_NOREPEAT.0),
                binding.key_code,
            ) {
                Ok(()) => {
                    if let Ok(mut hotkeys) = registered_hotkeys().lock() {
                        hotkeys.insert(id, binding.label);
                    }
                }
                Err(error) => {
                    eprintln!(
                        "failed to register island shortcut {}: {error}",
                        binding.label
                    );
                }
            }
        }

        if let Ok(service) = service() {
            service.emit_changed();
        }
    }
}

unsafe fn unregister_current_hotkeys(hwnd: HWND) {
    let registered_ids = registered_hotkeys()
        .lock()
        .map(|mut hotkeys| hotkeys.drain().map(|(id, _)| id).collect::<Vec<_>>())
        .unwrap_or_default();

    for id in registered_ids {
        let _ = unsafe { UnregisterHotKey(Some(hwnd), id) };
    }
}

fn parse_shortcut_binding(shortcut: &str) -> Option<ShortcutBinding> {
    let mut has_ctrl = false;
    let mut has_alt = false;
    let mut has_shift = false;
    let mut has_win = false;
    let mut key_label: Option<String> = None;
    let mut key_code: Option<u32> = None;

    for part in shortcut
        .split('+')
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
    {
        let normalized = part.to_ascii_lowercase();

        match normalized.as_str() {
            "ctrl" | "control" => has_ctrl = true,
            "alt" | "option" => has_alt = true,
            "shift" => has_shift = true,
            "win" | "windows" | "meta" | "cmd" | "super" => has_win = true,
            _ => {
                if key_code.is_some() {
                    return None;
                }

                let (label, code) = shortcut_key_code(part)?;
                key_label = Some(label);
                key_code = Some(code);
            }
        }
    }

    if !(has_ctrl || has_alt || has_shift || has_win) {
        return None;
    }

    let mut label_parts = Vec::new();
    let mut modifiers = 0;

    if has_ctrl {
        label_parts.push("Ctrl".to_string());
        modifiers |= MOD_CONTROL.0;
    }

    if has_alt {
        label_parts.push("Alt".to_string());
        modifiers |= MOD_ALT.0;
    }

    if has_shift {
        label_parts.push("Shift".to_string());
        modifiers |= MOD_SHIFT.0;
    }

    if has_win {
        label_parts.push("Win".to_string());
        modifiers |= MOD_WIN.0;
    }

    label_parts.push(key_label?);

    Some(ShortcutBinding {
        label: label_parts.join("+"),
        modifiers,
        key_code: key_code?,
    })
}

fn shortcut_key_code(key: &str) -> Option<(String, u32)> {
    let upper = key.trim().to_ascii_uppercase();

    if upper.len() == 1 {
        let byte = upper.as_bytes()[0];

        if byte.is_ascii_alphanumeric() {
            return Some((upper, byte as u32));
        }
    }

    if let Some(number) = upper
        .strip_prefix('F')
        .and_then(|value| value.parse::<u32>().ok())
    {
        if (1..=24).contains(&number) {
            return Some((format!("F{number}"), 0x70 + number - 1));
        }
    }

    match upper.as_str() {
        "ESC" | "ESCAPE" => Some(("Esc".to_string(), 0x1B)),
        "TAB" => Some(("Tab".to_string(), 0x09)),
        "ENTER" | "RETURN" => Some(("Enter".to_string(), 0x0D)),
        "SPACE" => Some(("Space".to_string(), 0x20)),
        "BACKSPACE" => Some(("Backspace".to_string(), 0x08)),
        "DELETE" | "DEL" => Some(("Delete".to_string(), 0x2E)),
        "INSERT" | "INS" => Some(("Insert".to_string(), 0x2D)),
        "HOME" => Some(("Home".to_string(), 0x24)),
        "END" => Some(("End".to_string(), 0x23)),
        "PAGEUP" | "PAGE UP" => Some(("PageUp".to_string(), 0x21)),
        "PAGEDOWN" | "PAGE DOWN" => Some(("PageDown".to_string(), 0x22)),
        "ARROWUP" | "UP" => Some(("Up".to_string(), 0x26)),
        "ARROWDOWN" | "DOWN" => Some(("Down".to_string(), 0x28)),
        "ARROWLEFT" | "LEFT" => Some(("Left".to_string(), 0x25)),
        "ARROWRIGHT" | "RIGHT" => Some(("Right".to_string(), 0x27)),
        _ => None,
    }
}

fn current_unix_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_include_all_page_shortcuts() {
        let settings = ClipboardHistorySettings::default().normalized();

        assert_eq!(settings.agent_shortcut, "Ctrl+Q");
        assert_eq!(settings.music_shortcut, "Ctrl+Alt+M");
        assert_eq!(settings.todo_shortcut, "Ctrl+Alt+T");
        assert_eq!(settings.shortcut, "Ctrl+X");
    }

    #[test]
    fn duplicate_shortcuts_are_preserved_for_page_cycling() {
        let settings = ClipboardHistorySettings {
            shortcut: "Ctrl+Q".to_string(),
            agent_shortcut: "Ctrl+Q".to_string(),
            todo_shortcut: "Ctrl+Q".to_string(),
            ..ClipboardHistorySettings::default()
        }
        .normalized();

        assert_eq!(settings.agent_shortcut, "Ctrl+Q");
        assert_eq!(settings.todo_shortcut, "Ctrl+Q");
        assert_eq!(settings.shortcut, "Ctrl+Q");
    }

    #[test]
    fn duplicate_shortcuts_register_once_using_cycle_priority() {
        let settings = ClipboardHistorySettings {
            shortcut: "Ctrl+Q".to_string(),
            agent_shortcut: "Ctrl+Q".to_string(),
            music_shortcut: "Ctrl+Q".to_string(),
            todo_shortcut: "Ctrl+Q".to_string(),
            ..ClipboardHistorySettings::default()
        }
        .normalized();
        let candidates = unique_shortcut_candidates(&settings);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].0, AGENT_HOTKEY_ID);
        assert_eq!(candidates[0].1.label, "Ctrl+Q");
    }
}

/*
编号1：新增/修改
主要修改内容：四个页面快捷键按唯一组合键去重注册；允许多个页面共享同一快捷键，并通过统一事件把组合键发送给前端轮换。
修改目的：支持 Agent、音乐、待办和剪贴板的全局快捷键，并实现同键多页面循环切换。
*/
