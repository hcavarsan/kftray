use kftray_commons::models::window::AppState;
use ksni::{
    Icon,
    TrayMethods,
    menu::{
        MenuItem,
        StandardItem,
        SubMenu,
    },
};
use log::{
    error,
    info,
    warn,
};
use tauri::{
    AppHandle,
    Manager,
    Wry,
};
use tauri_plugin_positioner::Position;

use crate::commands::portforward::handle_exit_app;
use crate::commands::window_state::toggle_pin_state;
use crate::window::{
    apply_window_size_preset,
    reset_window_position,
    set_window_position,
    toggle_window_visibility,
};
use crate::window_size::WindowSizePreset;

const TRAY_PNG_VARIANTS: &[&[u8]] = &[
    include_bytes!("../icons/tray-16.png"),
    include_bytes!("../icons/tray-22.png"),
    include_bytes!("../icons/tray-24.png"),
    include_bytes!("../icons/tray-32.png"),
    include_bytes!("../icons/tray-48.png"),
    include_bytes!("../icons/tray-64.png"),
];

struct KftrayTray {
    app: AppHandle<Wry>,
    icon: Vec<Icon>,
}

impl ksni::Tray for KftrayTray {
    fn id(&self) -> String {
        "kftray".into()
    }

    fn title(&self) -> String {
        "kftray".into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        self.icon.clone()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        toggle_main_window(&self.app);
    }

    fn secondary_activate(&mut self, _x: i32, _y: i32) {
        toggle_main_window(&self.app);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Toggle App".into(),
                activate: Box::new(|t: &mut Self| toggle_main_window(&t.app)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Pin Window".into(),
                activate: Box::new(|t: &mut Self| {
                    if let Some(window) = t.app.get_webview_window("main") {
                        toggle_pin_state(t.app.state::<AppState>(), window);
                    }
                }),
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: "Set Window Position".into(),
                submenu: vec![
                    position_item("Center", || Position::Center),
                    position_item("Top Right", || Position::TopRight),
                    position_item("Bottom Right", || Position::BottomRight),
                    position_item("Bottom Left", || Position::BottomLeft),
                    position_item("Top Left", || Position::TopLeft),
                    MenuItem::Separator,
                    StandardItem {
                        label: "Reset Position".into(),
                        activate: Box::new(|t: &mut Self| {
                            if let Some(window) = t.app.get_webview_window("main") {
                                reset_window_position(window);
                            }
                        }),
                        ..Default::default()
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: "Set Window Size".into(),
                submenu: vec![
                    size_item("Default", WindowSizePreset::Default),
                    size_item("Medium", WindowSizePreset::Medium),
                    size_item("Large", WindowSizePreset::Large),
                    size_item("Extra Large", WindowSizePreset::ExtraLarge),
                ],
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "View Logs".into(),
                activate: Box::new(|t: &mut Self| {
                    if let Err(e) = crate::commands::logs::open_log_viewer_window(t.app.clone()) {
                        error!("Failed to open log viewer window: {e}");
                    }
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Quit".into(),
                activate: Box::new(|t: &mut Self| {
                    let handle = t.app.clone();
                    tauri::async_runtime::spawn(async move {
                        handle_exit_app(handle).await;
                    });
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

fn position_item(label: &str, make: fn() -> Position) -> MenuItem<KftrayTray> {
    StandardItem {
        label: label.into(),
        activate: Box::new(move |t: &mut KftrayTray| {
            if let Some(window) = t.app.get_webview_window("main") {
                set_window_position(&window, make());
            }
        }),
        ..Default::default()
    }
    .into()
}

fn size_item(label: &str, preset: WindowSizePreset) -> MenuItem<KftrayTray> {
    StandardItem {
        label: label.into(),
        activate: Box::new(move |t: &mut KftrayTray| {
            if let Some(window) = t.app.get_webview_window("main") {
                let app_state = window.state::<AppState>();
                let runtime = app_state.runtime.clone();
                runtime.spawn(async move {
                    apply_window_size_preset(&window, preset).await;
                });
            }
        }),
        ..Default::default()
    }
    .into()
}

fn toggle_main_window(app: &AppHandle<Wry>) {
    match app.get_webview_window("main") {
        Some(window) => toggle_window_visibility(&window),
        None => error!("Main window not found on tray activation"),
    }
}

pub fn spawn(app: &tauri::App<Wry>) {
    let handle = app.handle().clone();
    let icon: Vec<Icon> = TRAY_PNG_VARIANTS
        .iter()
        .filter_map(|bytes| decode_tray_icon(bytes))
        .collect();
    let tray = KftrayTray { app: handle, icon };

    tauri::async_runtime::spawn(async move {
        match tray.spawn().await {
            Ok(handle) => {
                info!("SNI tray service started");
                std::mem::forget(handle);
            }
            Err(e) => {
                warn!(
                    "SNI tray service failed to start ({e}); kftray will run without a tray icon"
                );
            }
        }
    });
}

fn decode_tray_icon(bytes: &[u8]) -> Option<Icon> {
    let decoder = png::Decoder::new(bytes);
    let mut reader = match decoder.read_info() {
        Ok(reader) => reader,
        Err(e) => {
            warn!("Failed to decode tray icon PNG header: {e}");
            return None;
        }
    };

    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = match reader.next_frame(&mut buf) {
        Ok(info) => info,
        Err(e) => {
            warn!("Failed to decode tray icon PNG frame: {e}");
            return None;
        }
    };

    let rgba = to_rgba8(&buf[..info.buffer_size()], info.color_type, info.bit_depth)?;
    Some(Icon {
        width: info.width as i32,
        height: info.height as i32,
        data: rgba_to_argb(&rgba),
    })
}

fn to_rgba8(buf: &[u8], color: png::ColorType, bit_depth: png::BitDepth) -> Option<Vec<u8>> {
    if bit_depth != png::BitDepth::Eight {
        warn!("Unsupported tray icon bit depth: {bit_depth:?}");
        return None;
    }
    match color {
        png::ColorType::Rgba => Some(buf.to_vec()),
        png::ColorType::Rgb => {
            let mut out = Vec::with_capacity(buf.len() / 3 * 4);
            for px in buf.chunks_exact(3) {
                out.extend_from_slice(&[px[0], px[1], px[2], 0xFF]);
            }
            Some(out)
        }
        other => {
            warn!("Unsupported tray icon color type: {other:?}");
            None
        }
    }
}

fn rgba_to_argb(rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len());
    for px in rgba.chunks_exact(4) {
        let a = px[3] as u16;
        let r = ((px[0] as u16 * a + 127) / 255) as u8;
        let g = ((px[1] as u16 * a + 127) / 255) as u8;
        let b = ((px[2] as u16 * a + 127) / 255) as u8;
        out.extend_from_slice(&[a as u8, r, g, b]);
    }
    out
}
