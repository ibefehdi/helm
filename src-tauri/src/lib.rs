mod bridge;
mod commands;
mod gpu;
mod logitech;
mod pulsar;
mod pulsar_mouse;
mod model;
mod monitor;
mod state;
mod storage;
mod system;
mod updates;

use std::path::PathBuf;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WindowEvent};

use model::{Profile, Settings, load_json};
use state::AppState;

fn resolve_bridge_exe(app: &AppHandle) -> PathBuf {
    if let Ok(path) = std::env::var("HELM_BRIDGE") {
        return PathBuf::from(path);
    }
    if cfg!(debug_assertions) {
        return PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../bridge/bin/Release/net48/helm-bridge.exe");
    }
    app.path()
        .resource_dir()
        .map(|dir| dir.join("bridge").join("helm-bridge.exe"))
        .unwrap_or_else(|_| PathBuf::from("bridge/helm-bridge.exe"))
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Open Helm", true, None::<&str>)?;
    let release = MenuItem::with_id(app, "release", "Release forced fans", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &release, &PredefinedMenuItem::separator(app)?, &quit])?;

    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().cloned().expect("bundle defines an icon"))
        .tooltip("Helm")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "release" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = commands::release_fans(app).await;
                });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let args: Vec<String> = std::env::args().collect();
    let autostarted = args.iter().any(|a| a == "--autostart");

    // Release builds elevate themselves up front; the engine pipe only admits administrators.
    if !cfg!(debug_assertions) && !system::is_elevated() && !args.iter().any(|a| a == "--no-elevate") {
        if system::relaunch_elevated(&args[1..].join(" ")) {
            std::process::exit(0);
        }
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main_window(app)))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .setup(move |app| {
            let handle = app.handle().clone();
            let data_dir = handle.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;

            let settings: Settings = load_json::<Settings>(&data_dir.join("settings.json")).sanitized();
            let profiles: Vec<Profile> = load_json(&data_dir.join("profiles.json"));
            let start_hidden = autostarted && settings.start_minimized;

            app.manage(AppState::new(resolve_bridge_exe(&handle), data_dir, settings, profiles));
            build_tray(&handle)?;

            if !start_hidden {
                show_main_window(&handle);
            }

            tauri::async_runtime::spawn(monitor::run(handle.clone()));
            gpu::spawn(handle.clone());
            logitech::spawn(handle.clone());
            pulsar_mouse::spawn(handle);
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<AppState>();
                if state.settings.lock().unwrap().close_to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::get_info,
            commands::reconnect,
            commands::relaunch_as_admin,
            commands::get_fans,
            commands::set_fan,
            commands::set_fans,
            commands::force_fan,
            commands::release_fans,
            commands::get_overrides,
            commands::calibrate_fan,
            commands::cancel_calibration,
            commands::get_latest,
            commands::get_history,
            commands::reset_stats,
            commands::log_dir,
            commands::open_log_dir,
            commands::list_profiles,
            commands::save_profile,
            commands::rename_profile,
            commands::delete_profile,
            commands::apply_profile,
            commands::import_gcc,
            commands::export_profile,
            commands::import_profile_file,
            commands::get_settings,
            commands::update_settings,
            commands::get_board_info,
            commands::rgb_get,
            commands::rgb_set,
            commands::save_lighting_preset,
            commands::delete_lighting_preset,
            commands::oc_get,
            commands::oc_apply,
            commands::oc_confirm,
            commands::oc_restore,
            commands::storage_scan,
            commands::storage_trim,
            commands::updates_scan,
            commands::updates_install,
            commands::updates_cancel,
            commands::bios_check,
            commands::bios_download,
            commands::bios_usb_drives,
            commands::bios_copy_to_usb,
            commands::bios_flash_readiness,
            commands::bios_flash,
            commands::open_downloads,
            commands::gpu_get,
            commands::gpu_apply,
            commands::gpu_confirm,
            commands::gpu_restore,
            commands::devices_get,
            commands::devices_apply,
            commands::pulsar_get,
            commands::pulsar_apply,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Helm")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                // Dropping the bridge closes its stdin, which releases any forced duty cycle.
                if let Some(state) = app.try_state::<AppState>() {
                    state.bridge.lock().unwrap().take();
                }
                gpu::release_fans();
                gpu::clear_osd();
            }
        });
}
