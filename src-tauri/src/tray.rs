use tauri::{CustomMenuItem, SystemTray, SystemTrayMenu};

pub fn create_tray_menu() -> SystemTray {
    let quit = CustomMenuItem::new("quit".to_string(), "Quit").accelerator("CmdOrCtrl+Shift+Q");
    let open = CustomMenuItem::new("open".to_string(), "Open App");
    let system_tray_menu = SystemTrayMenu::new().add_item(open).add_item(quit);
    SystemTray::new().with_menu(system_tray_menu)
}
