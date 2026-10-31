// Головна точка входу Tauri бекенду: реєстрація модулів, налаштування вікна та трею

pub mod audio;
pub mod clipboard;
pub mod config;
pub mod converter;
pub mod disk;
pub mod display;
pub mod downloads;
pub mod files;
pub mod media;
pub mod memory;
pub mod obsidian;
pub mod presets;
pub mod reminders;
pub mod share;
pub mod voice;
pub mod web;
pub mod window;
pub mod workspace;

// Re-exports для CLI (main.rs) та зовнішніх модулів
pub use config::{
    get_home_dir, get_user_config_path, read_app_config, update_config_value, AppConfig,
};

use std::time::Duration;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            if let Some(w) = app.get_webview_window("main") {
                // У Mutter (GNOME Wayland + XWayland) негайний виклик _NET_WM_STATE_ABOVE до завершення
                // прив'язки wl_surface викликає збій meta_window_set_stack_position_no_sync (stack_position == -1),
                // через що вікно рендериться, але стає неклікабельним (події миші проходять крізь нього).
                // Тому застосовуємо always-on-top, sticky та скидання input_shape вже після мапінгу вікна.
                #[cfg(target_os = "linux")]
                {
                    for delay_ms in [350_u64, 1200_u64, 3000_u64] {
                        let w_delayed = w.clone();
                        gtk::glib::timeout_add_local_once(
                            Duration::from_millis(delay_ms),
                            move || {
                                window::ensure_linux_window_interactive(&w_delayed);
                            },
                        );
                    }
                }

                #[cfg(not(target_os = "linux"))]
                {
                    let _ = w.set_ignore_cursor_events(false);
                    let _ = w.set_always_on_top(true);
                    let _ = w.set_visible_on_all_workspaces(true);
                }

                // Забезпечуємо, щоб вікно маскота завжди залишалося поверх усіх інших програм при фокусі
                let startup_instant = std::time::Instant::now();
                let w_focus = w.clone();
                w.on_window_event(move |event| {
                    if startup_instant.elapsed() < Duration::from_millis(1200) {
                        return;
                    }
                    if let tauri::WindowEvent::Focused(_) = event {
                        let _ = w_focus.set_ignore_cursor_events(false);
                        let _ = w_focus.set_always_on_top(true);
                    }
                });
            }

            // Фоновий планувальник нагадувань і таймерів
            reminders::start_reminders_scheduler(app.handle().clone());

            // Додаємо іконку в системний трей для зручного керування та виходу
            let toggle_item =
                MenuItem::with_id(app, "toggle", "Показати / Сховати", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Вийти", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&toggle_item, &quit_item])?;

            let mut tray_builder = TrayIconBuilder::new()
                .menu(&menu)
                .tooltip("Local Helper Mascot")
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "toggle" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let is_visible = w.is_visible().unwrap_or(true);
                            if is_visible {
                                let _ = w.hide();
                            } else {
                                let _ = w.show();
                                let _ = w.set_focus();
                                #[cfg(target_os = "linux")]
                                window::ensure_linux_window_interactive(&w);
                            }
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                });

            if let Some(icon) = app.default_window_icon() {
                tray_builder = tray_builder.icon(icon.clone());
            }

            let _tray = tray_builder.build(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            obsidian::append_to_obsidian,
            obsidian::read_obsidian_note,
            obsidian::create_or_update_obsidian_note,
            obsidian::search_obsidian_notes,
            obsidian::list_obsidian_notes,
            obsidian::get_or_create_daily_note,
            obsidian::complete_obsidian_todo,
            files::create_or_update_file,
            files::read_file_or_dir,
            files::run_cli_command,
            voice::start_voice_recording,
            voice::stop_voice_recording,
            voice::cancel_voice_recording,
            clipboard::read_image_base64,
            clipboard::read_clipboard_image,
            clipboard::write_clipboard_text,
            config::get_app_config,
            media::control_media,
            memory::get_agent_memory,
            memory::append_conversation_turn,
            memory::manage_memory,
            reminders::manage_reminders,
            media::is_music_playing,
            display::control_display,
            web::summarize_webpage,
            web::fetch_image_from_url,
            window::open_external_url,
            window::set_window_expanded,
            workspace::get_active_workspace,
            converter::convert_file,
            converter::open_file,
            converter::reveal_in_file_manager,
            converter::get_available_conversion_formats,
            disk::scan_disk_space,
            disk::delete_disk_item,
            disk::open_disk_analyzer_window,
            share::start_file_share,
            share::stop_file_share,
            share::get_share_status,
            share::open_share_window,
            downloads::analyze_downloads,
            downloads::execute_downloads_cleanup,
            presets::manage_session_presets,
            display::lock_screen
        ])
        .run(tauri::generate_context!())
        .expect("Помилка запуску Tauri-застосунку");
}
