use serde::Serialize;
use std::{
    env, fs,
    path::PathBuf,
    sync::{
        mpsc::{self, Sender},
        Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};
use tauri::{
    menu::{Menu, MenuBuilder},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Emitter, Manager, Runtime, State,
};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::UpdaterExt;
#[cfg(all(windows, debug_assertions))]
use windows_sys::Win32::System::Console::{
    SetConsoleCtrlHandler, CTRL_BREAK_EVENT, CTRL_CLOSE_EVENT, CTRL_C_EVENT, CTRL_LOGOFF_EVENT,
    CTRL_SHUTDOWN_EVENT,
};

const OSD_WIDTH: u32 = 360;
const OSD_HEIGHT: u32 = 118;
const OSD_BOTTOM_GAP: i32 = 118;
const AUTOSTART_ARG: &str = "--keyboard-lock-osd-autostart";
const AUTOSTART_PREFERENCE_FILE: &str = "autostart-enabled.txt";
const PROJECT_REPOSITORY_URL: &str = "https://github.com/coderDJing/keyboard-lock-osd";

static KEY_EVENT_SENDER: OnceLock<Sender<KeyEvent>> = OnceLock::new();
static OSD_PREFERENCES: OnceLock<Mutex<OsdPreferences>> = OnceLock::new();
static OSD_READY: OnceLock<Mutex<bool>> = OnceLock::new();
static PENDING_OSD_NOTICE: OnceLock<Mutex<Option<LockChangePayload>>> = OnceLock::new();
#[cfg(all(windows, debug_assertions))]
static CONSOLE_APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

#[derive(Clone, Copy)]
struct LaunchContext {
    source: LaunchSource,
}

struct StartupTrayToastState {
    shown: Mutex<bool>,
}

#[derive(Clone, Copy)]
enum LaunchSource {
    Autostart,
    Manual,
}

#[derive(Serialize)]
struct LaunchSourcePayload {
    source: &'static str,
    autostart: bool,
}

#[derive(Serialize)]
struct StartupToastPayload {
    title: &'static str,
    message: &'static str,
}

impl LaunchSource {
    fn current() -> Self {
        if args_include_autostart_marker(env::args()) {
            Self::Autostart
        } else {
            Self::Manual
        }
    }

    fn payload(self) -> LaunchSourcePayload {
        match self {
            Self::Autostart => LaunchSourcePayload {
                source: "autostart",
                autostart: true,
            },
            Self::Manual => LaunchSourcePayload {
                source: "manual",
                autostart: false,
            },
        }
    }
}

fn args_include_autostart_marker<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter().any(|arg| arg.as_ref() == AUTOSTART_ARG)
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum LockKey {
    Caps,
    Num,
    Scroll,
}

impl LockKey {
    fn from_id(id: &str) -> Option<Self> {
        match id {
            "caps" => Some(Self::Caps),
            "num" => Some(Self::Num),
            "scroll" => Some(Self::Scroll),
            _ => None,
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Caps => "caps",
            Self::Num => "num",
            Self::Scroll => "scroll",
        }
    }

    fn abbreviation(self) -> &'static str {
        match self {
            Self::Caps => "CAP",
            Self::Num => "NUM",
            Self::Scroll => "SCRL",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Caps => "Caps Lock",
            Self::Num => "Num Lock",
            Self::Scroll => "Scroll Lock",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Caps => "caps",
            Self::Num => "num",
            Self::Scroll => "scroll",
        }
    }

    fn all() -> [Self; 3] {
        [Self::Caps, Self::Num, Self::Scroll]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KeyEventKind {
    Down,
    Up,
}

#[derive(Clone, Copy, Debug)]
struct KeyEvent {
    key: LockKey,
    kind: KeyEventKind,
}

#[derive(Clone, Copy)]
struct OsdPreferences {
    caps: bool,
    num: bool,
    scroll: bool,
    suppress_fullscreen: bool,
}

impl Default for OsdPreferences {
    fn default() -> Self {
        Self {
            caps: true,
            num: true,
            scroll: true,
            suppress_fullscreen: true,
        }
    }
}

impl OsdPreferences {
    fn get(self, key: LockKey) -> bool {
        match key {
            LockKey::Caps => self.caps,
            LockKey::Num => self.num,
            LockKey::Scroll => self.scroll,
        }
    }

    fn set(&mut self, key: LockKey, enabled: bool) {
        match key {
            LockKey::Caps => self.caps = enabled,
            LockKey::Num => self.num = enabled,
            LockKey::Scroll => self.scroll = enabled,
        }
    }

    fn suppress_fullscreen(self) -> bool {
        self.suppress_fullscreen
    }

    fn set_suppress_fullscreen(&mut self, enabled: bool) {
        self.suppress_fullscreen = enabled;
    }
}

#[derive(Clone, Copy, Debug)]
enum UiLanguage {
    En,
    Zh,
}

impl UiLanguage {
    fn id(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Zh => "zh",
        }
    }

    fn tray_open_settings(self) -> &'static str {
        match self {
            Self::En => "Open Settings",
            Self::Zh => "打开设置",
        }
    }

    fn tray_project_repository(self) -> &'static str {
        match self {
            Self::En => "Project Repository",
            Self::Zh => "项目地址",
        }
    }

    fn tray_quit(self) -> &'static str {
        match self {
            Self::En => "Quit",
            Self::Zh => "退出",
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LockChangePayload {
    key: &'static str,
    name: &'static str,
    abbreviation: &'static str,
    icon: &'static str,
    enabled: bool,
}

impl LockChangePayload {
    fn new(key: LockKey, enabled: bool) -> Self {
        Self {
            key: key.id(),
            name: key.name(),
            abbreviation: key.abbreviation(),
            icon: key.icon(),
            enabled,
        }
    }
}

#[derive(Clone, Copy)]
struct LockSnapshot {
    caps: bool,
    num: bool,
    scroll: bool,
}

impl LockSnapshot {
    fn read() -> Self {
        Self {
            caps: is_lock_enabled(LockKey::Caps),
            num: is_lock_enabled(LockKey::Num),
            scroll: is_lock_enabled(LockKey::Scroll),
        }
    }

    fn get(self, key: LockKey) -> bool {
        match key {
            LockKey::Caps => self.caps,
            LockKey::Num => self.num,
            LockKey::Scroll => self.scroll,
        }
    }

    fn set(&mut self, key: LockKey, enabled: bool) {
        match key {
            LockKey::Caps => self.caps = enabled,
            LockKey::Num => self.num = enabled,
            LockKey::Scroll => self.scroll = enabled,
        }
    }

    fn list(self) -> Vec<LockChangePayload> {
        LockKey::all()
            .into_iter()
            .map(|key| LockChangePayload::new(key, self.get(key)))
            .collect()
    }
}

#[tauri::command]
fn current_lock_states() -> Vec<LockChangePayload> {
    LockSnapshot::read().list()
}

#[tauri::command]
fn preview_osd(app: AppHandle, key: String, enabled: bool) -> Result<(), String> {
    let key = LockKey::from_id(&key).ok_or_else(|| format!("Unknown lock key: {key}"))?;
    show_osd(&app, key, enabled);
    Ok(())
}

#[tauri::command]
fn set_osd_enabled(key: String, enabled: bool) -> Result<(), String> {
    let key = LockKey::from_id(&key).ok_or_else(|| format!("Unknown lock key: {key}"))?;
    write_osd_enabled(key, enabled);
    Ok(())
}

#[tauri::command]
fn set_suppress_fullscreen_osd(enabled: bool) {
    write_suppress_fullscreen_osd(enabled);
}

#[tauri::command]
fn current_autostart_enabled(app: AppHandle) -> bool {
    if cfg!(debug_assertions) {
        return read_autostart_preference().unwrap_or(true);
    }

    app.autolaunch()
        .is_enabled()
        .unwrap_or_else(|_| read_autostart_preference().unwrap_or(true))
}

#[tauri::command]
fn set_autostart_enabled(app: AppHandle, enabled: bool) -> Result<bool, String> {
    write_autostart_preference(enabled);
    apply_autostart_preference(&app, enabled)?;
    Ok(current_autostart_enabled(app))
}

#[tauri::command]
fn current_language() -> String {
    detect_system_language().id().to_string()
}

#[tauri::command]
fn current_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[tauri::command]
fn current_launch_source(context: State<'_, LaunchContext>) -> LaunchSourcePayload {
    context.source.payload()
}

#[tauri::command]
fn osd_ready(
    app: AppHandle,
    context: State<'_, LaunchContext>,
    toast_state: State<'_, StartupTrayToastState>,
) {
    mark_osd_ready();

    if flush_pending_osd_notice(&app) {
        return;
    }

    if !matches!(context.source, LaunchSource::Manual)
        || !mark_startup_tray_toast_shown(&toast_state)
    {
        return;
    }

    show_startup_tray_toast(&app);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let launch_source = LaunchSource::current();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if !args_include_autostart_marker(args) {
                show_settings_window(app);
            }
        }))
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name("Keyboard Lock OSD")
                .arg(AUTOSTART_ARG)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .on_window_event(|window, event| {
            if window.label() == "settings" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(move |app| {
            app.manage(LaunchContext {
                source: launch_source,
            });
            app.manage(StartupTrayToastState {
                shown: Mutex::new(false),
            });

            if let Some(icon) = app.default_window_icon().cloned() {
                for label in ["settings", "osd"] {
                    if let Some(window) = app.get_webview_window(label) {
                        let _ = window.set_icon(icon.clone());
                    }
                }
            }

            if let Some(osd) = app.get_webview_window("osd") {
                if let Err(error) = osd.set_ignore_cursor_events(true) {
                    eprintln!("failed to enable OSD cursor passthrough: {error}");
                }
            }

            install_tray(app)?;
            install_console_shutdown_handler(app.handle().clone());
            initialize_autostart(app.handle());
            spawn_auto_update_check(app.handle().clone());

            if let Err(error) = start_keyboard_listener(app.handle().clone()) {
                eprintln!("keyboard listener failed: {error}");
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            current_lock_states,
            preview_osd,
            set_osd_enabled,
            set_suppress_fullscreen_osd,
            current_autostart_enabled,
            set_autostart_enabled,
            current_language,
            current_version,
            current_launch_source,
            osd_ready
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(all(windows, debug_assertions))]
fn install_console_shutdown_handler(app: AppHandle) {
    if CONSOLE_APP_HANDLE.set(app).is_err() {
        return;
    }

    let installed = unsafe { SetConsoleCtrlHandler(Some(handle_console_shutdown), 1) };
    if installed == 0 {
        eprintln!("failed to install console shutdown handler");
    }
}

#[cfg(not(all(windows, debug_assertions)))]
fn install_console_shutdown_handler(_app: AppHandle) {}

#[cfg(all(windows, debug_assertions))]
unsafe extern "system" fn handle_console_shutdown(event: u32) -> i32 {
    match event {
        CTRL_C_EVENT | CTRL_BREAK_EVENT | CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT
        | CTRL_SHUTDOWN_EVENT => {
            if let Some(app) = CONSOLE_APP_HANDLE.get() {
                app.exit(0);
            }
            1
        }
        _ => 0,
    }
}

fn spawn_auto_update_check(app: AppHandle) {
    if cfg!(debug_assertions) {
        return;
    }

    tauri::async_runtime::spawn(async move {
        let updater = match app.updater() {
            Ok(updater) => updater,
            Err(error) => {
                eprintln!("updater initialization failed: {error}");
                return;
            }
        };

        match updater.check().await {
            Ok(Some(update)) => {
                let version = update.version.clone();
                eprintln!("installing update {version}");
                match update.download_and_install(|_, _| {}, || {}).await {
                    Ok(()) => app.restart(),
                    Err(error) => eprintln!("update installation failed: {error}"),
                }
            }
            Ok(None) => {}
            Err(error) => eprintln!("update check failed: {error}"),
        }
    });
}

fn mark_startup_tray_toast_shown(state: &StartupTrayToastState) -> bool {
    let Ok(mut shown) = state.shown.lock() else {
        return false;
    };

    if *shown {
        return false;
    }

    *shown = true;
    true
}

fn show_startup_tray_toast(app: &AppHandle) {
    let language = detect_system_language();
    let payload = startup_tray_toast_payload(language);
    let Some(window) = app.get_webview_window("osd") else {
        return;
    };

    if read_suppress_fullscreen_osd() && is_foreground_window_fullscreen() {
        let _ = window.hide();
        return;
    }

    let _ = position_osd_window(&window);
    let _ = app.emit_to("osd", "startup-tray-toast", &payload);
    reveal_osd_window(&window);
}

fn startup_tray_toast_payload(language: UiLanguage) -> StartupToastPayload {
    match language {
        UiLanguage::En => StartupToastPayload {
            title: "Keyboard Lock OSD",
            message: "Started and minimized to the tray",
        },
        UiLanguage::Zh => StartupToastPayload {
            title: "Keyboard Lock OSD",
            message: "已启动并最小化到托盘",
        },
    }
}

fn start_keyboard_listener(app: AppHandle) -> Result<(), String> {
    let initial_state = LockSnapshot::read();

    let (tx, rx) = mpsc::channel::<KeyEvent>();
    KEY_EVENT_SENDER
        .set(tx)
        .map_err(|_| "keyboard listener is already running".to_string())?;

    thread::spawn(move || {
        let mut state = initial_state;
        let mut last_event: Option<(LockKey, KeyEventKind, Instant)> = None;

        for event in rx {
            let now = Instant::now();
            if let Some((last_key, last_kind, last_at)) = last_event {
                if last_key == event.key
                    && last_kind == event.kind
                    && now.duration_since(last_at) <= Duration::from_millis(80)
                {
                    continue;
                }
            }
            last_event = Some((event.key, event.kind, now));

            match event.kind {
                KeyEventKind::Down => {
                    let enabled = !state.get(event.key);
                    state.set(event.key, enabled);
                    emit_lock_state_change(&app, event.key, enabled);
                    if read_osd_enabled(event.key) {
                        show_osd(&app, event.key, enabled);
                    }
                }
                KeyEventKind::Up => {
                    let previous = state.get(event.key);
                    let enabled = is_lock_enabled(event.key);
                    state.set(event.key, enabled);
                    if enabled != previous {
                        emit_lock_state_change(&app, event.key, enabled);
                    }
                }
            }
        }
    });

    let hook_result = spawn_keyboard_hook_thread();
    let raw_input_result = spawn_raw_input_thread();

    if hook_result.is_err() && raw_input_result.is_err() {
        return Err(format!(
            "{}; {}",
            hook_result
                .err()
                .unwrap_or_else(|| "hook unavailable".to_string()),
            raw_input_result
                .err()
                .unwrap_or_else(|| "raw input unavailable".to_string())
        ));
    }

    Ok(())
}

fn install_tray(app: &mut App) -> tauri::Result<()> {
    let handle = app.handle();
    let language = detect_system_language();
    let menu = build_tray_menu(handle, language)?;

    let mut tray = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("Keyboard Lock OSD")
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show_settings" => show_settings_window(app),
            "project_repository" => open_project_repository(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                ..
            }
            | TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } => show_settings_window(tray.app_handle()),
            _ => {}
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        tray = tray.icon(icon);
    }

    app.manage(tray.build(handle)?);
    Ok(())
}

fn build_tray_menu<R, M>(manager: &M, language: UiLanguage) -> tauri::Result<Menu<R>>
where
    R: Runtime,
    M: Manager<R>,
{
    MenuBuilder::new(manager)
        .text("show_settings", language.tray_open_settings())
        .text("project_repository", language.tray_project_repository())
        .separator()
        .text("quit", language.tray_quit())
        .build()
}

fn show_settings_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn open_project_repository(app: &AppHandle) {
    if let Err(error) = app.opener().open_url(PROJECT_REPOSITORY_URL, None::<&str>) {
        eprintln!("failed to open project repository: {error}");
    }
}

fn show_osd(app: &AppHandle, key: LockKey, enabled: bool) {
    let Some(window) = app.get_webview_window("osd") else {
        return;
    };

    if read_suppress_fullscreen_osd() && is_foreground_window_fullscreen() {
        let _ = window.hide();
        return;
    }

    let payload = LockChangePayload::new(key, enabled);
    let _ = position_osd_window(&window);

    if !is_osd_ready() {
        store_pending_osd_notice(payload);
        return;
    }

    let _ = app.emit_to("osd", "lock-key-change", &payload);

    reveal_osd_window(&window);
}

fn mark_osd_ready() {
    if let Ok(mut ready) = OSD_READY.get_or_init(|| Mutex::new(false)).lock() {
        *ready = true;
    }
}

fn is_osd_ready() -> bool {
    OSD_READY
        .get_or_init(|| Mutex::new(false))
        .lock()
        .map(|ready| *ready)
        .unwrap_or(false)
}

fn store_pending_osd_notice(payload: LockChangePayload) {
    if let Ok(mut pending) = PENDING_OSD_NOTICE.get_or_init(|| Mutex::new(None)).lock() {
        *pending = Some(payload);
    }
}

fn flush_pending_osd_notice(app: &AppHandle) -> bool {
    let payload = PENDING_OSD_NOTICE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|mut pending| pending.take());

    let Some(payload) = payload else {
        return false;
    };

    let Some(window) = app.get_webview_window("osd") else {
        return true;
    };

    if read_suppress_fullscreen_osd() && is_foreground_window_fullscreen() {
        let _ = window.hide();
        return true;
    }

    let _ = position_osd_window(&window);
    let _ = app.emit_to("osd", "lock-key-change", &payload);
    reveal_osd_window(&window);
    true
}

fn reveal_osd_window(window: &tauri::WebviewWindow) {
    let _ = window.unminimize();
    let _ = window.set_always_on_top(true);
    let _ = window.show();
}

fn emit_lock_state_change(app: &AppHandle, key: LockKey, enabled: bool) {
    let payload = LockChangePayload::new(key, enabled);
    let _ = app.emit_to("settings", "lock-state-change", &payload);
}

fn read_osd_enabled(key: LockKey) -> bool {
    OSD_PREFERENCES
        .get_or_init(|| Mutex::new(OsdPreferences::default()))
        .lock()
        .map(|preferences| preferences.get(key))
        .unwrap_or(true)
}

fn write_osd_enabled(key: LockKey, enabled: bool) {
    if let Ok(mut preferences) = OSD_PREFERENCES
        .get_or_init(|| Mutex::new(OsdPreferences::default()))
        .lock()
    {
        preferences.set(key, enabled);
    }
}

fn read_suppress_fullscreen_osd() -> bool {
    OSD_PREFERENCES
        .get_or_init(|| Mutex::new(OsdPreferences::default()))
        .lock()
        .map(|preferences| preferences.suppress_fullscreen())
        .unwrap_or(true)
}

fn write_suppress_fullscreen_osd(enabled: bool) {
    if let Ok(mut preferences) = OSD_PREFERENCES
        .get_or_init(|| Mutex::new(OsdPreferences::default()))
        .lock()
    {
        preferences.set_suppress_fullscreen(enabled);
    }
}

fn initialize_autostart(app: &AppHandle) {
    let enabled = read_autostart_preference().unwrap_or_else(|| {
        write_autostart_preference(true);
        true
    });

    if let Err(error) = apply_autostart_preference(app, enabled) {
        eprintln!("failed to apply autostart preference: {error}");
    }
}

fn apply_autostart_preference(app: &AppHandle, enabled: bool) -> Result<(), String> {
    if cfg!(debug_assertions) {
        return Ok(());
    }

    let manager = app.autolaunch();
    if enabled {
        manager.enable()
    } else {
        manager.disable()
    }
    .map_err(|error| error.to_string())
}

fn read_autostart_preference() -> Option<bool> {
    let value = fs::read_to_string(autostart_preference_path()).ok()?;
    match value.trim() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

fn write_autostart_preference(enabled: bool) {
    let path = autostart_preference_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, if enabled { "true" } else { "false" });
}

fn autostart_preference_path() -> PathBuf {
    preference_path(AUTOSTART_PREFERENCE_FILE)
}

fn detect_system_language() -> UiLanguage {
    detect_system_locale()
        .as_deref()
        .map(language_from_locale)
        .unwrap_or(UiLanguage::En)
}

fn language_from_locale(locale: &str) -> UiLanguage {
    let normalized = locale.to_ascii_lowercase();
    if normalized.starts_with("zh") {
        UiLanguage::Zh
    } else {
        UiLanguage::En
    }
}

#[cfg(target_os = "windows")]
fn detect_system_locale() -> Option<String> {
    use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;

    let mut buffer = [0u16; 85];
    let len = unsafe { GetUserDefaultLocaleName(buffer.as_mut_ptr(), buffer.len() as i32) };
    if len <= 1 {
        return None;
    }

    Some(String::from_utf16_lossy(&buffer[..len as usize - 1]))
}

#[cfg(not(target_os = "windows"))]
fn detect_system_locale() -> Option<String> {
    env::var("LANG").ok()
}

fn preference_path(file_name: &str) -> PathBuf {
    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    base.join("Keyboard Lock OSD").join(file_name)
}

fn position_osd_window(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let Some(monitor) = window.current_monitor()? else {
        return Ok(());
    };

    let work_area = monitor.work_area();
    let scale_factor = monitor.scale_factor();
    let width = (OSD_WIDTH as f64 * scale_factor).round() as i32;
    let height = (OSD_HEIGHT as f64 * scale_factor).round() as i32;
    let bottom_gap = (OSD_BOTTOM_GAP as f64 * scale_factor).round() as i32;

    let x = work_area.position.x + ((work_area.size.width as i32 - width) / 2);
    let y = work_area.position.y + work_area.size.height as i32 - height - bottom_gap;

    window.set_position(tauri::Position::Physical(tauri::PhysicalPosition { x, y }))
}

#[cfg(target_os = "windows")]
fn is_foreground_window_fullscreen() -> bool {
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::{
        Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
        },
        UI::WindowsAndMessaging::{GetForegroundWindow, IsIconic},
    };

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() || IsIconic(hwnd) != 0 {
            return false;
        }

        if is_shell_desktop_window(hwnd) {
            return false;
        }

        let Some(window_rect) = visible_window_rect(hwnd) else {
            return false;
        };

        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        if monitor.is_null() {
            return false;
        }

        let mut monitor_info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            rcMonitor: zeroed(),
            rcWork: zeroed(),
            dwFlags: 0,
        };
        if GetMonitorInfoW(monitor, &mut monitor_info) == 0 {
            return false;
        }

        rect_covers_monitor(window_rect, monitor_info.rcMonitor)
    }
}

#[cfg(target_os = "windows")]
fn visible_window_rect(
    hwnd: windows_sys::Win32::Foundation::HWND,
) -> Option<windows_sys::Win32::Foundation::RECT> {
    use std::{
        ffi::c_void,
        mem::{size_of, zeroed},
    };
    use windows_sys::Win32::{
        Foundation::RECT,
        Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS},
        UI::WindowsAndMessaging::GetWindowRect,
    };

    unsafe {
        let mut rect: RECT = zeroed();
        let result = DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            &mut rect as *mut RECT as *mut c_void,
            size_of::<RECT>() as u32,
        );
        if result == 0 && rect_has_area(rect) {
            return Some(rect);
        }

        if GetWindowRect(hwnd, &mut rect) == 0 || !rect_has_area(rect) {
            return None;
        }

        Some(rect)
    }
}

#[cfg(not(target_os = "windows"))]
fn is_foreground_window_fullscreen() -> bool {
    false
}

#[cfg(target_os = "windows")]
fn is_shell_desktop_window(hwnd: windows_sys::Win32::Foundation::HWND) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetClassNameW;

    let mut class_name = [0u16; 256];
    let len = unsafe { GetClassNameW(hwnd, class_name.as_mut_ptr(), class_name.len() as i32) };
    if len <= 0 {
        return false;
    }

    let class_name = String::from_utf16_lossy(&class_name[..len as usize]);
    is_shell_desktop_class(&class_name)
}

#[cfg(target_os = "windows")]
fn is_shell_desktop_class(class_name: &str) -> bool {
    matches!(class_name, "Progman" | "WorkerW")
}

#[cfg(target_os = "windows")]
fn rect_covers_monitor(
    window_rect: windows_sys::Win32::Foundation::RECT,
    monitor_rect: windows_sys::Win32::Foundation::RECT,
) -> bool {
    const FULLSCREEN_TOLERANCE_PX: i32 = 2;

    window_rect.left <= monitor_rect.left + FULLSCREEN_TOLERANCE_PX
        && window_rect.top <= monitor_rect.top + FULLSCREEN_TOLERANCE_PX
        && window_rect.right >= monitor_rect.right - FULLSCREEN_TOLERANCE_PX
        && window_rect.bottom >= monitor_rect.bottom - FULLSCREEN_TOLERANCE_PX
}

#[cfg(target_os = "windows")]
fn rect_has_area(rect: windows_sys::Win32::Foundation::RECT) -> bool {
    rect.right > rect.left && rect.bottom > rect.top
}

#[cfg(target_os = "windows")]
fn is_lock_enabled(key: LockKey) -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetKeyState;

    unsafe { GetKeyState(vk_code(key)) & 1 != 0 }
}

#[cfg(not(target_os = "windows"))]
fn is_lock_enabled(_key: LockKey) -> bool {
    false
}

#[cfg(target_os = "windows")]
fn spawn_keyboard_hook_thread() -> Result<(), String> {
    thread::Builder::new()
        .name("keyboard-lock-hook".to_string())
        .spawn(|| unsafe {
            windows_keyboard_hook_loop();
        })
        .map(|_| ())
        .map_err(|error| format!("failed to start keyboard hook thread: {error}"))
}

#[cfg(not(target_os = "windows"))]
fn spawn_keyboard_hook_thread() -> Result<(), String> {
    Err("keyboard hooks are only implemented on Windows".to_string())
}

#[cfg(target_os = "windows")]
fn spawn_raw_input_thread() -> Result<(), String> {
    thread::Builder::new()
        .name("keyboard-lock-raw-input".to_string())
        .spawn(|| unsafe {
            windows_raw_input_loop();
        })
        .map(|_| ())
        .map_err(|error| format!("failed to start raw input thread: {error}"))
}

#[cfg(not(target_os = "windows"))]
fn spawn_raw_input_thread() -> Result<(), String> {
    Err("raw input is only implemented on Windows".to_string())
}

#[cfg(target_os = "windows")]
fn vk_code(key: LockKey) -> i32 {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_CAPITAL, VK_NUMLOCK, VK_SCROLL};

    match key {
        LockKey::Caps => VK_CAPITAL as i32,
        LockKey::Num => VK_NUMLOCK as i32,
        LockKey::Scroll => VK_SCROLL as i32,
    }
}

#[cfg(target_os = "windows")]
unsafe fn windows_raw_input_loop() {
    use std::{
        mem::{size_of, zeroed},
        ptr::null_mut,
    };
    use windows_sys::Win32::{
        Foundation::GetLastError,
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::{RegisterRawInputDevices, RAWINPUTDEVICE, RIDEV_INPUTSINK},
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
                RegisterClassW, TranslateMessage, CS_HREDRAW, CS_VREDRAW, MSG, WM_INPUT, WNDCLASSW,
                WS_OVERLAPPED,
            },
        },
    };

    const ERROR_CLASS_ALREADY_EXISTS: u32 = 1410;

    let module = GetModuleHandleW(null_mut());
    let class_name = wide_null("KeyboardLockOsdRawInputWindow");
    let window_class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(raw_input_window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: module,
        hIcon: null_mut(),
        hCursor: null_mut(),
        hbrBackground: null_mut(),
        lpszMenuName: null_mut(),
        lpszClassName: class_name.as_ptr(),
    };

    let atom = RegisterClassW(&window_class);
    if atom == 0 {
        let error = GetLastError();
        if error != ERROR_CLASS_ALREADY_EXISTS {
            eprintln!("failed to register raw input window class: {error}");
            return;
        }
    }

    let hwnd = CreateWindowExW(
        0,
        class_name.as_ptr(),
        class_name.as_ptr(),
        WS_OVERLAPPED,
        0,
        0,
        0,
        0,
        null_mut(),
        null_mut(),
        module,
        null_mut(),
    );
    if hwnd.is_null() {
        let error = GetLastError();
        eprintln!("failed to create raw input window: {error}");
        return;
    }

    let device = RAWINPUTDEVICE {
        usUsagePage: 0x01,
        usUsage: 0x06,
        dwFlags: RIDEV_INPUTSINK,
        hwndTarget: hwnd,
    };

    let registered = RegisterRawInputDevices(
        &device,
        1,
        size_of::<RAWINPUTDEVICE>()
            .try_into()
            .expect("RAWINPUTDEVICE size fits u32"),
    );
    if registered == 0 {
        let error = GetLastError();
        eprintln!("failed to register raw input: {error}");
        let _ = DestroyWindow(hwnd);
        return;
    }

    let mut message: MSG = zeroed();
    while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }

    let _ = DestroyWindow(hwnd);

    unsafe extern "system" fn raw_input_window_proc(
        hwnd: windows_sys::Win32::Foundation::HWND,
        msg: u32,
        w_param: windows_sys::Win32::Foundation::WPARAM,
        l_param: windows_sys::Win32::Foundation::LPARAM,
    ) -> windows_sys::Win32::Foundation::LRESULT {
        if msg == WM_INPUT {
            handle_raw_input(l_param as windows_sys::Win32::UI::Input::HRAWINPUT);
        }

        DefWindowProcW(hwnd, msg, w_param, l_param)
    }
}

#[cfg(target_os = "windows")]
unsafe fn handle_raw_input(hraw_input: windows_sys::Win32::UI::Input::HRAWINPUT) {
    use std::{ffi::c_void, mem::size_of, ptr::null_mut};
    use windows_sys::Win32::UI::{
        Input::{GetRawInputData, RAWINPUT, RAWINPUTHEADER, RID_INPUT, RIM_TYPEKEYBOARD},
        WindowsAndMessaging::RI_KEY_BREAK,
    };

    let mut size = 0u32;
    let header_size = size_of::<RAWINPUTHEADER>()
        .try_into()
        .expect("RAWINPUTHEADER size fits u32");

    let query_result = GetRawInputData(hraw_input, RID_INPUT, null_mut(), &mut size, header_size);
    if query_result == u32::MAX {
        return;
    }

    if size == 0 {
        return;
    }

    let mut buffer = vec![0u8; size as usize];
    let read = GetRawInputData(
        hraw_input,
        RID_INPUT,
        buffer.as_mut_ptr() as *mut c_void,
        &mut size,
        header_size,
    );
    if read == u32::MAX {
        return;
    }

    let raw = &*(buffer.as_ptr() as *const RAWINPUT);
    if raw.header.dwType != RIM_TYPEKEYBOARD {
        return;
    }

    let keyboard = raw.data.keyboard;
    let Some(key) = lock_key_from_vk(keyboard.VKey.into()) else {
        return;
    };

    let kind = if keyboard.Flags & RI_KEY_BREAK as u16 == 0 {
        KeyEventKind::Down
    } else {
        KeyEventKind::Up
    };

    if let Some(sender) = KEY_EVENT_SENDER.get() {
        let _ = sender.send(KeyEvent { key, kind });
    }
}

#[cfg(target_os = "windows")]
fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
unsafe fn windows_keyboard_hook_loop() {
    use std::{mem::zeroed, ptr::null_mut};
    use windows_sys::Win32::{
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            GetMessageW, SetWindowsHookExW, UnhookWindowsHookEx, MSG, WH_KEYBOARD_LL,
        },
    };

    let module = GetModuleHandleW(null_mut());
    let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook_proc), module, 0);
    if hook.is_null() {
        let error = windows_sys::Win32::Foundation::GetLastError();
        eprintln!("failed to install keyboard hook: {error}");
        return;
    }

    let mut message: MSG = zeroed();
    while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {}

    let _ = UnhookWindowsHookEx(hook);
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn keyboard_hook_proc(
    code: i32,
    w_param: windows_sys::Win32::Foundation::WPARAM,
    l_param: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, KBDLLHOOKSTRUCT, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    if code >= 0 {
        let event_kind = match w_param as u32 {
            WM_KEYDOWN | WM_SYSKEYDOWN => Some(KeyEventKind::Down),
            WM_KEYUP | WM_SYSKEYUP => Some(KeyEventKind::Up),
            _ => None,
        };

        if let Some(kind) = event_kind {
            let keyboard_event = *(l_param as *const KBDLLHOOKSTRUCT);
            if let Some(key) = lock_key_from_vk(keyboard_event.vkCode) {
                if let Some(sender) = KEY_EVENT_SENDER.get() {
                    let _ = sender.send(KeyEvent { key, kind });
                }
            }
        }
    }

    CallNextHookEx(std::ptr::null_mut(), code, w_param, l_param)
}

#[cfg(target_os = "windows")]
fn lock_key_from_vk(vk_code: u32) -> Option<LockKey> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_CAPITAL, VK_NUMLOCK, VK_SCROLL};

    match vk_code as u16 {
        VK_CAPITAL => Some(LockKey::Caps),
        VK_NUMLOCK => Some(LockKey::Num),
        VK_SCROLL => Some(LockKey::Scroll),
        _ => None,
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::{is_shell_desktop_class, rect_covers_monitor};
    use windows_sys::Win32::Foundation::RECT;

    #[test]
    fn identifies_windows_desktop_shell_classes() {
        assert!(is_shell_desktop_class("Progman"));
        assert!(is_shell_desktop_class("WorkerW"));
    }

    #[test]
    fn keeps_regular_fullscreen_windows_suppressible() {
        assert!(!is_shell_desktop_class("ApplicationFrameWindow"));
        assert!(!is_shell_desktop_class("Chrome_WidgetWin_1"));
    }

    #[test]
    fn treats_monitor_sized_rect_as_fullscreen() {
        let monitor = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };

        assert!(rect_covers_monitor(monitor, monitor));
    }

    #[test]
    fn treats_work_area_rect_as_not_fullscreen() {
        let monitor = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        let work_area = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        };

        assert!(!rect_covers_monitor(work_area, monitor));
    }
}
