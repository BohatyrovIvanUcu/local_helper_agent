// Модуль читання зображень з диска та системного буфера обміну (GTK/Clipboard)

use serde::Serialize;
use std::fs;
use std::time::Duration;
use crate::config::expand_tilde;
use crate::voice::encode_base64;

#[derive(Serialize, Clone, Debug)]
pub struct ClipboardImageResult {
    pub mime_type: String,
    pub base64_data: String,
    pub name: String,
}

pub fn decode_percent(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(val) = u8::from_str_radix(
                std::str::from_utf8(&bytes[i + 1..=i + 2]).unwrap_or(""),
                16,
            ) {
                out.push(val);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

pub fn try_read_image_file_path(path_str: &str) -> Option<ClipboardImageResult> {
    let trimmed = path_str.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut clean = trimmed;
    if clean.starts_with("copy ") || clean.starts_with("copy\t") || clean.starts_with("copy\n") {
        clean = clean[4..].trim_start();
    } else if clean.starts_with("cut ") || clean.starts_with("cut\t") || clean.starts_with("cut\n") {
        clean = clean[3..].trim_start();
    }
    let clean = clean.trim_start_matches("file://");
    let decoded = decode_percent(clean);
    let p = expand_tilde(&decoded).ok()?;
    if !p.exists() || !p.is_file() {
        return None;
    }
    let ext = p.extension()?.to_str()?.to_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        _ => return None,
    };
    let bytes = fs::read(&p).ok()?;
    let name = p.file_name()?.to_string_lossy().to_string();
    Some(ClipboardImageResult {
        mime_type: mime.to_string(),
        base64_data: encode_base64(&bytes),
        name,
    })
}

#[cfg(target_os = "linux")]
pub fn read_gtk_clipboard_on_main_thread() -> Option<ClipboardImageResult> {
    let display = gdk::Display::default()?;
    let clipboard = gtk::Clipboard::default(&display)?;

    // Отримуємо перелік форматів одним запитом, щоб не блокувати головний цикл зайвими перевірками
    let targets = clipboard.wait_for_targets()?;
    let mut has_uris = false;
    let mut has_image = false;
    let mut has_text = false;

    for atom in &targets {
        let name = atom.name();
        let s = name.as_str();
        if s == "text/uri-list" || s == "x-special/gnome-copied-files" {
            has_uris = true;
        } else if s.starts_with("image/") {
            has_image = true;
        } else if s == "UTF8_STRING" || s == "text/plain" || s.starts_with("text/plain;") {
            has_text = true;
        }
    }

    if has_uris {
        for uri in clipboard.wait_for_uris() {
            if let Some(res) = try_read_image_file_path(uri.as_str()) {
                return Some(res);
            }
        }
    }

    if has_image {
        if let Some(pixbuf) = clipboard.wait_for_image() {
            if let Ok(bytes) = pixbuf.save_to_bufferv("png", &[]) {
                if !bytes.is_empty() {
                    return Some(ClipboardImageResult {
                        mime_type: "image/png".to_string(),
                        base64_data: encode_base64(&bytes),
                        name: "screenshot.png".to_string(),
                    });
                }
            }
        }
    }

    if has_text || has_uris {
        if let Some(text) = clipboard.wait_for_text() {
            for line in text.as_str().lines() {
                if let Some(res) = try_read_image_file_path(line) {
                    return Some(res);
                }
            }
        }
    }

    None
}

/// 8. Читає файл зображення з диска та повертає його вміст у Base64.
#[tauri::command]
pub async fn read_image_base64(filepath: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = expand_tilde(&filepath)?;
        if !path.exists() {
            return Err(format!("Файл зображення не існує: {}", path.display()));
        }
        let bytes = fs::read(&path).map_err(|e| {
            format!(
                "Помилка читання файлу зображення '{}': {}",
                path.display(),
                e
            )
        })?;
        Ok(encode_base64(&bytes))
    })
    .await
    .map_err(|e| format!("Помилка потоку: {}", e))?
}

/// 9. Читає зображення з системного буфера обміну всередині процесу через GTK (без сторонніх утиліт).
#[tauri::command]
pub async fn read_clipboard_image(
    app: tauri::AppHandle,
) -> Result<Option<ClipboardImageResult>, String> {
    #[cfg(target_os = "linux")]
    {
        let (tx, rx) = std::sync::mpsc::channel();
        app.run_on_main_thread(move || {
            let res = read_gtk_clipboard_on_main_thread();
            let _ = tx.send(res);
        })
        .map_err(|e| format!("Помилка доступу до буфера обміну: {}", e))?;

        rx.recv_timeout(Duration::from_millis(1500))
            .map_err(|_| "Час очікування буфера обміну вичерпано".to_string())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = app;
        Ok(None)
    }
}

/// 10. Записує текстовий вміст у системний буфер обміну (GTK на Linux, clip на Windows, pbcopy на macOS).
#[tauri::command]
pub async fn write_clipboard_text(
    app: tauri::AppHandle,
    text: String,
) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let (tx, rx) = std::sync::mpsc::channel();
        app.run_on_main_thread(move || {
            let res = (|| -> Option<()> {
                let display = gdk::Display::default()?;
                let clipboard = gtk::Clipboard::default(&display)?;
                clipboard.set_text(&text);
                clipboard.store();
                Some(())
            })();
            let _ = tx.send(res.is_some());
        })
        .map_err(|e| format!("Помилка доступу до буфера обміну: {}", e))?;

        let _ = rx
            .recv_timeout(Duration::from_millis(1500))
            .map_err(|_| "Час очікування буфера обміну вичерпано".to_string())?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        let _ = app;
        tauri::async_runtime::spawn_blocking(move || {
            let mut child = std::process::Command::new("clip")
                .stdin(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| format!("Помилка запуску clip: {}", e))?;
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
            Ok(())
        })
        .await
        .map_err(|e| format!("Помилка фонового потоку: {}", e))?
    }
    #[cfg(target_os = "macos")]
    {
        let _ = app;
        tauri::async_runtime::spawn_blocking(move || {
            let mut child = std::process::Command::new("pbcopy")
                .stdin(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| format!("Помилка запуску pbcopy: {}", e))?;
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
            Ok(())
        })
        .await
        .map_err(|e| format!("Помилка фонового потоку: {}", e))?
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        let _ = app;
        let _ = text;
        Ok(())
    }
}

