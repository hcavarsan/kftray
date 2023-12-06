#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod db;
use std::sync::{Arc, Mutex};
use tauri::{CustomMenuItem, Manager, SystemTray, SystemTrayEvent, SystemTrayMenu};
use tauri_plugin_positioner::{Position, WindowExt};

use std::env;

use std::collections::HashMap;

use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use rusqlite::{params, Connection, Result};
use serde_json::json;

#[derive(Clone, Deserialize, PartialEq, Serialize, Debug)]
struct Config {
    id: Option<i64>,
    service: String,
    namespace: String,
    local_port: u16,
    remote_port: u16,
    context: String,
}

#[derive(serde::Serialize)]
struct CustomResponse {
    id: Option<i64>,
    service: String,
    namespace: String,
    local_port: u16,
    remote_port: u16,
    context: String,
    stdout: String,
    stderr: String,
    status: i32,
}

lazy_static! {
    static ref CHILD_PROCESSES: Arc<Mutex<HashMap<String, Mutex<Child>>>> =
        Arc::new(Mutex::new(HashMap::new()));
}

#[tauri::command]
async fn delete_config(id: i64) -> Result<(), String> {
    println!("Deleting config with id: {}", id);
    let home_dir = dirs::home_dir().unwrap();
    let db_dir = home_dir.to_str().unwrap().to_string() + "/.kftray/configs.db";
    let conn = match Connection::open(db_dir) {
        Ok(conn) => conn,
        Err(e) => return Err(format!("Failed to open database: {}", e)),
    };

    match conn.execute("DELETE FROM configs WHERE id=?1", params![id]) {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to delete config: {}", e)),
    }
}
#[tauri::command]
fn insert_config(config: Config) -> Result<(), String> {
    let home_dir = dirs::home_dir().unwrap();
    let db_dir = home_dir.to_str().unwrap().to_string() + "/.kftray/configs.db";

    let conn = Connection::open(db_dir).map_err(|e| e.to_string())?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS configs (
                  id INTEGER PRIMARY KEY,
                  data TEXT NOT NULL
                  )",
        params![],
    )
    .map_err(|e| e.to_string())?;

    let data = json!(config).to_string();
    conn.execute("INSERT INTO configs (data) VALUES (?1)", params![data])
        .map_err(|e| e.to_string())?;

    Ok(())
}

fn read_configs() -> Result<Vec<Config>, rusqlite::Error> {
    let home_dir = dirs::home_dir().unwrap();
    let db_dir = home_dir.to_str().unwrap().to_string() + "/.kftray/configs.db";
    let conn = Connection::open(db_dir)?;

    let mut stmt = conn.prepare("SELECT id, data FROM configs")?;
    let rows = stmt.query_map(params![], |row| {
        let id: i64 = row.get(0)?;
        let data: String = row.get(1)?;
        let mut config: Config =
            serde_json::from_str(&data).map_err(|_| rusqlite::Error::QueryReturnedNoRows)?;
        config.id = Some(id);
        Ok(config)
    })?;

    let mut configs = Vec::new();

    for row in rows {
        configs.push(row?);
    }
    println!("Reading configs {:?}", configs);
    Ok(configs)
}
#[tauri::command]
async fn port_forward() -> Result<Vec<CustomResponse>, String> {
    println!("Starting port_forward function");
    let configs = read_configs().map_err(|e| e.to_string())?;
    let mut responses = Vec::new();

    for config in configs {
        println!("Processing config: {:?}", config);
        let child_result = Command::new("kubectl")
            .args(&[
                "port-forward",
                "-n",
                &config.namespace,
                "--context",
                &config.context,
                &format!("svc/{}", config.service),
                &format!("{}:{}", config.local_port, config.remote_port),
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();

        match child_result {
            Ok(child) => {
                println!("Successfully started child process");
                thread::sleep(Duration::from_secs(1));
                CHILD_PROCESSES
                    .lock()
                    .unwrap()
                    .insert(config.service.clone(), Mutex::new(child));
                responses.push(CustomResponse {
                    id: Some(config.id.unwrap_or(0)),
                    service: config.service.clone(),
                    namespace: config.namespace.clone(),
                    local_port: config.local_port,
                    remote_port: config.remote_port,
                    context: config.context.clone(),
                    stdout: format!(
                        "Forwarding from 127.0.0.1:{} -> {}",
                        config.local_port, config.remote_port
                    ),
                    stderr: String::new(),
                    status: 0,
                });
            }
            Err(e) => {
                println!("Failed to start child process: {}", e);
                thread::sleep(Duration::from_secs(1));
                responses.push(CustomResponse {
                    id: Some(config.id.unwrap_or(0)),
                    service: config.service.clone(),
                    namespace: config.namespace.clone(),
                    local_port: config.local_port,
                    remote_port: config.remote_port,
                    context: config.context.clone(),
                    stdout: String::new(),
                    stderr: format!("Failed to execute command: {}", e),
                    status: 1,
                });
            }
        }
    }
    println!("Finished port_forward function");
    Ok(responses)
}

#[tauri::command]
fn kill_all_processes() -> Result<(), String> {
    println!("kill_all_processes called");
    let mut child_processes = CHILD_PROCESSES.lock().unwrap();
    for (_, child) in child_processes.iter() {
        child.lock().unwrap().kill().map_err(|e| e.to_string())?;
    }
    child_processes.clear();
    Ok(())
}

#[tauri::command]
async fn stop_port_forward() -> Result<Vec<CustomResponse>, String> {
    println!("kill_process called");
    let mut responses = Vec::new();
    let mut global_child = CHILD_PROCESSES.lock().unwrap();
    let configs = read_configs().map_err(|e| e.to_string())?;

    for (service, child_mutex) in global_child.drain() {
        let config = configs
            .iter()
            .find(|config| config.service == service)
            .unwrap();
        let mut child = child_mutex.lock().unwrap();
        match child.kill() {
            Ok(_) => {
                // Wait for the process to terminate
                let _ = child.wait();
                responses.push(CustomResponse {
                    id: Some(config.id.unwrap_or(0)),
                    service: config.service.clone(),
                    namespace: config.namespace.clone(),
                    local_port: config.local_port,
                    remote_port: config.remote_port,
                    context: config.context.clone(),
                    stdout: format!(
                        "Forwarding from 127.0.0.1:{} -> {}",
                        config.local_port, config.remote_port
                    ),
                    stderr: String::new(),
                    status: 0,
                });
            }
            Err(e) => {
                responses.push(CustomResponse {
                    id: Some(config.id.unwrap_or(0)),
                    service: config.service.clone(),
                    namespace: config.namespace.clone(),
                    local_port: config.local_port,
                    remote_port: config.remote_port,
                    context: config.context.clone(),
                    stdout: String::new(),
                    stderr: format!("Failed to execute command: {}", e),
                    status: 1,
                });
            }
        }
    }

    Ok(responses)
}
#[tauri::command]
fn quit_app(window: tauri::Window) {
    println!("quit_app called");
    window.close().unwrap();
    let _ = kill_all_processes();
}

#[tauri::command]
async fn get_configs() -> Result<Vec<Config>, String> {
    println!("get_configs called");
    let configs = read_configs().map_err(|e| e.to_string())?;
    println!("{:?}", configs);
    return Ok(configs);
}

fn main() {
    let _ = fix_path_env::fix();
    let quit = CustomMenuItem::new("quit".to_string(), "Quit").accelerator("Cmd+Q");
    let system_tray_menu = SystemTrayMenu::new().add_item(quit);
    tauri::Builder::default()
        .setup(|app| {
            // Initialize the database.
            db::init();
            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }
            Ok(())
        })
        .plugin(tauri_plugin_positioner::init())
        .system_tray(SystemTray::new().with_menu(system_tray_menu))
        .on_system_tray_event(|app, event| {
            tauri_plugin_positioner::on_tray_event(app, &event);
            match event {
                SystemTrayEvent::LeftClick { .. } => {
                    if let Some(window) = app.get_window("main") {
                        let _ = window.move_window(Position::TrayCenter);
                        match window.is_visible() {
                            Ok(true) => {
                                if let Err(e) = window.hide() {
                                    println!("Failed to hide window: {}", e);
                                }
                            }
                            Ok(false) => {
                                if let Err(e) = window.show() {
                                    println!("Failed to show window: {}", e);
                                }
                                if let Err(e) = window.set_focus() {
                                    println!("Failed to set focus: {}", e);
                                }
                            }
                            Err(e) => {
                                println!("Failed to check window visibility: {}", e);
                            }
                        }
                    }
                }
                SystemTrayEvent::RightClick { .. } => {
                    println!("system tray received a right click");
                }
                SystemTrayEvent::DoubleClick { .. } => {
                    println!("system tray received a double click");
                }
                SystemTrayEvent::MenuItemClick { id, .. } => match id.as_str() {
                    "quit" => {
                        std::process::exit(0);
                    }
                    "hide" => {
                        if let Some(window) = app.get_window("main") {
                            let _ = window.hide();
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        })
        .on_window_event(|event| {
            if let tauri::WindowEvent::Focused(is_focused) = event.event() {
                // detect click outside of the focused window and hide the app
                if !is_focused {
                    let _ = event.window().hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            port_forward,
            stop_port_forward,
            quit_app,
            get_configs,
            insert_config,
            delete_config
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
