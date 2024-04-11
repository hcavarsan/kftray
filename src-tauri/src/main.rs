#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod commands;
mod config;
mod db;
mod keychain;
mod logging;
mod remote_config;
mod tray;

use std::env;
use tauri::{GlobalShortcutManager, Manager, SystemTrayEvent};
use tauri_plugin_positioner::{Position, WindowExt};
use tokio::runtime::Runtime;

use std::sync::atomic::Ordering;

use commands::SaveDialogState;

use device_query::{DeviceQuery, DeviceState};

fn move_window_to_mouse_position(window: &tauri::Window, mouse_position: (i32, i32)) {
    if let Ok(window_size) = window.inner_size() {
        let window_width = window_size.width as f64;
        let window_height = window_size.height as f64;

        let new_x = mouse_position.0 as f64 - (window_width / 2.0);
        let new_y = mouse_position.1 as f64 - (window_height / 2.0);

        window
            .set_position(tauri::Position::Logical(tauri::LogicalPosition::new(
                new_x, new_y,
            )))
            .ok();
    }
}
fn main() {
    let inject_script = r#"
	var style = document.createElement('style');
	if (navigator.appVersion.includes('Mac')) {
	  style.innerHTML = `
		body { background-color: transparent !important; margin: 0; }
		.arrow { position: relative; padding: 12px 0; }
		.arrow:before {
		  content: "";
		  height: 0;
		  width: 0;
		  border-width: 0 8px 12px 8px;
		  border-style: solid;
		  border-color: transparent transparent rgba(45, 57, 81, 0.8) transparent;
		  position: absolute;
		  top: 0px;
		  left: 50%;
		  transform: translateX(-50%);
		  box-shadow: 0 -2px 4px rgba(0, 0, 0, 0.1);
		}
		body > div {
		  background-color: transparent !important;
		  overflow: hidden !important;
		}
	  `;
	} else if (navigator.appVersion.includes('Win')) {
	  style.innerHTML = 'body { background-color: transparent !important; margin: 0; } .arrow { position: relative; padding: 0 0 12px 0; } .arrow:after { content: ""; height: 0; width: 0; border-width: 12px 8px 0 8px; border-style: solid; border-color: #2f2f2f transparent transparent transparent; position: absolute; bottom: 0px; left: 50%; transform: translateX(-50%); } body > div { background-color: transparent !important; border-radius: 7px !important; overflow: hidden !important; } @media (prefers-color-scheme: light) { body > div { background-color: transparent !important; }}';
	} else {
	  style.innerHTML = `
		body { background-color: transparent !important; margin: 0; }
		body > div {
		  background-color: transparent !important;
		  border-radius: 7px !important;
		  overflow: hidden !important;
		}
	  `;
	  document.body.classList.remove('arrow');
	}
	document.head.appendChild(style);
	document.body.classList.add('arrow');
	"#;

    logging::setup_logging();
    let _ = fix_path_env::fix();
    // configure tray menu
    let system_tray = tray::create_tray_menu();
    tauri::Builder::default()
        .manage(SaveDialogState::default())
        .setup(move |app| {
            let _ = config::clean_all_custom_hosts_entries();
            let _ = db::init();
            if let Err(e) = config::migrate_configs() {
                eprintln!("Failed to migrate configs: {}", e);
            }

            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }

            let window = app.get_window("main").unwrap();
            // register global shortcut to open the app
            let mut shortcut = app.global_shortcut_manager();
            shortcut
                .register("CmdOrCtrl+Shift+F1", move || {
                    if window.is_visible().unwrap() {
                        window.hide().unwrap();
                    } else {
                        #[cfg(target_os = "linux")]
                        let device_state = DeviceState::new();
                        let mouse = device_state.get_mouse();
                        {
                            let mouse_position = mouse.coords;
                            move_window_to_mouse_position(&window, mouse_position);
                            println!("Position: {:#?}", mouse_position);
                        }
                        #[cfg(target_os = "windows")]
                        let _ = window.move_window(Position::BottomRight);
                        #[cfg(target_os = "macos")]
                        let _ = window.move_window(Position::TrayCenter);

                        window.show().unwrap();
                        window.set_focus().unwrap();
                    }
                })
                .unwrap_or_else(|err| println!("{:?}", err));

            Ok(())
        })
        .plugin(tauri_plugin_positioner::init())
        .system_tray(system_tray)
        .on_system_tray_event(|app, event| {
            tauri_plugin_positioner::on_tray_event(app, &event);
            match event {
                SystemTrayEvent::LeftClick {
                    position: _,
                    size: _,
                    ..
                } => {
                    // temp solution due to a limitation in libappindicator and tray events in linux
                    let window = app.get_window("main").unwrap();
                    #[cfg(target_os = "linux")]
                    let device_state = DeviceState::new();
                    let mouse = device_state.get_mouse();
                    {
                        let mouse_position = mouse.coords;
                        move_window_to_mouse_position(&window, mouse_position);
                        println!("Position: {:#?}", mouse_position);
                    }
                    #[cfg(target_os = "windows")]
                    let _ = window.move_window(Position::BottomRight);
                    #[cfg(target_os = "macos")]
                    let _ = window.move_window(Position::TrayCenter);

                    if window.is_visible().unwrap() {
                        window.hide().unwrap();
                    } else {
                        window.show().unwrap();
                        window.set_focus().unwrap();
                    }
                    window
                        .eval(inject_script)
                        .map_err(|err| println!("{:?}", err))
                        .ok();
                }
                SystemTrayEvent::RightClick {
                    position: _,
                    size: _,
                    ..
                } => {
                    println!("system tray received a right click");
                }
                SystemTrayEvent::DoubleClick {
                    position: _,
                    size: _,
                    ..
                } => {
                    println!("system tray received a double click");
                }
                SystemTrayEvent::MenuItemClick { id, .. } => match id.as_str() {
                    "quit" => {
                        let runtime = Runtime::new().expect("Failed to create a Tokio runtime");

                        runtime.block_on(async {
                            match kubeforward::port_forward::stop_all_port_forward().await {
                                Ok(_) => {
                                    println!("Successfully stopped all port forwards.");
                                }
                                Err(err) => {
                                    eprintln!("Failed to stop port forwards: {}", err);
                                }
                            }
                        });

                        std::process::exit(0);
                    }
                    "toggle" => {
                        let window = app.get_window("main").unwrap();
                        let device_state = DeviceState::new();
                        let mouse = device_state.get_mouse();
                        {
                            let mouse_position = mouse.coords;
                            move_window_to_mouse_position(&window, mouse_position);
                            println!("Position: {:#?}", mouse_position);
                        }
                        
                        if window.is_visible().unwrap() {
                            window.hide().unwrap();
                        } else {
                            window.show().unwrap();
                            window.set_focus().unwrap();
                        }
                        window
                            .eval(inject_script)
                            .map_err(|err| println!("{:?}", err))
                            .ok();
                    }
                    "hide" => {
                        let window = app.get_window("main").unwrap();
                        window.hide().unwrap();
                    }
                    _ => {}
                },
                _ => {}
            }
        })
        .on_window_event(|event| {
            if let tauri::WindowEvent::Focused(is_focused) = event.event() {
                if !is_focused {
                    let app_handle = event.window().app_handle();
                    if let Some(state) = app_handle.try_state::<SaveDialogState>() {
                        if !state.is_open.load(Ordering::SeqCst) {
                            event.window().hide().unwrap();
                        }
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            kubeforward::port_forward::start_port_forward,
            kubeforward::port_forward::stop_port_forward,
            kubeforward::port_forward::stop_all_port_forward,
            kubeforward::kubecontext::list_kube_contexts,
            kubeforward::kubecontext::list_namespaces,
            kubeforward::kubecontext::list_services,
            kubeforward::kubecontext::list_service_ports,
            kubeforward::proxy::deploy_and_forward_pod,
            kubeforward::proxy::stop_proxy_forward,
            config::get_configs,
            config::insert_config,
            config::delete_config,
            config::get_config,
            config::update_config,
            config::export_configs,
            config::import_configs,
            config::delete_configs,
            config::delete_all_configs,
            commands::open_save_dialog,
            commands::close_save_dialog,
            commands::import_configs_from_github,
            keychain::store_key,
            keychain::get_key,
            keychain::delete_key
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
