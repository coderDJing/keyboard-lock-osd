use serde::Serialize;
use std::{
    collections::HashSet,
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
    App, AppHandle, Emitter, Manager, Runtime, State, WebviewUrl, WebviewWindowBuilder,
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
const OSD_HORIZONTAL_GAP: i32 = 48;
const OSD_TOP_GAP: i32 = 48;
const OSD_BOTTOM_GAP: i32 = 118;
const OSD_LABEL_PREFIX: &str = "osd-";
const AUTOSTART_ARG: &str = "--keyboard-lock-osd-autostart";
const AUTOSTART_VALUE_NAME: &str = "Keyboard Lock OSD";
#[cfg(target_os = "windows")]
const AUTOSTART_RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
#[cfg(target_os = "windows")]
const AUTOSTART_APPROVED_KEY: &str =
    "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run";
const AUTOSTART_PREFERENCE_FILE: &str = "autostart-enabled.txt";
const PROJECT_REPOSITORY_URL: &str = "https://github.com/coderDJing/keyboard-lock-osd";

static KEY_EVENT_SENDER: OnceLock<Sender<KeyEvent>> = OnceLock::new();
static OSD_PREFERENCES: OnceLock<Mutex<OsdPreferences>> = OnceLock::new();
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();
#[cfg(all(windows, debug_assertions))]
static CONSOLE_APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

#[derive(Clone, Copy)]
struct LaunchContext {
    source: LaunchSource,
}

struct StartupTrayToastState {
    pending_windows: Mutex<HashSet<String>>,
}

impl StartupTrayToastState {
    fn capture_initial_windows(&self, app: &AppHandle) {
        let Ok(mut pending) = self.pending_windows.lock() else {
            return;
        };
        pending.extend(osd_windows(app).into_iter().map(|(label, _)| label));
    }
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

#[derive(Serialize)]
struct ToastPayload {
    title: String,
    message: String,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OsdPosition {
    TopLeft,
    TopCenter,
    TopRight,
    MiddleLeft,
    MiddleCenter,
    MiddleRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

impl OsdPosition {
    fn from_id(id: &str) -> Option<Self> {
        match id {
            "top-left" => Some(Self::TopLeft),
            "top-center" => Some(Self::TopCenter),
            "top-right" => Some(Self::TopRight),
            "middle-left" => Some(Self::MiddleLeft),
            "middle-center" => Some(Self::MiddleCenter),
            "middle-right" => Some(Self::MiddleRight),
            "bottom-left" => Some(Self::BottomLeft),
            "bottom-center" => Some(Self::BottomCenter),
            "bottom-right" => Some(Self::BottomRight),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
struct OsdPreferences {
    caps: bool,
    num: bool,
    scroll: bool,
    suppress_fullscreen: bool,
    position: OsdPosition,
}

impl Default for OsdPreferences {
    fn default() -> Self {
        Self {
            caps: true,
            num: true,
            scroll: true,
            suppress_fullscreen: true,
            position: OsdPosition::BottomCenter,
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

    fn position(self) -> OsdPosition {
        self.position
    }

    fn set_position(&mut self, position: OsdPosition) {
        self.position = position;
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

    fn tray_check_update(self) -> &'static str {
        match self {
            Self::En => "Check for Updates",
            Self::Zh => "检查更新",
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
fn set_osd_position(app: AppHandle, position: String) -> Result<(), String> {
    let position = OsdPosition::from_id(&position)
        .ok_or_else(|| format!("Unknown OSD position: {position}"))?;
    write_osd_position(position);

    for (index, monitor) in sorted_monitors(&app).iter().enumerate() {
        if let Some(window) = app.get_webview_window(&osd_label(index)) {
            let _ = position_osd_on_monitor(&window, monitor);
        }
    }

    Ok(())
}

#[tauri::command]
fn set_osd_theme_color(app: AppHandle, color: String) -> Result<(), String> {
    if !is_valid_theme_color(&color) {
        return Err(format!("Invalid theme color: {color}"));
    }

    app.emit("osd-theme-color", color)
        .map_err(|error| error.to_string())
}

fn is_valid_theme_color(color: &str) -> bool {
    color.len() == 7
        && color.starts_with('#')
        && color.bytes().skip(1).all(|byte| byte.is_ascii_hexdigit())
}

#[tauri::command]
fn current_autostart_enabled(app: AppHandle) -> bool {
    app.autolaunch()
        .is_enabled()
        .unwrap_or_else(|_| read_autostart_preference().unwrap_or(true))
}

#[tauri::command]
fn set_autostart_enabled(app: AppHandle, enabled: bool) -> Result<bool, String> {
    // debug 构建不接管系统自启项（避免把 target\debug 的 exe 写进注册表），
    // 也不再谎报偏好文件里的状态，直接返回真实系统状态。
    if cfg!(debug_assertions) {
        return Ok(current_autostart_enabled(app));
    }

    let previous = read_autostart_preference();
    if let Err(error) = apply_autostart_preference(&app, enabled) {
        restore_autostart_preference(previous);
        return Err(error);
    }
    write_autostart_preference(enabled);
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
    window: tauri::WebviewWindow,
    context: State<'_, LaunchContext>,
    toast_state: State<'_, StartupTrayToastState>,
) -> Option<StartupToastPayload> {
    if !matches!(context.source, LaunchSource::Manual)
        || !consume_startup_tray_toast(&toast_state, window.label())
    {
        return None;
    }

    if let Some(monitor) = assigned_monitor(&window) {
        let fullscreen_monitor_pos = if read_suppress_fullscreen_osd() {
            fullscreen_monitor_position()
        } else {
            None
        };
        if is_monitor_at_pos(&monitor, fullscreen_monitor_pos) {
            let _ = window.hide();
            return None;
        }
        let _ = position_osd_on_monitor(&window, &monitor);
    }

    reveal_osd_window(&window);
    Some(startup_tray_toast_payload(detect_system_language()))
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
                .app_name(AUTOSTART_VALUE_NAME)
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
                pending_windows: Mutex::new(HashSet::new()),
            });

            let _ = APP_HANDLE.set(app.handle().clone());

            if let Some(icon) = app.default_window_icon().cloned() {
                if let Some(window) = app.get_webview_window("settings") {
                    let _ = window.set_icon(icon.clone());
                }
            }

            // Create OSD windows for each monitor on the main thread
            reconcile_osd_windows(app.handle());
            // 只允许启动时已存在的 OSD 窗口领取启动提示，避免后来热插拔的窗口重复弹出
            app.state::<StartupTrayToastState>()
                .capture_initial_windows(app.handle());

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
            set_osd_position,
            set_osd_theme_color,
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

fn spawn_manual_update_check(app: AppHandle) {
    let language = detect_system_language();

    tauri::async_runtime::spawn(async move {
        let (checking, found, not_found, failed, installing) = match language {
            UiLanguage::En => ("Checking for updates...", "Update available", "Already up to date", "Update check failed", "Installing update..."),
            UiLanguage::Zh => ("正在检查更新...", "发现新版本", "已是最新版本", "检查更新失败", "正在安装更新..."),
        };

        show_dynamic_toast(&app, "Keyboard Lock OSD", checking);

        let updater = match app.updater() {
            Ok(updater) => updater,
            Err(error) => {
                eprintln!("updater initialization failed: {error}");
                show_dynamic_toast(&app, "Keyboard Lock OSD", failed);
                return;
            }
        };

        match updater.check().await {
            Ok(Some(update)) => {
                let version = update.version.clone();
                let msg = format!("{found}: {version}");
                show_dynamic_toast(&app, "Keyboard Lock OSD", &msg);

                show_dynamic_toast(&app, "Keyboard Lock OSD", installing);
                match update.download_and_install(|_, _| {}, || {}).await {
                    Ok(()) => app.restart(),
                    Err(error) => {
                        eprintln!("update installation failed: {error}");
                        show_dynamic_toast(&app, "Keyboard Lock OSD", failed);
                    }
                }
            }
            Ok(None) => {
                show_dynamic_toast(&app, "Keyboard Lock OSD", not_found);
            }
            Err(error) => {
                eprintln!("update check failed: {error}");
                show_dynamic_toast(&app, "Keyboard Lock OSD", failed);
            }
        }
    });
}

fn consume_startup_tray_toast(state: &StartupTrayToastState, label: &str) -> bool {
    let Ok(mut pending) = state.pending_windows.lock() else {
        return false;
    };

    pending.remove(label)
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

fn crop_to_content(icon: tauri::image::Image<'_>) -> tauri::image::Image<'static> {
    let w = icon.width();
    let h = icon.height();
    let rgba = icon.rgba();

    let (mut min_x, mut max_x) = (w, 0u32);
    let (mut min_y, mut max_y) = (h, 0u32);
    for y in 0..h {
        for x in 0..w {
            let idx = ((y * w + x) * 4) as usize;
            if rgba[idx + 3] > 0 {
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }
    }

    if max_x <= min_x || max_y <= min_y {
        return icon.to_owned();
    }

    let cw = max_x - min_x + 1;
    let ch = max_y - min_y + 1;
    let mut cropped = Vec::with_capacity((cw * ch * 4) as usize);
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let idx = ((y * w + x) * 4) as usize;
            cropped.extend_from_slice(&rgba[idx..idx + 4]);
        }
    }

    tauri::image::Image::new_owned(cropped, cw, ch)
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
            "check_update" => spawn_manual_update_check(app.clone()),
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
        tray = tray.icon(crop_to_content(icon));
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
        .text("check_update", language.tray_check_update())
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
    let payload = LockChangePayload::new(key, enabled);
    let fullscreen_monitor_pos = if read_suppress_fullscreen_osd() {
        fullscreen_monitor_position()
    } else {
        None
    };
    let js = osd_eval_script("lock", &payload);
    let monitors = sorted_monitors(app);
    ensure_osd_windows(app, monitors.len());

    for (index, monitor) in monitors.iter().enumerate() {
        let Some(window) = app.get_webview_window(&osd_label(index)) else {
            continue;
        };
        if is_monitor_at_pos(monitor, fullscreen_monitor_pos) {
            let _ = window.hide();
            continue;
        }
        let _ = position_osd_on_monitor(&window, monitor);
        reveal_osd_window(&window);
        let w = window.clone();
        let js_clone = js.clone();
        let _ = app.run_on_main_thread(move || {
            let _ = w.eval(&js_clone);
        });
    }
}

fn reveal_osd_window(window: &tauri::WebviewWindow) {
    let _ = window.unminimize();
    // 先移除再重新设置，强制触发 SetWindowPos(HWND_TOPMOST)。
    // tao 的 set_always_on_top 仅在标志有变化时才调用 SetWindowPos，
    // 若已经是 topmost 则是空操作，无法抢回被其他 TOPMOST 窗口夺走的 Z-order。
    let _ = window.set_always_on_top(false);
    let _ = window.set_always_on_top(true);
    let _ = window.show();
}

fn osd_eval_script(kind: &str, payload: &LockChangePayload) -> String {
    let json = serde_json::to_string(payload).unwrap_or_default();
    format!(
        "window.__KEYBOARD_LOCK_OSD_SHOW && window.__KEYBOARD_LOCK_OSD_SHOW({{kind:\"{kind}\",payload:{json}}})"
    )
}

fn show_dynamic_toast(app: &AppHandle, title: &str, message: &str) {
    let payload = ToastPayload {
        title: title.to_string(),
        message: message.to_string(),
    };
    let json = serde_json::to_string(&payload).unwrap_or_default();
    let js = format!(
        "window.__KEYBOARD_LOCK_OSD_SHOW && window.__KEYBOARD_LOCK_OSD_SHOW({{kind:\"toast\",payload:{json}}})"
    );
    let fullscreen_monitor_pos = if read_suppress_fullscreen_osd() {
        fullscreen_monitor_position()
    } else {
        None
    };
    let monitors = sorted_monitors(app);
    ensure_osd_windows(app, monitors.len());

    for (index, monitor) in monitors.iter().enumerate() {
        let Some(window) = app.get_webview_window(&osd_label(index)) else {
            continue;
        };
        if is_monitor_at_pos(monitor, fullscreen_monitor_pos) {
            let _ = window.hide();
            continue;
        }
        let _ = position_osd_on_monitor(&window, monitor);
        reveal_osd_window(&window);
        let w = window.clone();
        let js_clone = js.clone();
        let _ = app.run_on_main_thread(move || {
            let _ = w.eval(&js_clone);
        });
    }
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

fn read_osd_position() -> OsdPosition {
    OSD_PREFERENCES
        .get_or_init(|| Mutex::new(OsdPreferences::default()))
        .lock()
        .map(|preferences| preferences.position())
        .unwrap_or(OsdPosition::BottomCenter)
}

fn write_osd_position(position: OsdPosition) {
    if let Ok(mut preferences) = OSD_PREFERENCES
        .get_or_init(|| Mutex::new(OsdPreferences::default()))
        .lock()
    {
        preferences.set_position(position);
    }
}

fn initialize_autostart(app: &AppHandle) {
    let enabled = match read_autostart_preference() {
        Some(value) => value,
        None => {
            write_autostart_preference(true);
            true
        }
    };

    let result = if enabled {
        // 只在注册表项缺失或命令过期时补写，不重置 StartupApproved，
        // 保留任务管理器里的手动禁用状态（用户直接删除 Run 值时仍会按偏好补回）。
        reconcile_autostart_entry(app)
    } else if autostart_entry_present() {
        apply_autostart_preference(app, false)
    } else {
        Ok(())
    };

    if let Err(error) = result {
        eprintln!("failed to apply autostart preference: {error}");
    }
}

fn apply_autostart_preference(app: &AppHandle, enabled: bool) -> Result<(), String> {
    if cfg!(debug_assertions) {
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    if enabled {
        // auto-launch 0.5 写入的路径不带引号，这里自己写入带引号的命令，
        // 避免含空格路径的 unquoted path 隐患。
        let command = autostart_command_line()?;
        set_autostart_registry_command(&command)?;
        mark_autostart_approved();
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

fn restore_autostart_preference(previous: Option<bool>) {
    match previous {
        Some(value) => write_autostart_preference(value),
        None => {
            let _ = fs::remove_file(autostart_preference_path());
        }
    }
}

#[cfg(target_os = "windows")]
fn reconcile_autostart_entry(app: &AppHandle) -> Result<(), String> {
    if cfg!(debug_assertions) {
        return Ok(());
    }

    let desired = autostart_command_line()?;
    match autostart_registry_command() {
        None => apply_autostart_preference(app, true),
        Some(current) if current != desired => set_autostart_registry_command(&desired),
        Some(_) => Ok(()),
    }
}

#[cfg(not(target_os = "windows"))]
fn reconcile_autostart_entry(app: &AppHandle) -> Result<(), String> {
    apply_autostart_preference(app, true)
}

#[cfg(target_os = "windows")]
fn autostart_entry_present() -> bool {
    autostart_registry_command().is_some()
}

#[cfg(not(target_os = "windows"))]
fn autostart_entry_present() -> bool {
    false
}

#[cfg(target_os = "windows")]
fn autostart_command_line() -> Result<String, String> {
    let exe = env::current_exe().map_err(|error| error.to_string())?;
    Ok(format!("\"{}\" {AUTOSTART_ARG}", exe.display()))
}

#[cfg(target_os = "windows")]
fn autostart_registry_command() -> Option<String> {
    use std::{ffi::c_void, ptr::null_mut};
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};

    let mut buffer = [0u16; 1024];
    let mut size = (buffer.len() * std::mem::size_of::<u16>()) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            wide_null(AUTOSTART_RUN_KEY).as_ptr(),
            wide_null(AUTOSTART_VALUE_NAME).as_ptr(),
            RRF_RT_REG_SZ,
            null_mut(),
            buffer.as_mut_ptr() as *mut c_void,
            &mut size,
        )
    };
    if status != 0 {
        return None;
    }

    let len = (size as usize / std::mem::size_of::<u16>())
        .saturating_sub(1)
        .min(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..len]))
}

#[cfg(target_os = "windows")]
fn set_autostart_registry_command(command: &str) -> Result<(), String> {
    use std::ptr::null_mut;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ,
    };

    let value = wide_null(command);
    unsafe {
        let mut key: HKEY = null_mut();
        let status = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            wide_null(AUTOSTART_RUN_KEY).as_ptr(),
            0,
            KEY_SET_VALUE,
            &mut key,
        );
        if status != 0 {
            return Err(format!("failed to open autostart registry key: {status}"));
        }
        let status = RegSetValueExW(
            key,
            wide_null(AUTOSTART_VALUE_NAME).as_ptr(),
            0,
            REG_SZ,
            value.as_ptr() as *const u8,
            (value.len() * std::mem::size_of::<u16>()) as u32,
        );
        let _ = RegCloseKey(key);
        if status != 0 {
            return Err(format!("failed to write autostart registry value: {status}"));
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn mark_autostart_approved() {
    use std::ptr::null_mut;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE,
        REG_BINARY,
    };

    const ENABLED: [u8; 12] = [0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    unsafe {
        let mut key: HKEY = null_mut();
        let status = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            wide_null(AUTOSTART_APPROVED_KEY).as_ptr(),
            0,
            KEY_SET_VALUE,
            &mut key,
        );
        if status != 0 {
            return;
        }
        let _ = RegSetValueExW(
            key,
            wide_null(AUTOSTART_VALUE_NAME).as_ptr(),
            0,
            REG_BINARY,
            ENABLED.as_ptr(),
            ENABLED.len() as u32,
        );
        let _ = RegCloseKey(key);
    }
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

fn position_osd_on_monitor(
    window: &tauri::WebviewWindow,
    monitor: &tauri::Monitor,
) -> tauri::Result<()> {
    let work_area = monitor.work_area();
    let scale_factor = monitor.scale_factor();
    let width = (OSD_WIDTH as f64 * scale_factor).round() as i32;
    let height = (OSD_HEIGHT as f64 * scale_factor).round() as i32;
    let horizontal_gap = (OSD_HORIZONTAL_GAP as f64 * scale_factor).round() as i32;
    let top_gap = (OSD_TOP_GAP as f64 * scale_factor).round() as i32;
    let bottom_gap = (OSD_BOTTOM_GAP as f64 * scale_factor).round() as i32;
    let position = read_osd_position();

    let x = match position {
        OsdPosition::TopLeft | OsdPosition::MiddleLeft | OsdPosition::BottomLeft => {
            work_area.position.x + horizontal_gap
        }
        OsdPosition::TopCenter | OsdPosition::MiddleCenter | OsdPosition::BottomCenter => {
            work_area.position.x + ((work_area.size.width as i32 - width) / 2)
        }
        OsdPosition::TopRight | OsdPosition::MiddleRight | OsdPosition::BottomRight => {
            work_area.position.x + work_area.size.width as i32 - width - horizontal_gap
        }
    };
    let y = match position {
        OsdPosition::TopLeft | OsdPosition::TopCenter | OsdPosition::TopRight => {
            work_area.position.y + top_gap
        }
        OsdPosition::MiddleLeft | OsdPosition::MiddleCenter | OsdPosition::MiddleRight => {
            work_area.position.y + ((work_area.size.height as i32 - height) / 2)
        }
        OsdPosition::BottomLeft | OsdPosition::BottomCenter | OsdPosition::BottomRight => {
            work_area.position.y + work_area.size.height as i32 - height - bottom_gap
        }
    };

    window.set_position(tauri::Position::Physical(tauri::PhysicalPosition { x, y }))
}


fn osd_label(index: usize) -> String {
    format!("{OSD_LABEL_PREFIX}{index}")
}

fn osd_label_index(label: &str) -> Option<usize> {
    label.strip_prefix(OSD_LABEL_PREFIX)?.parse().ok()
}

fn osd_windows(app: &AppHandle) -> Vec<(String, tauri::WebviewWindow)> {
    app.webview_windows()
        .into_iter()
        .filter(|(label, _)| osd_label_index(label).is_some())
        .collect()
}

// 按屏幕位置排序并去重：镜像/复制模式下 Windows 会把多块物理屏报成同一个矩形，
// 把它们当成多块屏会导致窗口叠在同一块屏上、另一块屏没有窗口。
fn sorted_monitors(app: &AppHandle) -> Vec<tauri::Monitor> {
    let Some(any_window) = app.get_webview_window("settings") else {
        return Vec::new();
    };

    let Ok(mut monitors) = any_window.available_monitors() else {
        return Vec::new();
    };

    monitors.sort_by_key(|monitor| {
        let position = monitor.position();
        let size = monitor.size();
        (position.x, position.y, size.width, size.height)
    });
    monitors.dedup_by(|a, b| a.position() == b.position() && a.size() == b.size());
    monitors
}

fn assigned_monitor(window: &tauri::WebviewWindow) -> Option<tauri::Monitor> {
    let index = osd_label_index(window.label())?;
    sorted_monitors(window.app_handle()).get(index).cloned()
}

fn ensure_osd_windows(app: &AppHandle, monitor_count: usize) {
    if osd_windows(app).len() == monitor_count {
        return;
    }

    let app_for_main = app.clone();
    let _ = app.run_on_main_thread(move || reconcile_osd_windows(&app_for_main));
}

// OSD 窗口与“排序后的显示器下标”一一绑定：缺的补建、多的销毁，并把每个窗口
// 强制重新定位到它负责的显示器。只在数量变化时增删窗口会让热插拔后的窗口
// 漂到相邻屏，出现一块屏叠两个窗口、另一块屏永远没有窗口的情况。
fn reconcile_osd_windows(app: &AppHandle) {
    let monitors = sorted_monitors(app);
    if monitors.is_empty() {
        eprintln!("reconcile_osd_windows: no monitors available");
        return;
    }

    for (index, monitor) in monitors.iter().enumerate() {
        let label = osd_label(index);
        let window = app
            .get_webview_window(&label)
            .or_else(|| create_osd_window(app, &label));
        if let Some(window) = window {
            let _ = position_osd_on_monitor(&window, monitor);
        }
    }

    for (label, window) in osd_windows(app) {
        let keep = osd_label_index(&label).is_some_and(|index| index < monitors.len());
        if !keep {
            let _ = window.destroy();
        }
    }
}

fn create_osd_window(app: &AppHandle, label: &str) -> Option<tauri::WebviewWindow> {
    let window = WebviewWindowBuilder::new(
        app,
        label,
        WebviewUrl::App("index.html?view=osd".into()),
    )
    .title("Keyboard Lock OSD")
    .additional_browser_args("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --disable-crash-reporter --disable-breakpad")
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .focusable(false)
    .visible(true)
    .inner_size(OSD_WIDTH as f64, OSD_HEIGHT as f64)
    .resizable(false)
    .build();
    let window = match window {
        Ok(window) => window,
        Err(error) => {
            eprintln!("failed to create OSD window {label}: {error}");
            return None;
        }
    };

    if let Err(error) = window.set_ignore_cursor_events(true) {
        eprintln!("failed to enable OSD cursor passthrough for {label}: {error}");
    }

    if let Some(icon) = app.default_window_icon().cloned() {
        let _ = window.set_icon(icon);
    }

    Some(window)
}

fn fullscreen_monitor_position() -> Option<tauri::PhysicalPosition<i32>> {
    #[cfg(target_os = "windows")]
    {
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
                return None;
            }

            if is_shell_desktop_window(hwnd) {
                return None;
            }

            let window_rect = visible_window_rect(hwnd)?;

            let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            if monitor.is_null() {
                return None;
            }

            let mut monitor_info = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                rcMonitor: zeroed(),
                rcWork: zeroed(),
                dwFlags: 0,
            };
            if GetMonitorInfoW(monitor, &mut monitor_info) == 0 {
                return None;
            }

            if rect_covers_monitor(window_rect, monitor_info.rcMonitor) {
                Some(tauri::PhysicalPosition {
                    x: monitor_info.rcMonitor.left,
                    y: monitor_info.rcMonitor.top,
                })
            } else {
                None
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

fn is_monitor_at_pos(
    monitor: &tauri::Monitor,
    target_pos: Option<tauri::PhysicalPosition<i32>>,
) -> bool {
    let Some(target) = target_pos else {
        return false;
    };
    let pos = monitor.position();
    pos.x == target.x && pos.y == target.y
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
        const WM_DISPLAYCHANGE: u32 = 0x007E;

        if msg == WM_INPUT {
            handle_raw_input(l_param as windows_sys::Win32::UI::Input::HRAWINPUT);
        }

        if msg == WM_DISPLAYCHANGE {
            if let Some(app) = APP_HANDLE.get() {
                let app_clone = app.clone();
                let _ = app.run_on_main_thread(move || {
                    reconcile_osd_windows(&app_clone);
                });
            }
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
    use super::{
        consume_startup_tray_toast, is_shell_desktop_class, rect_covers_monitor,
        StartupTrayToastState,
    };
    use std::sync::Mutex;
    use windows_sys::Win32::Foundation::RECT;

    #[test]
    fn consumes_startup_toast_only_for_initial_windows() {
        let state = StartupTrayToastState {
            pending_windows: Mutex::new(
                ["osd-0".to_string(), "osd-1".to_string()]
                    .into_iter()
                    .collect(),
            ),
        };

        assert!(consume_startup_tray_toast(&state, "osd-0"));
        assert!(!consume_startup_tray_toast(&state, "osd-0"));
        assert!(consume_startup_tray_toast(&state, "osd-1"));
        assert!(!consume_startup_tray_toast(&state, "osd-9"));
    }

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
