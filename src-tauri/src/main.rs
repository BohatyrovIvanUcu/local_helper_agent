// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(target_os = "linux")]
    {
        // У GNOME на Wayland стандартний протокол xdg-shell блокує always-on-top для клієнтських вікон.
        // GDK_BACKEND=x11 перемикає вікно на XWayland, де _NET_WM_STATE_ABOVE гарантовано тримає
        // маскота поверх навіть повноекранних/максимізованих вікон (VS Code, браузерів тощо).
        if std::env::var("GDK_BACKEND").is_err() {
            std::env::set_var("GDK_BACKEND", "x11");
        }
    }

    local_helper_ai_lib::run()
}

