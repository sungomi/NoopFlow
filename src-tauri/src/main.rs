#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;

use noopflow_core::{Cell, Cmd, CoreEvent, CoreHandle, Settings, Snapshot};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

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

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)))
        .setup(|app| {
            let open = MenuItem::with_id(app, "open", "NoopFlow 열기", true, None::<&str>)?;
            let lock = CheckMenuItem::with_id(app, "lock", "화면 이동 잠금", true, false, None::<&str>)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let quit = MenuItem::with_id(app, "quit", "종료", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &lock, &sep, &quit])?;

            let handle = app.handle().clone();
            let lock_item = lock.clone();
            let sink = Arc::new(move |ev: CoreEvent| {
                match &ev {
                    CoreEvent::PairCode { .. } => show_main(&handle),
                    CoreEvent::State(s) => {
                        let _ = lock_item.set_checked(s.locked);
                    }
                    _ => {}
                }
                let _ = handle.emit("core", &ev);
            });

            let dir = app.path().app_config_dir()?;
            let core = noopflow_core::start(dir, sink)?;
            app.manage(core);

            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().cloned().expect("icon"))
                .tooltip("NoopFlow")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => show_main(app),
                    "lock" => app.state::<CoreHandle>().send(Cmd::ToggleLocked),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                        show_main(tray.app_handle());
                    }
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
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
            request_permission
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
