use chrono::Local;
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager,
};

const MAX_READ_CHARS: usize = 8_000;
const MAX_CLI_OUTPUT_CHARS: usize = 6_000;

/// Повертає домашню директорію користувача (`HOME` або `USERPROFILE`).
fn get_home_dir() -> Result<PathBuf, String> {
    env::var("HOME")
        .or_else(|_| env::var("USERPROFILE"))
        .map(PathBuf::from)
        .map_err(|_| "Не вдалося визначити домашню директорію користувача (HOME/USERPROFILE)".to_string())
}

/// Розгортає префікс `~` або `~/` у повний шлях до домашньої директорії.
fn expand_tilde(raw_path: &str) -> Result<PathBuf, String> {
    let trimmed = raw_path.trim();
    if trimmed == "~" {
        return get_home_dir();
    }
    if let Some(stripped) = trimmed.strip_prefix("~/").or_else(|| trimmed.strip_prefix("~\\")) {
        return Ok(get_home_dir()?.join(stripped));
    }
    Ok(PathBuf::from(trimmed))
}

/// Перевіряє відносний шлях на відсутність `..` (захист від виходу за межі Obsidian Vault).
fn sanitize_relative_path(rel: &str) -> Result<PathBuf, String> {
    let path = Path::new(rel.trim());
    let mut clean = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!(
                    "Неприпустимий відносний шлях '{}': заборонено використовувати '..' або абсолютні шляхи у Vault",
                    rel
                ));
            }
        }
    }
    Ok(clean)
}

/// Читає значення змінної з оточення або локального файлу `.env` (якщо він є).
fn get_env_or_dotenv(key: &str) -> Option<String> {
    if let Ok(val) = env::var(key) {
        if !val.trim().is_empty() {
            return Some(val.trim().to_string());
        }
    }
    let mut candidates = vec![PathBuf::from(".env"), PathBuf::from("../.env")];
    if let Ok(home) = get_home_dir() {
        candidates.push(home.join("local_helper_ai/.env"));
        candidates.push(home.join(".config/local_helper_ai/.env"));
    }

    for candidate in candidates {
        if let Ok(content) = fs::read_to_string(&candidate) {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    if k.trim() == key {
                        let clean_v = v.trim().trim_matches('"').trim_matches('\'').to_string();
                        if !clean_v.is_empty() {
                            return Some(clean_v);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Безпечно обрізає UTF-8 рядок за кількістю символів (щоб не панікувати на кирилиці).
fn truncate_chars(text: &str, max_chars: usize) -> String {
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

/// Розумно вставляє новий запис або TODO у Markdown/Kanban структуру під вказану секцію (наприклад, ## Ideas або ## TO DO).
fn insert_into_obsidian_markdown(
    original: &str,
    formatted_entry: &str,
    as_todo: bool,
    section: Option<&str>,
) -> String {
    // Якщо попередній запис випадково приклеївся до блоку налаштувань %% kanban:settings
    let clean_original = if original.contains("%%- [") {
        original.replace("%%- [", "%%\n- [")
    } else {
        original.to_string()
    };

    let lines: Vec<&str> = clean_original.lines().collect();

    let target_name = section
        .map(|s| s.trim().to_uppercase())
        .filter(|s| !s.is_empty())
        .or_else(|| if as_todo { Some("TO DO".to_string()) } else { None });

    if let Some(ref target) = target_name {
        let mut target_heading_idx = None;
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                let clean_title = trimmed.trim_start_matches('#').trim().to_uppercase();
                if target == "TO DO" || target == "TODO" || target == "TASKS" {
                    if clean_title == "TO DO"
                        || clean_title == "TODO"
                        || clean_title == "TASKS"
                        || clean_title == "ЗАДАЧІ"
                        || clean_title == "СПРАВИ"
                    {
                        target_heading_idx = Some(i);
                        break;
                    }
                } else if target == "IDEAS" || target == "IDEA" || target == "ІДЕЇ" || target == "ІДЕЯ" {
                    if clean_title == "IDEAS"
                        || clean_title == "IDEA"
                        || clean_title == "ІДЕЇ"
                        || clean_title == "ІДЕЯ"
                    {
                        target_heading_idx = Some(i);
                        break;
                    }
                } else if clean_title == *target {
                    target_heading_idx = Some(i);
                    break;
                }
            }
        }

        if let Some(h_idx) = target_heading_idx {
            // Шукаємо кінець цієї колонки (наступний заголовок '#' або блок налаштувань Kanban '%%')
            let mut insert_idx = lines.len();
            for i in (h_idx + 1)..lines.len() {
                let trimmed = lines[i].trim();
                if trimmed.starts_with('#') || trimmed.starts_with("%%") {
                    insert_idx = i;
                    break;
                }
            }

            let existing_items: Vec<&str> = lines[(h_idx + 1)..insert_idx]
                .iter()
                .copied()
                .filter(|l| !l.trim().is_empty() && !l.trim().starts_with("%%"))
                .collect();

            let mut result_lines = Vec::new();
            for i in 0..=h_idx {
                result_lines.push(lines[i].to_string());
            }

            result_lines.push("".to_string());
            for item in existing_items {
                result_lines.push(item.to_string());
            }
            result_lines.push(formatted_entry.trim_end().to_string());
            result_lines.push("".to_string());

            // Залишок файлу
            let trimmed_entry = formatted_entry.trim();
            for i in insert_idx..lines.len() {
                if lines[i].trim() != trimmed_entry {
                    result_lines.push(lines[i].to_string());
                }
            }

            let mut out = result_lines.join("\n");
            if !out.ends_with('\n') {
                out.push('\n');
            }
            return out;
        }

        // Якщо вказана секція ще не існує у файлі — створюємо її перед блоком %% або в кінці
        let k_idx = lines
            .iter()
            .position(|l| l.trim().starts_with("%%"))
            .unwrap_or(lines.len());

        let mut result_lines = Vec::new();
        for i in 0..k_idx {
            result_lines.push(lines[i].to_string());
        }

        let section_title = section.unwrap_or("Ideas");
        result_lines.push("".to_string());
        result_lines.push(format!("## {}", section_title));
        result_lines.push("".to_string());
        result_lines.push(formatted_entry.trim_end().to_string());
        result_lines.push("".to_string());

        for i in k_idx..lines.len() {
            result_lines.push(lines[i].to_string());
        }

        let mut out = result_lines.join("\n");
        if !out.ends_with('\n') {
            out.push('\n');
        }
        return out;
    }

    // Якщо це не секція і не TODO, але є блок %% kanban:settings — вставляємо ПЕРЕД %%
    if let Some(k_idx) = lines.iter().position(|l| l.trim().starts_with("%%")) {
        let mut result_lines = Vec::new();
        for i in 0..k_idx {
            result_lines.push(lines[i].to_string());
        }
        result_lines.push("".to_string());
        result_lines.push(formatted_entry.trim_end().to_string());
        result_lines.push("".to_string());
        for i in k_idx..lines.len() {
            result_lines.push(lines[i].to_string());
        }
        let mut out = result_lines.join("\n");
        if !out.ends_with('\n') {
            out.push('\n');
        }
        return out;
    }

    // Звичайний fallback
    let mut out = clean_original;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(formatted_entry);
    out
}

/// 1. Додає запис, TODO-таск або ідею у нотатку Obsidian Vault (підтримує Kanban дошки та конкретні колонки).
#[tauri::command(rename_all = "snake_case")]
fn append_to_obsidian(
    subfolder: Option<String>,
    filename: String,
    content: String,
    as_todo: bool,
    section: Option<String>,
) -> Result<String, String> {
    let vault_raw = get_env_or_dotenv("OBSIDIAN_VAULT_PATH").unwrap_or_else(|| "~/Obsidian/Ivan".to_string());
    let vault_base = expand_tilde(&vault_raw)?;

    let mut target_path = vault_base;

    if let Some(sub) = subfolder {
        let trimmed_sub = sub.trim();
        if !trimmed_sub.is_empty() {
            let safe_sub = sanitize_relative_path(trimmed_sub)?;
            target_path = target_path.join(safe_sub);
        }
    }

    let safe_file = sanitize_relative_path(&filename)?;
    if safe_file.as_os_str().is_empty() {
        return Err("Ім'я файлу нотатки не може бути порожнім".to_string());
    }

    target_path = target_path.join(safe_file);
    if target_path.extension().and_then(|e| e.to_str()) != Some("md") {
        target_path.set_extension("md");
    }

    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Не вдалося створити папку '{}': {}", parent.display(), e))?;
    }

    let is_ideas_section = section
        .as_deref()
        .map(|s| {
            let upper = s.trim().to_uppercase();
            upper == "IDEAS" || upper == "IDEA" || upper == "ІДЕЇ" || upper == "ІДЕЯ"
        })
        .unwrap_or(false);

    let mut clean_content = content.trim();
    // Якщо текст ідеї випадково починався з дати/таймштампу **[YYYY-MM-DD...]**, прибираємо його
    if is_ideas_section && clean_content.starts_with("**[") {
        if let Some(pos) = clean_content.find("]**") {
            clean_content = clean_content[pos + 3..].trim();
        }
    }

    let formatted_entry = if is_ideas_section || as_todo {
        if clean_content.starts_with("- [ ]") {
            format!("{}\n", clean_content)
        } else {
            format!("- [ ] {}\n", clean_content)
        }
    } else {
        let timestamp = Local::now().format("%Y-%m-%d %H:%M").to_string();
        format!("- **[{}]** {}\n", timestamp, clean_content)
    };

    let new_content = if target_path.exists() {
        let existing = fs::read_to_string(&target_path)
            .map_err(|e| format!("Помилка читання нотатки '{}': {}", target_path.display(), e))?;
        insert_into_obsidian_markdown(&existing, &formatted_entry, as_todo, section.as_deref())
    } else {
        let title = target_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Нотатка");
        if as_todo {
            format!("# {}\n\n## TO DO\n\n{}\n", title, formatted_entry.trim_end())
        } else {
            format!("# {}\n\n{}", title, formatted_entry)
        }
    };

    fs::write(&target_path, new_content.as_bytes())
        .map_err(|e| format!("Помилка запису в Obsidian нотатку '{}': {}", target_path.display(), e))?;

    Ok(format!(
        "Записано у нотатку Obsidian: {}",
        target_path.display()
    ))
}

/// 2. Створює або повністю перезаписує файл за вказаним шляхом (підтримує `~`).
#[tauri::command(rename_all = "snake_case")]
fn create_or_update_file(filepath: String, content: String) -> Result<String, String> {
    let path = expand_tilde(&filepath)?;

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

    fs::write(&path, content.as_bytes())
        .map_err(|e| format!("Помилка запису файлу '{}': {}", path.display(), e))?;

    Ok(format!(
        "Файл успішно збережено ({} байтів): {}",
        content.len(),
        path.display()
    ))
}

/// 3. Читає вміст файлу або список елементів директорії (підтримує `~`).
#[tauri::command(rename_all = "snake_case")]
fn read_file_or_dir(target_path: String) -> Result<String, String> {
    let path = expand_tilde(&target_path)?;

    if !path.exists() {
        return Err(format!("Шлях не існує: {}", path.display()));
    }

    if path.is_dir() {
        let entries = fs::read_dir(&path)
            .map_err(|e| format!("Не вдалося прочитати директорію '{}': {}", path.display(), e))?;

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
}

/// 4. Виконує CLI-команду в оболонці ОС та повертає stdout / stderr.
#[tauri::command(rename_all = "snake_case")]
fn run_cli_command(command: String, working_dir: Option<String>) -> Result<String, String> {
    let trimmed_cmd = command.trim();
    if trimmed_cmd.is_empty() {
        return Err("Команда не може бути порожньою".to_string());
    }

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

    let output = cmd
        .output()
        .map_err(|e| format!("Помилка запуску команди '{}': {}", trimmed_cmd, e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let status_code = output.status.code().unwrap_or(-1);

    let combined = format!(
        "Exit code: {}\n--- STDOUT ---\n{}\n--- STDERR ---\n{}",
        status_code,
        stdout.trim(),
        stderr.trim()
    );

    Ok(truncate_chars(&combined, MAX_CLI_OUTPUT_CHARS))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_always_on_top(true);
                let _ = window.set_visible_on_all_workspaces(true);

                // Забезпечуємо, щоб вікно маскота завжди залишалося поверх усіх інших програм
                let w = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::Focused(_) = event {
                        let _ = w.set_always_on_top(true);
                    }
                });
            }

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
                        if let Some(window) = app.get_webview_window("main") {
                            let is_visible = window.is_visible().unwrap_or(true);
                            if is_visible {
                                let _ = window.hide();
                            } else {
                                let _ = window.show();
                                let _ = window.set_focus();
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
            append_to_obsidian,
            create_or_update_file,
            read_file_or_dir,
            run_cli_command
        ])
        .run(tauri::generate_context!())
        .expect("Помилка запуску Tauri-застосунку");
}

