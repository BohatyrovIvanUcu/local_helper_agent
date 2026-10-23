// Модуль керування головним вікном маскота (Always-on-top, інтерактивність Wayland/X11, розміри)

use tauri::{AppHandle, Manager};
use crate::media::open_browser_detached;

/// Відкриває зовнішнє посилання у системному браузері за замовчуванням.
#[tauri::command]
pub async fn open_external_url(url: String) -> Result<(), String> {
    let clean = url.trim();
    if clean.starts_with("http://") || clean.starts_with("https://") {
        open_browser_detached(clean);
        Ok(())
    } else {
        Err("Некоректний URL для відкриття".to_string())
    }
}

/// Змінює розмір вікна маскота між компактним (280x240) та розгорнутим (380x440).
#[tauri::command]
pub async fn set_window_expanded(app: AppHandle, expanded: bool) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        let (w, h) = if expanded {
            (380.0, 440.0)
        } else {
            (280.0, 258.0)
        };
        let _ = window.set_size(tauri::LogicalSize::new(w, h));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn ensure_linux_window_interactive(window: &tauri::WebviewWindow) {
    use gtk::prelude::*;
    let _ = window.set_ignore_cursor_events(false);
    let _ = window.set_always_on_top(true);
    let _ = window.set_visible_on_all_workspaces(true);
    if let Ok(gtk_win) = window.gtk_window() {
        gtk_win.set_sensitive(true);
        gtk_win.set_accept_focus(true);
        gtk_win.input_shape_combine_region(None);
        gtk_win.set_keep_above(true);
        gtk_win.stick();
    }
}

