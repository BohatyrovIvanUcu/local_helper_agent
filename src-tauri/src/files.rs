// Модуль для роботи з локальними файлами, директоріями та безпечним виконанням CLI-команд

use std::env;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;
use crate::config::expand_tilde;

pub const MAX_READ_CHARS: usize = 8_000;
pub const MAX_CLI_OUTPUT_CHARS: usize = 6_000;

/// Безпечно обрізає UTF-8 рядок за кількістю символів (щоб не панікувати на кирилиці).
pub fn truncate_chars(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        text.to_string()
    } else {
        let head: String = text.chars().take(max_chars).collect();
        format!(
            "{}\n\n... [Обрізано: показано {} з {} символів]",
            head, max_chars, count
        )
    }
}

/// Перевіряє, чи не є шлях конфіденційним (SSH, GPG, AWS, /etc/shadow, .env, config.env)
pub fn is_sensitive_path(path: &Path) -> bool {
    let str_rep = path.to_string_lossy().replace('\\', "/");
    let sensitive = [
        ".ssh",
        ".gnupg",
        ".aws",
        "/etc/shadow",
        "config.env",
    ];
    sensitive.iter().any(|s| str_rep.contains(s))
        || str_rep == ".env"
        || str_rep.ends_with("/.env")
        || str_rep.contains("/.env/")
}

/// 2. Створює або повністю перезаписує файл за вказаним шляхом (підтримує `~`).
#[tauri::command(rename_all = "snake_case")]
pub async fn create_or_update_file(filepath: String, content: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = expand_tilde(&filepath)?;

        if is_sensitive_path(&path) {
            return Err(format!("Доступ заборонено політикою безпеки: заборонено запис у чутливі шляхи ({})", path.display()));
        }

        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|e| {
                    format!(
                        "Не вдалося створити батьківські папки '{}': {}",
                        parent.display(),
                        e
                    )
                })?;
            }
        }

        // Якщо файл уже існує, перед перезаписом зберігаємо резервну копію .bak
        if path.exists() && path.is_file() {
            let bak_path = format!("{}.bak", path.display());
            let _ = fs::copy(&path, bak_path);
        }

        fs::write(&path, content.as_bytes())
            .map_err(|e| format!("Помилка запису файлу '{}': {}", path.display(), e))?;

        Ok(format!(
            "Файл успішно збережено ({} байтів): {}",
            content.len(),
            path.display()
        ))
    })
    .await
    .map_err(|e| format!("Помилка потоку: {}", e))?
}

/// 3. Читає вміст файлу або список елементів директорії (підтримує `~`).
#[tauri::command(rename_all = "snake_case")]
pub async fn read_file_or_dir(target_path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = expand_tilde(&target_path)?;

        if is_sensitive_path(&path) {
            return Err(format!("Доступ заборонено політикою безпеки: заборонено читання чутливих шляхів ({})", path.display()));
        }

        if !path.exists() {
            return Err(format!("Шлях не існує: {}", path.display()));
        }

        if path.is_dir() {
            let entries = fs::read_dir(&path).map_err(|e| {
                format!("Не вдалося прочитати директорію '{}': {}", path.display(), e)
            })?;

            let mut items: Vec<String> = Vec::new();
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
                let prefix = if is_dir { "[DIR] " } else { "[FILE]" };
                items.push(format!("{} {}", prefix, name));
            }
            items.sort();

            if items.is_empty() {
                Ok(format!("Директорія порожня: {}", path.display()))
            } else {
                Ok(format!(
                    "Вміст директорії {} ({} елементів):\n{}",
                    path.display(),
                    items.len(),
                    items.join("\n")
                ))
            }
        } else {
            let raw_content = fs::read_to_string(&path)
                .map_err(|e| format!("Не вдалося прочитати файл '{}': {}", path.display(), e))?;
            Ok(truncate_chars(&raw_content, MAX_READ_CHARS))
        }
    })
    .await
    .map_err(|e| format!("Помилка потоку: {}", e))?
}

/// Визначає, чи команда запускає десктопну GUI-програму / браузер або фоновий процес.
pub fn is_gui_or_detached_command(cmd_str: &str) -> bool {
    let trimmed = cmd_str.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.ends_with('&') {
        return true;
    }

    const GUI_BINARIES: &[&str] = &[
        "open",
        "xdg-open",
        "sensible-browser",
        "x-www-browser",
        "gnome-open",
        "kde-open",
        "firefox",
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
        "brave",
        "brave-browser",
        "vivaldi",
        "opera",
        "nautilus",
        "dolphin",
        "thunar",
        "nemo",
        "pcmanfm",
        "code",
        "vscode",
        "codium",
        "vscodium",
        "obsidian",
        "gedit",
        "kate",
        "mousepad",
        "xed",
        "leafpad",
        "subl",
        "telegram-desktop",
        "telegram",
        "discord",
        "slack",
        "skype",
        "thunderbird",
        "spotify",
        "vlc",
        "mpv",
        "rhythmbox",
        "gnome-terminal",
        "konsole",
        "alacritty",
        "kitty",
        "tilix",
        "terminator",
        "gnome-calculator",
        "kcalc",
        "calc",
        "libreoffice",
        "soffice",
        "gimp",
        "inkscape",
        "steam",
        "dbeaver",
        "dbeaver-ce",
        "postman",
        "obs",
        "obs-studio",
        "gtk-launch",
    ];

    // Розбиваємо команду на окремі сегменти по розділювачах ;, &&, ||, |
    let segments = cmd_str.split(|c| c == ';' || c == '|' || c == '&');
    for segment in segments {
        let seg = segment.trim();
        if seg.is_empty() {
            continue;
        }
        let lower = seg.to_lowercase();
        // Якщо сегмент починається з nohup або setsid — це фоновий/від'єднаний процес
        if lower.starts_with("nohup ") || lower.starts_with("setsid ") || lower.starts_with("disown") {
            return true;
        }

        // Витягуємо токени сегмента
        let tokens: Vec<&str> = seg.split_whitespace().collect();
        if tokens.is_empty() {
            continue;
        }

        // Пропускаємо префікси env, sudo, змінні середовища VAR=val
        let mut idx = 0;
        while idx < tokens.len() {
            let tok = tokens[idx];
            if tok == "nohup" || tok == "setsid" || tok == "exec" {
                return true;
            }
            if tok.contains('=') && !tok.starts_with('-') {
                idx += 1;
                continue;
            }
            break;
        }

        if idx >= tokens.len() {
            continue;
        }

        let bin_token = tokens[idx].to_lowercase();
        let bin_name = Path::new(&bin_token)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(&bin_token);

        if bin_name == "gio" && tokens.get(idx + 1).map(|t| t.to_lowercase()) == Some("open".to_string()) {
            return true;
        }

        if GUI_BINARIES.contains(&bin_name) {
            return true;
        }
    }

    false
}

/// 4. Виконує CLI-команду в оболонці ОС без блокування UI-потоку та без зависання на дочірніх GUI-процесах.
#[tauri::command(rename_all = "snake_case")]
pub async fn run_cli_command(command: String, working_dir: Option<String>) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let trimmed_cmd = command.trim();
        if trimmed_cmd.is_empty() {
            return Err("Команда не може бути порожньою".to_string());
        }

        let is_gui = is_gui_or_detached_command(trimmed_cmd);

        let mut cmd = if cfg!(target_os = "windows") {
            let mut c = Command::new("cmd");
            c.args(["/C", trimmed_cmd]);
            c
        } else {
            let mut c = Command::new("sh");
            c.args(["-c", trimmed_cmd]);
            c
        };

        if let Some(dir_str) = working_dir {
            let trimmed_dir = dir_str.trim();
            if !trimmed_dir.is_empty() {
                let resolved_dir = expand_tilde(trimmed_dir)?;
                if !resolved_dir.exists() {
                    return Err(format!(
                        "Робоча директорія не існує: {}",
                        resolved_dir.display()
                    ));
                }
                cmd.current_dir(resolved_dir);
            }
        }

        // Використовуємо тимчасові файли замість Stdio::piped(), щоб дочірні процеси (наприклад, браузер,
        // запущений через xdg-open) не утримували відкритий pipe дескриптор і не блокували очікування.
        let uid = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let stdout_path = env::temp_dir().join(format!("local_helper_out_{}.tmp", uid));
        let stderr_path = env::temp_dir().join(format!("local_helper_err_{}.tmp", uid));

        let out_file = fs::File::create(&stdout_path)
            .map_err(|e| format!("Не вдалося створити тимчасовий файл stdout: {}", e))?;
        let err_file = fs::File::create(&stderr_path)
            .map_err(|e| format!("Не вдалося створити тимчасовий файл stderr: {}", e))?;

        cmd.stdin(Stdio::null())
            .stdout(Stdio::from(out_file))
            .stderr(Stdio::from(err_file));

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Помилка запуску команди '{}': {}", trimmed_cmd, e))?;

        // Для GUI/браузерів чекаємо до 1.2 с, для звичайних CLI-команд — до 10 с.
        let max_polls = if is_gui { 30 } else { 250 };
        let mut exit_status = None;

        for _ in 0..max_polls {
            match child.try_wait() {
                Ok(Some(status)) => {
                    exit_status = Some(status);
                    break;
                }
                Ok(None) => {
                    thread::sleep(Duration::from_millis(40));
                }
                Err(_) => break,
            }
        }

        let stdout = fs::read_to_string(&stdout_path).unwrap_or_default();
        let stderr = fs::read_to_string(&stderr_path).unwrap_or_default();
        let _ = fs::remove_file(&stdout_path);
        let _ = fs::remove_file(&stderr_path);

        if let Some(status) = exit_status {
            let mut status_code = status.code().unwrap_or(0);
            let mut final_stdout = stdout;
            let mut final_stderr = stderr;

            // Якщо команда відкриття посилання/браузера завершилась помилкою (наприклад, xdg-open/gio помилка 4 через конфлікт desktop-файлів)
            if status_code != 0
                && (trimmed_cmd.starts_with("xdg-open ")
                    || trimmed_cmd.starts_with("open ")
                    || trimmed_cmd.starts_with("gio open "))
            {
                let parts: Vec<&str> = trimmed_cmd.split_whitespace().collect();
                if let Some(target) = parts.last() {
                    if target.starts_with("http://") || target.starts_with("https://") {
                        let fallbacks = [
                            "/snap/bin/firefox",
                            "firefox",
                            "sensible-browser",
                            "google-chrome",
                            "google-chrome-stable",
                            "chromium",
                            "chromium-browser",
                        ];
                        for fb in fallbacks {
                            if let Ok(mut c) = Command::new(fb)
                                .arg(target)
                                .stdin(Stdio::null())
                                .stdout(Stdio::null())
                                .stderr(Stdio::null())
                                .spawn()
                            {
                                thread::sleep(Duration::from_millis(150));
                                match c.try_wait() {
                                    Ok(Some(fb_st)) if !fb_st.success() => continue,
                                    _ => {
                                        status_code = 0;
                                        final_stdout = format!("Відкрито у fallback-браузері ({})", fb);
                                        final_stderr.clear();
                                        std::mem::forget(c);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            let combined = format!(
                "Exit code: {}\n--- STDOUT ---\n{}\n--- STDERR ---\n{}",
                status_code,
                final_stdout.trim(),
                final_stderr.trim()
            );
            Ok(truncate_chars(&combined, MAX_CLI_OUTPUT_CHARS))
        } else if is_gui {
            // Залишаємо запущений браузер чи десктопну програму працювати у фоні
            std::mem::forget(child);
            let combined = format!(
                "Exit code: 0 (програму успішно запущено у фоновому режимі)\n--- STDOUT ---\n{}\n--- STDERR ---\n{}",
                stdout.trim(),
                stderr.trim()
            );
            Ok(truncate_chars(&combined, MAX_CLI_OUTPUT_CHARS))
        } else {
            let _ = child.kill();
            let _ = child.wait();
            let combined = format!(
                "Exit code: 124 (перевищено ліміт часу виконання 10с)\n--- STDOUT ---\n{}\n--- STDERR ---\n{}",
                stdout.trim(),
                stderr.trim()
            );
            Ok(truncate_chars(&combined, MAX_CLI_OUTPUT_CHARS))
        }
    })
    .await
    .map_err(|e| format!("Помилка потоку: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_gui_or_detached_command_filtering() {
        assert!(!is_gui_or_detached_command("cat operation.log"));
        assert!(!is_gui_or_detached_command("grep opera file.txt"));
        assert!(!is_gui_or_detached_command("ls -la"));
        assert!(is_gui_or_detached_command("opera https://example.com"));
        assert!(is_gui_or_detached_command("/usr/bin/firefox"));
        assert!(is_gui_or_detached_command("xdg-open https://example.com"));
        assert!(is_gui_or_detached_command("open https://example.com"));
        assert!(is_gui_or_detached_command("gio open https://example.com"));
        assert!(is_gui_or_detached_command("python3 script.py &"));
        assert!(is_gui_or_detached_command("nohup python3 script.py"));
    }

    #[test]
    fn test_is_sensitive_path() {
        assert!(is_sensitive_path(Path::new("/home/user/.ssh/id_rsa")));
        assert!(is_sensitive_path(Path::new("/home/user/.gnupg/secring.gpg")));
        assert!(is_sensitive_path(Path::new("/home/user/.aws/credentials")));
        assert!(is_sensitive_path(Path::new("/etc/shadow")));
        assert!(is_sensitive_path(Path::new("/home/user/project/config.env")));
        assert!(is_sensitive_path(Path::new("/home/user/project/.env")));
        assert!(!is_sensitive_path(Path::new("/home/user/project/README.md")));
        assert!(!is_sensitive_path(Path::new("/tmp/test.txt")));
    }
}

