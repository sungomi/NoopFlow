#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;

use noopflow_core::{Cell, Cmd, CoreEvent, CoreHandle, Settings, Snapshot};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

const MINIMIZED_ARG: &str = "--minimized";

#[tauri::command]
async fn get_state(core: State<'_, CoreHandle>) -> Result<Snapshot, String> {
    core.snapshot().await.ok_or_else(|| "core is not running".to_string())
}

#[tauri::command]
fn set_layout(core: State<CoreHandle>, cells: Vec<Cell>) {
    core.send(Cmd::SetLayout(cells));
}

#[tauri::command]
fn set_settings(core: State<CoreHandle>, settings: Settings) {
    core.send(Cmd::SetSettings(settings));
}

#[tauri::command]
fn set_name(core: State<CoreHandle>, name: String) {
    core.send(Cmd::SetName(name));
}

#[tauri::command]
fn set_locked(core: State<CoreHandle>, locked: bool) {
    core.send(Cmd::SetLocked(locked));
}

#[tauri::command]
fn pair_request(core: State<CoreHandle>, id: String) {
    core.send(Cmd::PairRequest(id));
}

#[tauri::command]
fn pair_submit(core: State<CoreHandle>, id: String, code: String) {
    core.send(Cmd::PairSubmit { id, code });
}

#[tauri::command]
fn pair_cancel(core: State<CoreHandle>) {
    core.send(Cmd::PairCancel);
}

#[tauri::command]
fn unpair(core: State<CoreHandle>, id: String) {
    core.send(Cmd::Unpair(id));
}

#[tauri::command]
fn send_files(core: State<CoreHandle>, id: String, paths: Vec<String>) {
    core.send(Cmd::SendFiles { id, paths });
}

#[tauri::command]
fn probe(core: State<CoreHandle>, ip: String) {
    core.send(Cmd::Probe(ip));
}

#[tauri::command]
fn open_receive_dir(core: State<CoreHandle>) {
    core.send(Cmd::OpenReceiveDir);
}

#[tauri::command]
fn reveal(core: State<CoreHandle>, path: String) {
    core.send(Cmd::Reveal(path));
}

#[tauri::command]
fn request_permission(core: State<CoreHandle>) {
    core.send(Cmd::RequestPermission);
}

#[tauri::command]
fn request_input_monitoring(core: State<CoreHandle>) {
    core.send(Cmd::RequestInputMonitoring);
}

#[tauri::command]
fn clip_pull(core: State<CoreHandle>, id: String) {
    core.send(Cmd::ClipPull(id));
}

#[tauri::command]
fn clip_push(core: State<CoreHandle>, id: String) {
    core.send(Cmd::ClipPush(id));
}

/// macOS applies newly granted privacy permissions only to a fresh process.
#[tauri::command]
fn restart_app(app: AppHandle) {
    app.restart();
}

#[tauri::command]
fn get_autostart(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let al = app.autolaunch();
    let res = if enabled { al.enable() } else { al.disable() };
    res.map_err(|e| e.to_string())?;
    let on = al.is_enabled().unwrap_or(enabled);
    if let Some(item) = app.try_state::<TrayItems>() {
        let _ = item.autostart.set_checked(on);
    }
    Ok(on)
}

/// Hide to the tray / menu bar; sharing keeps running.
#[tauri::command]
fn hide_window(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Tray/menu bar items that change with the app state.
struct TrayItems {
    status: MenuItem<tauri::Wry>,
    lock: CheckMenuItem<tauri::Wry>,
    autostart: CheckMenuItem<tauri::Wry>,
}

fn status_text(s: &Snapshot) -> String {
    let name = |id: &str| s.devices.iter().find(|d| d.id == id).map(|d| d.name.clone()).unwrap_or_default();
    if s.active != s.me.id {
        return format!("● {} 조작 중", name(&s.active));
    }
    if let Some(c) = &s.controlled_by {
        return format!("● {}에서 조작 중", name(c));
    }
    let online: Vec<&str> = s.devices.iter().filter(|d| d.trusted && d.online).map(|d| d.name.as_str()).collect();
    match online.len() {
        0 => "연결된 기기 없음".into(),
        1 => format!("● {} 연결됨", online[0]),
        n => format!("● {}대 연결됨", n),
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let start_minimized = std::env::args().any(|a| a == MINIMIZED_ARG);

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec![MINIMIZED_ARG])))
        .setup(move |app| {
            let dir = app.path().app_config_dir()?;

            // Start at login by default (Mac and Windows); the user can turn it off in Settings or the tray.
            // The marker is written only once enabling worked, so a failed first try is retried next launch.
            let marker = dir.join("autostart.init");
            if !marker.exists() {
                let al = app.autolaunch();
                if al.enable().is_ok() && al.is_enabled().unwrap_or(false) {
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = std::fs::write(&marker, b"1");
                } else {
                    log::warn!("could not enable start at login; will retry next launch");
                }
            }

            let status = MenuItem::with_id(app, "status", "연결된 기기 없음", false, None::<&str>)?;
            let open = MenuItem::with_id(app, "open", "NoopFlow 열기", true, None::<&str>)?;
            let lock = CheckMenuItem::with_id(app, "lock", "화면 이동 잠금", true, false, None::<&str>)?;
            let autostart_label = if cfg!(target_os = "macos") { "로그인 시 자동 실행" } else { "Windows 시작 시 자동 실행" };
            let autostart_on = app.autolaunch().is_enabled().unwrap_or(false);
            let autostart = CheckMenuItem::with_id(app, "autostart", autostart_label, true, autostart_on, None::<&str>)?;
            let clipboard = MenuItem::with_id(app, "clipboard", "클립보드…", true, None::<&str>)?;
            let settings = MenuItem::with_id(app, "settings", "설정…", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "NoopFlow 종료", true, None::<&str>)?;
            let sep1 = PredefinedMenuItem::separator(app)?;
            let sep2 = PredefinedMenuItem::separator(app)?;
            let menu = Menu::with_items(app, &[&status, &sep1, &open, &clipboard, &lock, &autostart, &settings, &sep2, &quit])?;
            app.manage(TrayItems { status: status.clone(), lock: lock.clone(), autostart: autostart.clone() });

            let handle = app.handle().clone();
            let sink = Arc::new(move |ev: CoreEvent| {
                match &ev {
                    CoreEvent::PairCode { .. } => show_main(&handle),
                    CoreEvent::State(s) => {
                        if let Some(items) = handle.try_state::<TrayItems>() {
                            let _ = items.lock.set_checked(s.locked);
                            let _ = items.status.set_text(status_text(s));
                        }
                    }
                    _ => {}
                }
                let _ = handle.emit("core", &ev);
            });

            let core = noopflow_core::start(dir, sink)?;
            app.manage(core);

            let mut tray = TrayIconBuilder::with_id("main").tooltip("NoopFlow").menu(&menu);
            if cfg!(target_os = "macos") {
                // Menu bar: monochrome template icon; a click opens the menu.
                let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-template.png"))?;
                tray = tray.icon(icon).icon_as_template(true).show_menu_on_left_click(true);
            } else {
                // Windows tray: left click opens the window, right click shows the menu.
                tray = tray.icon(app.default_window_icon().cloned().expect("icon")).show_menu_on_left_click(false);
            }
            tray.on_menu_event(|app, event| match event.id.as_ref() {
                "open" => show_main(app),
                "clipboard" => {
                    show_main(app);
                    let _ = app.emit("open-tab", "clipboard");
                }
                "settings" => {
                    show_main(app);
                    let _ = app.emit("open-tab", "settings");
                }
                "lock" => app.state::<CoreHandle>().send(Cmd::ToggleLocked),
                "autostart" => {
                    let al = app.autolaunch();
                    let on = al.is_enabled().unwrap_or(false);
                    let _ = if on { al.disable() } else { al.enable() };
                    let now = al.is_enabled().unwrap_or(!on);
                    if let Some(items) = app.try_state::<TrayItems>() {
                        let _ = items.autostart.set_checked(now);
                    }
                    let _ = app.emit("autostart", now);
                }
                "quit" => app.exit(0),
                _ => {}
            })
            .on_tray_icon_event(|tray, event| {
                if cfg!(target_os = "macos") {
                    return;
                }
                if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                    show_main(tray.app_handle());
                }
            })
            .build(app)?;

            if !start_minimized {
                show_main(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if cfg!(target_os = "macos") {
                    // Mac convention: closing hides; the menu bar icon stays.
                    let _ = window.hide();
                } else {
                    // Windows: minimize to the tray by default; the page applies the "quit" setting if chosen.
                    let _ = window.emit("close-requested", ());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            set_layout,
            set_settings,
            set_name,
            set_locked,
            pair_request,
            pair_submit,
            pair_cancel,
            unpair,
            send_files,
            probe,
            open_receive_dir,
            reveal,
            request_permission,
            request_input_monitoring,
            restart_app,
            get_autostart,
            set_autostart,
            hide_window,
            quit_app,
            clip_pull,
            clip_push
        ])
        .build(tauri::generate_context!())
        .expect("error while building NoopFlow");

    app.run(|_app, _event| {
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen { .. } = _event {
            show_main(_app);
        }
    });
}
