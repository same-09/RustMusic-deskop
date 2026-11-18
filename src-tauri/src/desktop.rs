//! Bagian desktop: ikon tray, tombol media global (play/pause, berikutnya, sebelumnya),
//! dan "tutup ke tray" supaya musik terus jalan saat jendela ditutup.
//!
//! Tombol media dan menu tray tidak memutar apa-apa sendiri. Keduanya mengirim event
//! `media-key` ("toggle" | "next" | "prev" | "stop") ke UI, dan UI yang mengendalikan pemutar.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::plugin::TauriPlugin;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Runtime, Window, WindowEvent};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Shortcut, ShortcutState};

const TRAY_ID: &str = "main";
const MEDIA_KEYS: [Code; 4] = [
    Code::MediaPlayPause,
    Code::MediaTrackNext,
    Code::MediaTrackPrevious,
    Code::MediaStop,
];

/// Tray sudah berhasil dibuat. Tanpa tray, jendela tidak boleh disembunyikan (tidak ada jalan kembali).
static TRAY_OK: AtomicBool = AtomicBool::new(false);
static CLOSE_TO_TRAY: AtomicBool = AtomicBool::new(true);

fn action_of(code: Code) -> Option<&'static str> {
    match code {
        Code::MediaPlayPause => Some("toggle"),
        Code::MediaTrackNext => Some("next"),
        Code::MediaTrackPrevious => Some("prev"),
        Code::MediaStop => Some("stop"),
        _ => None,
    }
}

/// Plugin global shortcut dengan penangan yang meneruskan tombol media ke UI.
pub fn media_plugin<R: Runtime>() -> TauriPlugin<R> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            if let Some(action) = action_of(shortcut.key) {
                let _ = app.emit("media-key", action);
            }
        })
        .build()
}

/// Daftarkan tombol media. Galat (tombol dipakai app lain) diabaikan.
pub fn register_media_keys(app: &AppHandle) {
    let gs = app.global_shortcut();
    for code in MEDIA_KEYS {
        let _ = gs.register(Shortcut::new(None, code));
    }
}

#[tauri::command]
pub fn set_media_keys(app: AppHandle, on: bool) {
    let gs = app.global_shortcut();
    for code in MEDIA_KEYS {
        let sc = Shortcut::new(None, code);
        if on {
            if !gs.is_registered(sc) {
                let _ = gs.register(sc);
            }
        } else {
            let _ = gs.unregister(sc);
        }
    }
}

#[tauri::command]
pub fn set_close_to_tray(on: bool) {
    CLOSE_TO_TRAY.store(on, Ordering::Relaxed);
}

/// Teks yang muncul saat kursor di atas ikon tray (judul lagu yang sedang diputar).
#[tauri::command]
pub fn set_tooltip(app: AppHandle, text: String) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let short: String = text.chars().take(120).collect();
        let _ = tray.set_tooltip(Some(short));
    }
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Buat ikon tray + menu. Kiri-klik membuka jendela; kanan-klik membuka menu.
pub fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Buka RustMusic", true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", "Putar / Jeda", true, None::<&str>)?;
    let prev = MenuItem::with_id(app, "prev", "Sebelumnya", true, None::<&str>)?;
    let next = MenuItem::with_id(app, "next", "Berikutnya", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Keluar", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&show, &sep, &toggle, &prev, &next, &sep, &quit])?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("RustMusic")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "quit" => app.exit(0),
            other => {
                let _ = app.emit("media-key", other);
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    TRAY_OK.store(true, Ordering::Relaxed);
    Ok(())
}

/// Tutup jendela utama = sembunyikan ke tray (bila tray ada dan pengaturan aktif).
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        if window.label() == "main"
            && TRAY_OK.load(Ordering::Relaxed)
            && CLOSE_TO_TRAY.load(Ordering::Relaxed)
        {
            api.prevent_close();
            let _ = window.hide();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_codes_map_to_actions() {
        assert_eq!(action_of(Code::MediaPlayPause), Some("toggle"));
        assert_eq!(action_of(Code::MediaTrackNext), Some("next"));
        assert_eq!(action_of(Code::MediaTrackPrevious), Some("prev"));
        assert_eq!(action_of(Code::KeyA), None);
    }
}
