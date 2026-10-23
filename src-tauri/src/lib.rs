use chrono::{DateTime, Duration as ChronoDuration, Local, NaiveDateTime, NaiveTime, TimeZone};
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

static LAST_KNOWN_MUSIC_PLAYING: AtomicBool = AtomicBool::new(false);
static REMINDERS_LOCK: Mutex<()> = Mutex::new(());
static MEMORY_LOCK: Mutex<()> = Mutex::new(());
use std::thread;
use std::time::Duration;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Emitter, Manager,
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

/// Повертає шлях до конфігураційного файлу користувача (~/.config/local-helper-ai/config.env або %APPDATA%/local-helper-ai/config.env).
pub fn get_user_config_path() -> Result<PathBuf, String> {
    if let Ok(xdg) = env::var("XDG_CONFIG_HOME") {
        let trimmed = xdg.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed).join("local-helper-ai").join("config.env"));
        }
    }
    #[cfg(target_os = "windows")]
    if let Ok(appdata) = env::var("APPDATA") {
        let trimmed = appdata.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed).join("local-helper-ai").join("config.env"));
        }
    }
    let home = get_home_dir()?;
    Ok(home.join(".config").join("local-helper-ai").join("config.env"))
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct AppConfig {
    pub gemini_api_key: String,
    pub gemini_model: String,
    pub obsidian_vault_path: String,
    pub config_file_path: String,
}

/// Повертає поточну конфігурацію застосунку.
pub fn read_app_config() -> AppConfig {
    let api_key = get_env_or_dotenv("GEMINI_API_KEY")
        .or_else(|| get_env_or_dotenv("VITE_GEMINI_API_KEY"))
        .unwrap_or_default();
    let model = get_env_or_dotenv("GEMINI_MODEL")
        .or_else(|| get_env_or_dotenv("VITE_GEMINI_MODEL"))
        .unwrap_or_else(|| "gemini-3.5-flash-lite".to_string());
    let vault = get_env_or_dotenv("OBSIDIAN_VAULT_PATH")
        .unwrap_or_else(|| "~/Obsidian/Ivan".to_string());
    let config_path = get_user_config_path()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "~/.config/local-helper-ai/config.env".to_string());

    AppConfig {
        gemini_api_key: api_key,
        gemini_model: model,
        obsidian_vault_path: vault,
        config_file_path: config_path,
    }
}

/// Оновлює або додає значення в конфігураційний файл ~/.config/local-helper-ai/config.env.
pub fn update_config_value(key: &str, val: &str) -> Result<PathBuf, String> {
    let config_path = get_user_config_path()?;
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            format!(
                "Не вдалося створити директорію {}: {}",
                parent.display(),
                e
            )
        })?;
    }

    let trimmed_val = val.trim();
    let mut lines: Vec<String> = if config_path.exists() {
        let content = fs::read_to_string(&config_path).map_err(|e| {
            format!("Не вдалося прочитати {}: {}", config_path.display(), e)
        })?;
        content.lines().map(|s| s.to_string()).collect()
    } else {
        vec![
            "# Local Helper Mascot — Конфігурація користувача".to_string(),
            "# Цей файл автоматично згенеровано утилітою local-helper-ai / addkey".to_string(),
            "".to_string(),
        ]
    };

    let alt_key = match key {
        "GEMINI_API_KEY" => Some("VITE_GEMINI_API_KEY"),
        "VITE_GEMINI_API_KEY" => Some("GEMINI_API_KEY"),
        "GEMINI_MODEL" => Some("VITE_GEMINI_MODEL"),
        "VITE_GEMINI_MODEL" => Some("GEMINI_MODEL"),
        _ => None,
    };

    let mut found = false;
    for line in &mut lines {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((k, _)) = trimmed.split_once('=') {
            let k = k.trim();
            if k == key || alt_key.map(|a| a == k).unwrap_or(false) {
                *line = format!("{}={}", key, trimmed_val);
                found = true;
                break;
            }
        }
    }

    if !found {
        lines.push(format!("{}={}", key, trimmed_val));
    }

    // Для сумісності з фронтендом зберігаємо також VITE_ дзеркала
    if key == "GEMINI_API_KEY" {
        let mut vite_found = false;
        for line in &mut lines {
            if let Some((k, _)) = line.trim().split_once('=') {
                if k.trim() == "VITE_GEMINI_API_KEY" {
                    *line = format!("VITE_GEMINI_API_KEY={}", trimmed_val);
                    vite_found = true;
                    break;
                }
            }
        }
        if !vite_found {
            lines.push(format!("VITE_GEMINI_API_KEY={}", trimmed_val));
        }
    } else if key == "GEMINI_MODEL" {
        let mut vite_found = false;
        for line in &mut lines {
            if let Some((k, _)) = line.trim().split_once('=') {
                if k.trim() == "VITE_GEMINI_MODEL" {
                    *line = format!("VITE_GEMINI_MODEL={}", trimmed_val);
                    vite_found = true;
                    break;
                }
            }
        }
        if !vite_found {
            lines.push(format!("VITE_GEMINI_MODEL={}", trimmed_val));
        }
    }

    let new_content = lines.join("\n") + "\n";
    fs::write(&config_path, new_content).map_err(|e| {
        format!(
            "Не вдалося зберегти конфігурацію у {}: {}",
            config_path.display(),
            e
        )
    })?;

    env::set_var(key, trimmed_val);
    if let Some(alt) = alt_key {
        env::set_var(alt, trimmed_val);
    }

    Ok(config_path)
}

#[tauri::command]
async fn get_app_config() -> Result<AppConfig, String> {
    tauri::async_runtime::spawn_blocking(read_app_config)
        .await
        .map_err(|e| format!("Помилка читання конфігурації: {}", e))
}

/// Читає значення змінної з оточення або локального файлу `.env` (якщо він є).
fn get_env_or_dotenv(key: &str) -> Option<String> {
    if let Ok(val) = env::var(key) {
        if !val.trim().is_empty() {
            return Some(val.trim().to_string());
        }
    }

    let alt_key = match key {
        "GEMINI_API_KEY" => Some("VITE_GEMINI_API_KEY"),
        "VITE_GEMINI_API_KEY" => Some("GEMINI_API_KEY"),
        "GEMINI_MODEL" => Some("VITE_GEMINI_MODEL"),
        "VITE_GEMINI_MODEL" => Some("GEMINI_MODEL"),
        _ => None,
    };

    if let Some(alt) = alt_key {
        if let Ok(val) = env::var(alt) {
            if !val.trim().is_empty() {
                return Some(val.trim().to_string());
            }
        }
    }

    let mut candidates = Vec::new();
    if let Ok(config_path) = get_user_config_path() {
        candidates.push(config_path);
    }
    #[cfg(target_os = "windows")]
    if let Ok(appdata) = env::var("APPDATA") {
        candidates.push(PathBuf::from(appdata).join("local-helper-ai").join("config.env"));
    }
    if let Ok(home) = get_home_dir() {
        candidates.push(home.join(".config/local-helper-ai/config.env"));
        candidates.push(home.join(".config/local_helper_ai/.env"));
        candidates.push(home.join("local_helper_ai/.env"));
    }
    candidates.push(PathBuf::from(".env"));
    candidates.push(PathBuf::from("../.env"));

    for candidate in candidates {
        if let Ok(content) = fs::read_to_string(&candidate) {
            let mut found_alt = None;
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    let k = k.trim();
                    if k == key {
                        let clean_v = v.trim().trim_matches('"').trim_matches('\'').to_string();
                        if !clean_v.is_empty() {
                            return Some(clean_v);
                        }
                    } else if let Some(alt) = alt_key {
                        if k == alt && found_alt.is_none() {
                            let clean_v = v.trim().trim_matches('"').trim_matches('\'').to_string();
                            if !clean_v.is_empty() {
                                found_alt = Some(clean_v);
                            }
                        }
                    }
                }
            }
            if let Some(val) = found_alt {
                return Some(val);
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

            let trimmed_entry = formatted_entry.trim();
            let existing_items: Vec<&str> = lines[(h_idx + 1)..insert_idx]
                .iter()
                .copied()
                .filter(|l| !l.trim().is_empty() && !l.trim().starts_with("%%") && l.trim() != trimmed_entry)
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

            // Залишок файлу копіюємо без змін (не видаляємо однакові тексти з інших секцій)
            for i in insert_idx..lines.len() {
                result_lines.push(lines[i].to_string());
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

        let section_title = section.unwrap_or(if as_todo { "TO DO" } else { "Ideas" });
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
async fn append_to_obsidian(
    subfolder: Option<String>,
    filename: String,
    content: String,
    as_todo: bool,
    section: Option<String>,
    due_date: Option<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let vault_raw = get_env_or_dotenv("OBSIDIAN_VAULT_PATH")
            .unwrap_or_else(|| "~/Obsidian/Ivan".to_string());
        let vault_base = expand_tilde(&vault_raw)?;

        let mut target_path = vault_base;

        if let Some(sub) = subfolder {
            let trimmed_sub = sub.trim();
            let lower_sub = trimmed_sub.to_lowercase();
            if !trimmed_sub.is_empty()
                && lower_sub != "null"
                && lower_sub != "none"
                && lower_sub != "undefined"
                && lower_sub != "nil"
                && trimmed_sub != "."
                && trimmed_sub != "/"
                && trimmed_sub != "~"
            {
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
            fs::create_dir_all(parent).map_err(|e| {
                format!("Не вдалося створити папку '{}': {}", parent.display(), e)
            })?;
        }

        let clean_section = section
            .as_deref()
            .map(|s| s.trim().trim_start_matches('#').trim())
            .filter(|s| !s.is_empty());

        let is_ideas_section = clean_section
            .map(|s| {
                let upper = s.to_uppercase();
                upper == "IDEAS" || upper == "IDEA" || upper == "ІДЕЇ" || upper == "ІДЕЯ"
            })
            .unwrap_or(false);

        let mut clean_content = content.trim().to_string();
        if is_ideas_section && clean_content.starts_with("**[") {
            if let Some(pos) = clean_content.find("]**") {
                clean_content = clean_content[pos + 3..].trim().to_string();
            }
        }

        // Якщо передано дедлайн, а у тексті ще немає дедлайну або @{...} — додаємо у форматі @{YYYY-MM-DD}
        if let Some(due) = due_date {
            let due_trim = due.trim();
            if !due_trim.is_empty() && !clean_content.contains(due_trim) && !clean_content.contains("@{") {
                clean_content = format!("{} @{{{}}}", clean_content, due_trim);
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
            let existing = fs::read_to_string(&target_path).map_err(|e| {
                format!("Помилка читання нотатки '{}': {}", target_path.display(), e)
            })?;
            insert_into_obsidian_markdown(&existing, &formatted_entry, as_todo, clean_section)
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

        fs::write(&target_path, new_content.as_bytes()).map_err(|e| {
            format!(
                "Помилка запису в Obsidian нотатку '{}': {}",
                target_path.display(),
                e
            )
        })?;

        Ok(format!(
            "Записано у нотатку Obsidian: {}",
            target_path.display()
        ))
    })
    .await
    .map_err(|e| format!("Помилка потоку: {}", e))?
}

/// Перевіряє, чи не є шлях конфіденційним (SSH, GPG, AWS, /etc/shadow, .env, config.env)
fn is_sensitive_path(path: &Path) -> bool {
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
async fn create_or_update_file(filepath: String, content: String) -> Result<String, String> {
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
async fn read_file_or_dir(target_path: String) -> Result<String, String> {
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
fn is_gui_or_detached_command(cmd_str: &str) -> bool {
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
async fn run_cli_command(command: String, working_dir: Option<String>) -> Result<String, String> {
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

struct RecordingSession {
    child: Child,
    file_path: PathBuf,
}

static VOICE_RECORDER: OnceLock<Mutex<Option<RecordingSession>>> = OnceLock::new();

fn get_recorder() -> &'static Mutex<Option<RecordingSession>> {
    VOICE_RECORDER.get_or_init(|| Mutex::new(None))
}

/// Кодує байти у стандартний Base64 (RFC 4648).
fn encode_base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(((input.len() + 2) / 3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Гарантує коректні розміри RIFF та data-чанків у WAV-файлі після зупинки запису.
fn patch_wav_header(bytes: &mut [u8]) {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return;
    }
    let riff_size = ((bytes.len() - 8) as u32).to_le_bytes();
    bytes[4..8].copy_from_slice(&riff_size);

    let mut offset = 12usize;
    while offset + 8 <= bytes.len() {
        if &bytes[offset..offset + 4] == b"data" {
            let data_size = ((bytes.len() - (offset + 8)) as u32).to_le_bytes();
            bytes[offset + 4..offset + 8].copy_from_slice(&data_size);
            break;
        }
        let chunk_size = u32::from_le_bytes([
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ]) as usize;
        let next = offset
            .saturating_add(8)
            .saturating_add(chunk_size)
            .saturating_add(chunk_size % 2);
        if next <= offset {
            break;
        }
        offset = next;
    }
}

fn terminate_recording_session(mut session: RecordingSession) -> PathBuf {
    let pid = session.child.id();
    let _ = Command::new("kill")
        .args(["-s", "INT", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    for _ in 0..12 {
        if let Ok(Some(_)) = session.child.try_wait() {
            break;
        }
        thread::sleep(Duration::from_millis(40));
    }

    let _ = session.child.kill();
    let _ = session.child.wait();
    session.file_path
}

/// 5. Запускає фоновий запис голосу з мікрофона через PipeWire (`pw-record`) або ALSA (`arecord`).
#[tauri::command]
async fn start_voice_recording() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = get_recorder()
            .lock()
            .map_err(|_| "Помилка блокування аудіорекордера".to_string())?;

        if let Some(old_session) = guard.take() {
            let old_path = terminate_recording_session(old_session);
            let _ = fs::remove_file(old_path);
        }

        let file_path = env::temp_dir().join("local_helper_voice.wav");
        let _ = fs::remove_file(&file_path);
        let path_str = file_path.to_string_lossy().to_string();

        let child = Command::new("pw-record")
            .args(["--rate=16000", "--channels=1", "--format=s16", &path_str])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .or_else(|_| {
                Command::new("arecord")
                    .args([
                        "-f", "S16_LE", "-r", "16000", "-c", "1", "-t", "wav", &path_str,
                    ])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
            })
            .map_err(|e| {
                format!(
                    "Не вдалося запустити запис мікрофона (pw-record / arecord): {}",
                    e
                )
            })?;

        *guard = Some(RecordingSession { child, file_path });
        Ok(())
    })
    .await
    .map_err(|e| format!("Помилка потоку: {}", e))?
}

/// 6. Зупиняє запис голосу та повертає Base64-рядок `audio/wav`.
#[tauri::command]
async fn stop_voice_recording() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let session = {
            let mut guard = get_recorder()
                .lock()
                .map_err(|_| "Помилка блокування аудіорекордера".to_string())?;
            guard
                .take()
                .ok_or_else(|| "Активний запис голосу не знайдено".to_string())?
        };

        let file_path = terminate_recording_session(session);
        let mut bytes = fs::read(&file_path)
            .map_err(|e| format!("Не вдалося прочитати записаний аудіофайл: {}", e))?;
        let _ = fs::remove_file(&file_path);

        if bytes.len() <= 200 {
            return Err("Запис занадто короткий або мікрофон не передав звук.".to_string());
        }

        patch_wav_header(&mut bytes);
        Ok(encode_base64(&bytes))
    })
    .await
    .map_err(|e| format!("Помилка потоку: {}", e))?
}

/// 7. Скасовує активний запис голосу без повернення даних.
#[tauri::command]
async fn cancel_voice_recording() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = get_recorder()
            .lock()
            .map_err(|_| "Помилка блокування аудіорекордера".to_string())?;

        if let Some(session) = guard.take() {
            let file_path = terminate_recording_session(session);
            let _ = fs::remove_file(file_path);
        }
        Ok(())
    })
    .await
    .map_err(|e| format!("Помилка потоку: {}", e))?
}

/// 8. Читає файл зображення з диска та повертає його вміст у Base64.
#[tauri::command]
async fn read_image_base64(filepath: String) -> Result<String, String> {
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

#[derive(serde::Serialize)]
struct ClipboardImageResult {
    mime_type: String,
    base64_data: String,
    name: String,
}

fn decode_percent(s: &str) -> String {
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

fn try_read_image_file_path(path_str: &str) -> Option<ClipboardImageResult> {
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
fn read_gtk_clipboard_on_main_thread() -> Option<ClipboardImageResult> {
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

/// 9. Читає зображення з системного буфера обміну всередині процесу через GTK (без сторонніх утиліт).
#[tauri::command]
async fn read_clipboard_image(
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

#[cfg(target_os = "linux")]
fn run_python_mpris_cmd(py_code: &str) -> Option<std::process::ExitStatus> {
    if Path::new("/usr/bin/aa-exec").exists() {
        if let Ok(status) = Command::new("/usr/bin/aa-exec")
            .args(["-p", "unconfined", "python3", "-c", py_code])
            .stdin(Stdio::null())
            .status()
        {
            return Some(status);
        }
    }
    Command::new("python3")
        .args(["-c", py_code])
        .stdin(Stdio::null())
        .status()
        .ok()
}

fn resume_media_player() -> bool {
    #[cfg(target_os = "linux")]
    {
        let py_code = r#"
import sys, dbus
try:
    bus = dbus.SessionBus()
    players = [s for s in bus.list_names() if s.startswith('org.mpris.MediaPlayer2.')]
    if not players:
        sys.exit(1)
    resumed = False
    for p in players:
        try:
            player_obj = bus.get_object(p, '/org/mpris/MediaPlayer2', introspect=False)
            iface = dbus.Interface(player_obj, 'org.mpris.MediaPlayer2.Player')
            try:
                iface.Play()
                resumed = True
            except Exception:
                try:
                    iface.PlayPause()
                    resumed = True
                except Exception:
                    pass
        except Exception:
            pass
    sys.exit(0 if resumed else 1)
except Exception:
    sys.exit(1)
"#;
        if let Some(status) = run_python_mpris_cmd(py_code) {
            if status.success() {
                return true;
            }
        }
        return false;
    }

    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("powershell")
            .args(["-Command", "$w = New-Object -ComObject WScript.Shell; $w.SendKeys([char]179)"])
            .status();
        return true;
    }

    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("osascript")
            .args(["-e", "tell application \"Spotify\" to play"])
            .status();
        let _ = Command::new("osascript")
            .args(["-e", "tell application \"Music\" to play"])
            .status();
        return true;
    }

    #[allow(unreachable_code)]
    false
}

/// Якщо нова вкладка YouTube Music відкрилась зі спливаючим блокуванням "Почати відтворення",
/// MPRIS-сесія ще не активна до першого кліку/натискання клавіші у вкладці.
/// Ця функція перевіряє через D-Bus, чи плеєр ще не грає, і надсилає апаратне натискання Space
/// через ядерний інтерфейс /dev/uinput (працює у Wayland для Firefox та Chrome) або XTest.
fn kickstart_browser_playback() -> bool {
    if resume_media_player() {
        return true;
    }

    #[cfg(target_os = "linux")]
    {
        let py_kick = r#"
import sys, os, time, fcntl, struct, subprocess, ctypes, ctypes.util

def is_mpris_playing():
    try:
        import dbus
        bus = dbus.SessionBus()
        for p in bus.list_names():
            if p.startswith('org.mpris.MediaPlayer2.'):
                try:
                    obj = bus.get_object(p, '/org/mpris/MediaPlayer2')
                    props = dbus.Interface(obj, 'org.freedesktop.DBus.Properties')
                    if str(props.Get('org.mpris.MediaPlayer2.Player', 'PlaybackStatus')) == 'Playing':
                        return True
                except Exception:
                    pass
    except Exception:
        pass
    return False

if is_mpris_playing():
    sys.exit(0)

# 1. Нативний Wayland / X11 метод через /dev/uinput (працює для Mozilla Firefox та Chrome у Wayland)
try:
    fd = os.open("/dev/uinput", os.O_WRONLY | os.O_NONBLOCK)
    UI_SET_EVBIT = 0x40045564
    UI_SET_KEYBIT = 0x40045565
    UI_DEV_CREATE = 0x5501
    UI_DEV_DESTROY = 0x5502
    EV_SYN = 0x00
    EV_KEY = 0x01
    KEY_SPACE = 57

    fcntl.ioctl(fd, UI_SET_EVBIT, EV_KEY)
    fcntl.ioctl(fd, UI_SET_EVBIT, EV_SYN)
    fcntl.ioctl(fd, UI_SET_KEYBIT, KEY_SPACE)

    udev = struct.pack("80sHHHHI", b"local-helper-media-kbd", 0x03, 0x1234, 0x5678, 1, 0) + b"\x00" * (64 * 4 * 4)
    os.write(fd, udev)
    fcntl.ioctl(fd, UI_DEV_CREATE)
    time.sleep(0.3)

    if not is_mpris_playing():
        os.write(fd, struct.pack("llHHi", 0, 0, EV_KEY, KEY_SPACE, 1))
        os.write(fd, struct.pack("llHHi", 0, 0, EV_SYN, 0, 0))
        time.sleep(0.05)
        os.write(fd, struct.pack("llHHi", 0, 0, EV_KEY, KEY_SPACE, 0))
        os.write(fd, struct.pack("llHHi", 0, 0, EV_SYN, 0, 0))
        time.sleep(0.1)

    fcntl.ioctl(fd, UI_DEV_DESTROY)
    os.close(fd)
    time.sleep(0.6)
    if is_mpris_playing():
        sys.exit(0)
    sys.exit(1)
except Exception:
    pass

# 2. Фолбек через X11 XTest (якщо /dev/uinput недоступний)
try:
    active_out = subprocess.check_output(["xprop", "-root", "_NET_ACTIVE_WINDOW"], timeout=2).decode("utf-8", errors="ignore")
    parts = active_out.strip().split()
    win_id = parts[-1] if parts else ""
    if not win_id.startswith("0x") or win_id == "0x0":
        sys.exit(1)
    win_info = subprocess.check_output(["xprop", "-id", win_id, "_NET_WM_NAME", "WM_NAME", "WM_CLASS"], timeout=2).decode("utf-8", errors="ignore").lower()
    if not any(k in win_info for k in ("youtube music", "chrome", "chromium", "brave", "opera", "firefox")):
        sys.exit(1)

    x11_path = ctypes.util.find_library("X11")
    xtst_path = ctypes.util.find_library("Xtst")
    if not x11_path or not xtst_path:
        sys.exit(1)

    x11 = ctypes.cdll.LoadLibrary(x11_path)
    xtst = ctypes.cdll.LoadLibrary(xtst_path)

    x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
    x11.XOpenDisplay.restype = ctypes.c_void_p
    x11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
    x11.XKeysymToKeycode.restype = ctypes.c_ubyte
    xtst.XTestFakeKeyEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
    xtst.XTestFakeKeyEvent.restype = ctypes.c_int
    x11.XFlush.argtypes = [ctypes.c_void_p]
    x11.XFlush.restype = ctypes.c_int
    x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
    x11.XCloseDisplay.restype = ctypes.c_int

    dpy = x11.XOpenDisplay(None)
    if not dpy:
        sys.exit(1)

    keycode = x11.XKeysymToKeycode(dpy, 0x0020)
    if keycode != 0:
        xtst.XTestFakeKeyEvent(dpy, keycode, 1, 0)
        x11.XFlush(dpy)
        time.sleep(0.05)
        xtst.XTestFakeKeyEvent(dpy, keycode, 0, 0)
        x11.XFlush(dpy)

    x11.XCloseDisplay(dpy)
    time.sleep(0.35)
    sys.exit(0)
except Exception:
    sys.exit(1)
"#;
        if let Ok(status) = Command::new("python3").args(["-c", py_kick]).status() {
            if status.success() {
                return true;
            }
        }
    }

    false
}

fn pause_media_player() -> bool {
    #[cfg(target_os = "linux")]
    {
        let py_code = r#"
import sys, dbus
try:
    bus = dbus.SessionBus()
    players = [s for s in bus.list_names() if s.startswith('org.mpris.MediaPlayer2.')]
    if not players:
        sys.exit(1)
    paused = False
    for p in players:
        try:
            player_obj = bus.get_object(p, '/org/mpris/MediaPlayer2', introspect=False)
            iface = dbus.Interface(player_obj, 'org.mpris.MediaPlayer2.Player')
            try:
                iface.Pause()
                paused = True
            except Exception:
                try:
                    iface.PlayPause()
                    paused = True
                except Exception:
                    pass
        except Exception:
            pass
    sys.exit(0 if paused else 1)
except Exception:
    sys.exit(1)
"#;
        if let Some(status) = run_python_mpris_cmd(py_code) {
            if status.success() {
                return true;
            }
        }
        return false;
    }

    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("powershell")
            .args(["-Command", "$w = New-Object -ComObject WScript.Shell; $w.SendKeys([char]179)"])
            .status();
        return true;
    }

    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("osascript")
            .args(["-e", "tell application \"Spotify\" to pause"])
            .status();
        let _ = Command::new("osascript")
            .args(["-e", "tell application \"Music\" to pause"])
            .status();
        return true;
    }

    #[allow(unreachable_code)]
    false
}

fn next_media_player() -> bool {
    #[cfg(target_os = "linux")]
    {
        let py_code = r#"
import sys, dbus
try:
    bus = dbus.SessionBus()
    players = [s for s in bus.list_names() if s.startswith('org.mpris.MediaPlayer2.')]
    if not players:
        sys.exit(1)
    success = False
    for p in players:
        try:
            player_obj = bus.get_object(p, '/org/mpris/MediaPlayer2', introspect=False)
            iface = dbus.Interface(player_obj, 'org.mpris.MediaPlayer2.Player')
            try:
                iface.Next()
                success = True
            except Exception:
                pass
        except Exception:
            pass
    sys.exit(0 if success else 1)
except Exception:
    sys.exit(1)
"#;
        if let Some(status) = run_python_mpris_cmd(py_code) {
            if status.success() {
                return true;
            }
        }
        return false;
    }

    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("powershell")
            .args(["-Command", "$w = New-Object -ComObject WScript.Shell; $w.SendKeys([char]176)"])
            .status();
        return true;
    }

    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("osascript")
            .args(["-e", "tell application \"Spotify\" to next track"])
            .status();
        let _ = Command::new("osascript")
            .args(["-e", "tell application \"Music\" to next track"])
            .status();
        return true;
    }

    #[allow(unreachable_code)]
    false
}

fn prev_media_player() -> bool {
    #[cfg(target_os = "linux")]
    {
        let py_code = r#"
import sys, dbus
try:
    bus = dbus.SessionBus()
    players = [s for s in bus.list_names() if s.startswith('org.mpris.MediaPlayer2.')]
    if not players:
        sys.exit(1)
    success = False
    for p in players:
        try:
            player_obj = bus.get_object(p, '/org/mpris/MediaPlayer2', introspect=False)
            iface = dbus.Interface(player_obj, 'org.mpris.MediaPlayer2.Player')
            try:
                iface.Previous()
                success = True
            except Exception:
                pass
        except Exception:
            pass
    sys.exit(0 if success else 1)
except Exception:
    sys.exit(1)
"#;
        if let Some(status) = run_python_mpris_cmd(py_code) {
            if status.success() {
                return true;
            }
        }
        return false;
    }

    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("powershell")
            .args(["-Command", "$w = New-Object -ComObject WScript.Shell; $w.SendKeys([char]177)"])
            .status();
        return true;
    }

    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("osascript")
            .args(["-e", "tell application \"Spotify\" to previous track"])
            .status();
        let _ = Command::new("osascript")
            .args(["-e", "tell application \"Music\" to previous track"])
            .status();
        return true;
    }

    #[allow(unreachable_code)]
    false
}

fn open_browser_detached(url: &str) {
    #[cfg(target_os = "windows")]
    let _ = Command::new("cmd")
        .args(["/C", "start", url])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();

    #[cfg(target_os = "macos")]
    let _ = Command::new("open")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();

    #[cfg(target_os = "linux")]
    {
        let spawned = Command::new("xdg-open")
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();

        let ok = match spawned {
            Ok(mut child) => {
                thread::sleep(Duration::from_millis(150));
                match child.try_wait() {
                    Ok(Some(status)) => status.success(),
                    Ok(None) => true,
                    Err(_) => false,
                }
            }
            Err(_) => false,
        };

        if !ok {
            let fallbacks = [
                "/snap/bin/firefox",
                "firefox",
                "sensible-browser",
                "google-chrome",
                "google-chrome-stable",
                "chromium",
                "chromium-browser",
                "brave-browser",
            ];
            for fb in fallbacks {
                if let Ok(mut c) = Command::new(fb)
                    .arg(url)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                {
                    thread::sleep(Duration::from_millis(100));
                    match c.try_wait() {
                        Ok(Some(status)) if !status.success() => continue,
                        _ => break,
                    }
                }
            }
        }
    }
}

fn open_and_play_query(query: &str) -> Result<String, String> {
    let clean = query.trim();
    let lower = clean.to_lowercase();

    // 1. Миттєве визначення URL для стандартних плейлистів без спавну сторонніх процесів
    let (play_url, reply_msg) = if clean.is_empty()
        || lower == "liked"
        || lower == "liked_music"
        || lower == "лайки"
        || lower == "мої лайки"
        || lower == "улюблені"
        || lower == "улюблена музика"
        || lower == "мій плейлист"
        || lower == "моя музика"
        || lower.contains("сподобалась")
    {
        (
            "https://music.youtube.com/watch?list=LM&index=1".to_string(),
            "Вмикаю твій плейлист 'Музика, яка сподобалась' у YouTube Music 🎵".to_string(),
        )
    } else if lower == "my_mix"
        || lower == "mix"
        || lower == "рекомендації"
        || lower == "мікс"
        || lower == "мої рекомендації"
        || lower == "супермікс"
        || lower.contains("рекомендац")
    {
        (
            "https://music.youtube.com/watch?list=RDMM".to_string(),
            "Вмикаю твій персональний мікс рекомендацій у YouTube Music 🎵".to_string(),
        )
    } else if clean.starts_with("http://") || clean.starts_with("https://") {
        let mut url = clean.to_string();
        if url.contains("playlist?list=") {
            url = url.replace("playlist?list=", "watch?list=");
        }
        if url.contains("watch?list=") && !url.contains("&index=") && !url.contains("&v=") {
            url.push_str("&index=1");
        }
        (url, "Відкриваю вказане медіа-посилання 🎵".to_string())
    } else {
        // Довільний запит (назва пісні / виконавця) — швидко знаходимо прямий video id без відкриття браузера
        let py_search = r#"
import sys, urllib.request, urllib.parse, re

raw = sys.argv[1].strip() if len(sys.argv) > 1 else ""
play_url = None
if raw:
    try:
        q = urllib.parse.quote_plus(raw)
        url = f"https://www.youtube.com/results?search_query={q}"
        req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36"})
        html = urllib.request.urlopen(req, timeout=3).read().decode('utf-8', errors='ignore')
        matches = re.findall(r"watch\?v=([a-zA-Z0-9_-]{11})", html)
        if matches:
            play_url = f"https://music.youtube.com/watch?v={matches[0]}"
    except Exception:
        pass

if not play_url:
    play_url = f"https://music.youtube.com/search?q={urllib.parse.quote_plus(raw)}"

print("URL:" + play_url)
"#;
        let found_url = if let Ok(out) = Command::new("python3")
            .args(["-c", py_search, clean])
            .stdin(Stdio::null())
            .output()
        {
            let s = String::from_utf8_lossy(&out.stdout);
            s.lines()
                .find(|l| l.starts_with("URL:"))
                .map(|l| l.trim_start_matches("URL:").trim().to_string())
        } else {
            None
        };

        let final_url = found_url.unwrap_or_else(|| {
            format!("https://music.youtube.com/search?q={}", clean)
        });

        (final_url, format!("Увімкнено відтворення музики ('{}') 🎵", clean))
    };

    // 2. Запускаємо браузер повністю від'єднаним від Rust, щоб жодні дескриптори stdout/stderr не утримували виклик
    open_browser_detached(&play_url);

    // 3. У фоновому потоці через uinput та MPRIS знімаємо паузу/автоплей, якщо браузер заблокував старт
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(2200));
        if resume_media_player() {
            return;
        }
        for delay_ms in [2000, 2500] {
            std::thread::sleep(Duration::from_millis(delay_ms));
            if kickstart_browser_playback() {
                break;
            }
        }
    });

    // 4. Миттєво повертаємо результат фронтенду!
    Ok(reply_msg)
}

fn is_generic_music_query(q: &str) -> bool {
    let lower = q.trim().to_lowercase();
    matches!(
        lower.as_str(),
        "" | "liked"
            | "liked_music"
            | "лайки"
            | "мої лайки"
            | "улюблені"
            | "улюблена музика"
            | "мій плейлист"
            | "моя музика"
            | "музика"
            | "музику"
            | "музики"
            | "музичко"
            | "music"
            | "track"
            | "трек"
            | "пісня"
            | "пісню"
            | "song"
            | "плей"
            | "play"
    ) || lower.contains("сподобалась")
}

/// Керує відтворенням медіа: натискає Play/Pause на активному плеєрі або запускає прямий трек.
#[tauri::command]
async fn control_media(
    app: tauri::AppHandle,
    action: String,
    query: Option<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let action_clean = action.trim().to_lowercase();
        let query_clean = query.as_deref().map(str::trim).filter(|s| !s.is_empty());

        if let Some(q) = query_clean {
            let is_generic = is_generic_music_query(q);
            // Якщо запитано відтворення улюблених треків або загальну музику, спочатку пробуємо відновити вже відкритий плеєр на паузі
            if (action_clean == "play" || action_clean == "open_and_play" || action_clean == "play_query")
                && is_generic
            {
                if resume_media_player() {
                    LAST_KNOWN_MUSIC_PLAYING.store(true, Ordering::Relaxed);
                    let _ = app.emit("media-playback-changed", true);
                    return Ok("Продовжую відтворення музики ▶️".to_string());
                }
            }

            if action_clean == "play_query" || action_clean == "open_and_play" || (action_clean == "play" && !q.is_empty()) {
                let res = open_and_play_query(q);
                if res.is_ok() {
                    LAST_KNOWN_MUSIC_PLAYING.store(true, Ordering::Relaxed);
                    let _ = app.emit("media-playback-changed", true);
                }
                return res;
            }
        }

        if action_clean == "pause" {
            let handled = pause_media_player();
            LAST_KNOWN_MUSIC_PLAYING.store(false, Ordering::Relaxed);
            let _ = app.emit("media-playback-changed", false);
            if handled {
                return Ok("Музику поставлено на паузу ⏸️".to_string());
            } else {
                return Ok("Команда паузи надіслана ⏸️".to_string());
            }
        }

        if action_clean == "next" {
            let handled = next_media_player();
            LAST_KNOWN_MUSIC_PLAYING.store(true, Ordering::Relaxed);
            let _ = app.emit("media-playback-changed", true);
            if handled {
                return Ok("Увімкнено наступний трек ⏭️".to_string());
            } else {
                return Ok("Команда наступного треку надіслана ⏭️".to_string());
            }
        }

        if action_clean == "prev" || action_clean == "previous" {
            let handled = prev_media_player();
            LAST_KNOWN_MUSIC_PLAYING.store(true, Ordering::Relaxed);
            let _ = app.emit("media-playback-changed", true);
            if handled {
                return Ok("Увімкнено попередній трек ⏮️".to_string());
            } else {
                return Ok("Команда попереднього треку надіслана ⏮️".to_string());
            }
        }

        // Дія "play" без query:
        // 1. Спочатку пробуємо відновити вже відкритий плеєр у системі (браузер YouTube Music / Spotify тощо)
        let resumed = resume_media_player();
        if resumed {
            LAST_KNOWN_MUSIC_PLAYING.store(true, Ordering::Relaxed);
            let _ = app.emit("media-playback-changed", true);
            return Ok("Продовжую відтворення музики ▶️".to_string());
        }

        // 2. Якщо плеєр не був активний — вмикаємо особистий плейлист уподобань користувача (Liked Music)
        let res = open_and_play_query("liked_music");
        if res.is_ok() {
            LAST_KNOWN_MUSIC_PLAYING.store(true, Ordering::Relaxed);
            let _ = app.emit("media-playback-changed", true);
        }
        res
    })
    .await
    .map_err(|e| format!("Помилка обробки медіа: {}", e))?
}

fn check_is_music_playing() -> bool {
    #[cfg(target_os = "linux")]
    {
        let py_code = r#"
import sys, dbus
try:
    bus = dbus.SessionBus()
    players = [s for s in bus.list_names() if s.startswith('org.mpris.MediaPlayer2.')]
    if not players:
        sys.exit(1)
    found_any = False
    for p in players:
        try:
            player_obj = bus.get_object(p, '/org/mpris/MediaPlayer2', introspect=False)
            props = dbus.Interface(player_obj, 'org.freedesktop.DBus.Properties')
            status = str(props.Get('org.mpris.MediaPlayer2.Player', 'PlaybackStatus'))
            found_any = True
            if status.lower() == 'playing':
                sys.exit(0)
        except Exception:
            pass
    if found_any:
        sys.exit(2)
except Exception:
    pass
sys.exit(1)
"#;
        if let Some(status) = run_python_mpris_cmd(py_code) {
            let is_playing = status.code() == Some(0);
            LAST_KNOWN_MUSIC_PLAYING.store(is_playing, Ordering::Relaxed);
            return is_playing;
        } else {
            LAST_KNOWN_MUSIC_PLAYING.store(false, Ordering::Relaxed);
            return false;
        }
    }

    #[cfg(target_os = "macos")]
    {
        let script = r#"
tell application "System Events"
    if (exists process "Spotify") then
        tell application "Spotify" to if player state is playing then return "playing"
    end if
    if (exists process "Music") then
        tell application "Music" to if player state is playing then return "playing"
    end if
end tell
"#;
        if let Ok(out) = Command::new("osascript").args(["-e", script]).output() {
            let s = String::from_utf8_lossy(&out.stdout);
            if s.trim() == "playing" {
                LAST_KNOWN_MUSIC_PLAYING.store(true, Ordering::Relaxed);
                return true;
            } else {
                LAST_KNOWN_MUSIC_PLAYING.store(false, Ordering::Relaxed);
                return false;
            }
        }
    }

    #[allow(unreachable_code)]
    LAST_KNOWN_MUSIC_PLAYING.load(Ordering::Relaxed)
}

#[tauri::command]
async fn is_music_playing() -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(check_is_music_playing)
        .await
        .map_err(|e| format!("Помилка перевірки музики: {}", e))
}

const MAX_MEMORY_FACTS: usize = 25;
const MAX_FACT_CHARS: usize = 220;
const MAX_RECENT_MESSAGES: usize = 10;
const MAX_MESSAGE_CHARS: usize = 450;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
pub struct MemoryMessage {
    pub role: String,
    pub text: String,
    pub timestamp: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
pub struct AgentMemory {
    #[serde(default)]
    pub facts: Vec<String>,
    #[serde(default)]
    pub recent_messages: Vec<MemoryMessage>,
}

fn truncate_memory_text(input: &str, max_chars: usize) -> String {
    let trimmed = input.trim();
    if trimmed.chars().count() <= max_chars {
        trimmed.to_string()
    } else {
        let mut s: String = trimmed.chars().take(max_chars).collect();
        s.push_str("…");
        s
    }
}

pub fn get_memory_file_path() -> Result<PathBuf, String> {
    let config_path = get_user_config_path()?;
    if let Some(parent) = config_path.parent() {
        return Ok(parent.join("memory.json"));
    }
    let home = get_home_dir()?;
    Ok(home.join(".config").join("local-helper-ai").join("memory.json"))
}

pub fn get_agent_log_path() -> Result<PathBuf, String> {
    let config_path = get_user_config_path()?;
    if let Some(parent) = config_path.parent() {
        return Ok(parent.join("agent.jsonl"));
    }
    let home = get_home_dir()?;
    Ok(home.join(".config").join("local-helper-ai").join("agent.jsonl"))
}

fn append_agent_log_sync(user_text: &str, model_text: &str) -> Result<(), String> {
    let log_path = get_agent_log_path()?;
    if let Some(parent) = log_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut lines = Vec::new();
    if log_path.exists() {
        if let Ok(content) = fs::read_to_string(&log_path) {
            lines = content
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.to_string())
                .collect();
        }
    }
    let entry = serde_json::json!({
        "timestamp": Local::now().to_rfc3339(),
        "user_query": user_text,
        "reply_preview": truncate_memory_text(model_text, 120),
    });
    lines.push(entry.to_string());
    if lines.len() > 10 {
        let excess = lines.len() - 10;
        lines.drain(0..excess);
    }
    let new_content = lines.join("\n") + "\n";
    let _ = fs::write(&log_path, new_content);
    Ok(())
}

fn read_agent_memory_sync() -> AgentMemory {
    if let Ok(path) = get_memory_file_path() {
        if path.exists() {
            if let Ok(raw) = fs::read_to_string(&path) {
                if let Ok(mem) = serde_json::from_str::<AgentMemory>(&raw) {
                    return mem;
                }
            }
        }
    }
    AgentMemory::default()
}

fn write_agent_memory_sync(mem: &AgentMemory) -> Result<(), String> {
    let path = get_memory_file_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            format!(
                "Не вдалося створити директорію пам'яті {}: {}",
                parent.display(),
                e
            )
        })?;
    }
    let json = serde_json::to_string_pretty(mem)
        .map_err(|e| format!("Помилка серіалізації пам'яті: {}", e))?;
    fs::write(&path, json).map_err(|e| {
        format!(
            "Не вдалося зберегти пам'ять у {}: {}",
            path.display(),
            e
        )
    })
}

#[tauri::command]
async fn get_agent_memory() -> Result<AgentMemory, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let _guard = MEMORY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        read_agent_memory_sync()
    })
    .await
    .map_err(|e| format!("Помилка читання пам'яті: {}", e))
}

#[tauri::command]
async fn append_conversation_turn(
    user_text: Option<String>,
    model_text: Option<String>,
    #[allow(non_snake_case)]
    userText: Option<String>,
    #[allow(non_snake_case)]
    modelText: Option<String>,
) -> Result<AgentMemory, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let clean_user_arg = user_text.or(userText).unwrap_or_default();
        let clean_model_arg = model_text.or(modelText).unwrap_or_default();

        let _guard = MEMORY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut mem = read_agent_memory_sync();
        let now = Local::now().format("%Y-%m-%d %H:%M").to_string();

        let clean_user = truncate_memory_text(&clean_user_arg, MAX_MESSAGE_CHARS);
        let clean_model = truncate_memory_text(&clean_model_arg, MAX_MESSAGE_CHARS);

        let _ = append_agent_log_sync(&clean_user, &clean_model);

        if !clean_user.is_empty() {
            mem.recent_messages.push(MemoryMessage {
                role: "user".to_string(),
                text: clean_user,
                timestamp: now.clone(),
            });
        }
        if !clean_model.is_empty() {
            mem.recent_messages.push(MemoryMessage {
                role: "model".to_string(),
                text: clean_model,
                timestamp: now,
            });
        }

        if mem.recent_messages.len() > MAX_RECENT_MESSAGES {
            let drain_count = mem.recent_messages.len() - MAX_RECENT_MESSAGES;
            mem.recent_messages.drain(0..drain_count);
        }

        write_agent_memory_sync(&mem)?;
        Ok(mem)
    })
    .await
    .map_err(|e| format!("Помилка збереження діалогу: {}", e))?
}

#[tauri::command]
async fn manage_memory(action: String, fact: Option<String>) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = MEMORY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut mem = read_agent_memory_sync();
        let act = action.trim().to_lowercase();
        let raw_fact = fact.unwrap_or_default();
        let clean_fact = truncate_memory_text(&raw_fact, MAX_FACT_CHARS);

        match act.as_str() {
            "remember" | "add" | "save" => {
                if clean_fact.is_empty() {
                    return Err("Порожній факт для запам'ятовування".to_string());
                }
                let lower = clean_fact.to_lowercase();
                if !mem.facts.iter().any(|f| f.to_lowercase() == lower) {
                    mem.facts.push(clean_fact.clone());
                    if mem.facts.len() > MAX_MEMORY_FACTS {
                        let excess = mem.facts.len() - MAX_MEMORY_FACTS;
                        mem.facts.drain(0..excess);
                    }
                    write_agent_memory_sync(&mem)?;
                }
                Ok(format!("Запам'ятав: {}", clean_fact))
            }
            "forget" | "remove" | "delete" => {
                if clean_fact.is_empty() {
                    return Err("Не вказано, що саме потрібно забути".to_string());
                }
                let lower = clean_fact.to_lowercase();
                let before = mem.facts.len();
                mem.facts.retain(|f| !f.to_lowercase().contains(&lower));
                let removed = before.saturating_sub(mem.facts.len());
                write_agent_memory_sync(&mem)?;
                Ok(format!("Видалено записів із пам'яті: {}", removed))
            }
            "clear_all" | "clear" => {
                mem.facts.clear();
                mem.recent_messages.clear();
                write_agent_memory_sync(&mem)?;
                Ok("Пам'ять та історію розмов повністю очищено.".to_string())
            }
            "list" | _ => {
                if mem.facts.is_empty() {
                    Ok("Довгострокова пам'ять поки порожня.".to_string())
                } else {
                    Ok(format!("Збережені факти:\n- {}", mem.facts.join("\n- ")))
                }
            }
        }
    })
    .await
    .map_err(|e| format!("Помилка керування пам'яттю: {}", e))?
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct ReminderItem {
    pub id: String,
    pub text: String,
    pub target_time: String,
    pub timestamp_epoch: i64,
    #[serde(default)]
    pub completed: bool,
    #[serde(default)]
    pub is_timer: bool,
}

pub fn get_reminders_file_path() -> Result<PathBuf, String> {
    let config_path = get_user_config_path()?;
    if let Some(parent) = config_path.parent() {
        return Ok(parent.join("reminders.json"));
    }
    let home = get_home_dir()?;
    Ok(home.join(".config").join("local-helper-ai").join("reminders.json"))
}

fn read_reminders_sync() -> Vec<ReminderItem> {
    if let Ok(path) = get_reminders_file_path() {
        if path.exists() {
            if let Ok(raw) = fs::read_to_string(&path) {
                if let Ok(items) = serde_json::from_str::<Vec<ReminderItem>>(&raw) {
                    return items;
                }
            }
        }
    }
    Vec::new()
}

fn write_reminders_sync(items: &[ReminderItem]) -> Result<(), String> {
    let path = get_reminders_file_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            format!(
                "Не вдалося створити директорію для нагадувань {}: {}",
                parent.display(),
                e
            )
        })?;
    }
    let json = serde_json::to_string_pretty(items)
        .map_err(|e| format!("Помилка серіалізації нагадувань: {}", e))?;
    fs::write(&path, json).map_err(|e| {
        format!(
            "Не вдалося зберегти нагадування у {}: {}",
            path.display(),
            e
        )
    })
}

fn trigger_system_alert(title: &str, message: &str) {
    #[cfg(target_os = "linux")]
    {
        // 1. Системне сповіщення (без зайвих прапорців -a/-i, які можуть конфліктувати у Wayland)
        let _ = Command::new("notify-send")
            .args(["-u", "critical", title, message])
            .spawn();

        // 2. Системний звук (PipeWire pw-play, canberra-gtk-play, aplay)
        let sound_paths = [
            "/usr/share/sounds/freedesktop/stereo/complete.oga",
            "/usr/share/sounds/freedesktop/stereo/bell.oga",
            "/usr/share/sounds/ubuntu/notifications/Blip.ogg",
            "/usr/share/sounds/gnome/default/alerts/glass.ogg",
        ];

        let mut played = false;
        for sp in &sound_paths {
            if Path::new(sp).exists() {
                if let Ok(child) = Command::new("pw-play").arg(sp).spawn() {
                    played = true;
                    let _ = child;
                    break;
                }
            }
        }

        if !played {
            let _ = Command::new("canberra-gtk-play")
                .args(["-i", "bell"])
                .spawn();
        }

        if !played {
            for sp in &sound_paths {
                if Path::new(sp).exists() {
                    let _ = Command::new("aplay").arg(sp).spawn();
                    break;
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("powershell")
            .args(["-Command", "[System.Media.SystemSounds]::Asterisk.Play();"])
            .spawn();
    }

    #[cfg(target_os = "macos")]
    {
        let script = format!("display notification \"{}\" with title \"{}\" sound name \"Glass\"", message, title);
        let _ = Command::new("osascript").args(["-e", &script]).spawn();
    }
}

fn parse_target_timestamp(target_time_str: &str) -> Result<i64, String> {
    let mut trimmed = target_time_str.trim().replace(';', ":");
    // Якщо передано час через пробіл (наприклад, "4 44" -> "4:44")
    if let Some(pos) = trimmed.find(' ') {
        let (first, second) = trimmed.split_at(pos);
        let sec_trimmed = second.trim();
        if !first.is_empty() && first.chars().all(|c| c.is_ascii_digit())
            && !sec_trimmed.is_empty() && sec_trimmed.chars().all(|c| c.is_ascii_digit())
        {
            trimmed = format!("{}:{}", first, sec_trimmed);
        }
    }

    if let Ok(dt) = DateTime::parse_from_rfc3339(&trimmed) {
        return Ok(dt.timestamp());
    }

    let dt_formats = [
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M",
        "%d.%m.%Y %H:%M:%S",
        "%d.%m.%Y %H:%M",
    ];
    for fmt in dt_formats {
        if let Ok(naive) = NaiveDateTime::parse_from_str(&trimmed, fmt) {
            if let Some(local_dt) = Local.from_local_datetime(&naive).single() {
                return Ok(local_dt.timestamp());
            }
        }
    }

    let time_formats = [
        "%H:%M:%S",
        "%H:%M",
        "%k:%M:%S",
        "%k:%M",
        "%I:%M:%S %p",
        "%I:%M %p",
    ];
    for fmt in time_formats {
        if let Ok(time) = NaiveTime::parse_from_str(&trimmed, fmt) {
            let today = Local::now().date_naive();
            let now_epoch = Local::now().timestamp();
            let naive_dt = today.and_time(time);
            if let Some(local_dt) = Local.from_local_datetime(&naive_dt).single() {
                let mut epoch = local_dt.timestamp();
                if epoch <= now_epoch {
                    let tomorrow = today + ChronoDuration::days(1);
                    if let Some(tomorrow_dt) = Local.from_local_datetime(&tomorrow.and_time(time)).single() {
                        epoch = tomorrow_dt.timestamp();
                    }
                }
                return Ok(epoch);
            }
        }
    }

    Err(format!(
        "Не вдалося розпізнати дату й час '{}'. Очікується формат YYYY-MM-DDTHH:MM:SS, YYYY-MM-DD HH:MM або час HH:MM",
        trimmed
    ))
}

#[tauri::command(rename_all = "snake_case")]
async fn manage_reminders(
    action: String,
    text: Option<String>,
    delay_seconds: Option<i64>,
    target_time: Option<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = REMINDERS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut reminders = read_reminders_sync();
        let now_dt = Local::now();
        let now_epoch = now_dt.timestamp();
        let act = action.trim().to_lowercase();

        match act.as_str() {
            "create" | "add" | "set" => {
                let raw_text = text.unwrap_or_default();
                let clean_text = raw_text.trim();
                if clean_text.is_empty() {
                    return Err("Потрібно вказати текст нагадування (про що нагадати).".to_string());
                }

                let (target_epoch, is_timer) = if let Some(secs) = delay_seconds {
                    if secs <= 0 {
                        return Err("Кількість секунд таймера має бути більшою за 0.".to_string());
                    }
                    (now_epoch + secs, true)
                } else if let Some(ref t_str) = target_time {
                    (parse_target_timestamp(t_str)?, false)
                } else {
                    return Err("Потрібно вказати delay_seconds (наприклад, 900 для 15 хв) або target_time (наприклад, 2026-10-22T10:00:00).".to_string());
                };

                if target_epoch <= now_epoch {
                    return Err("Час нагадування вже минув! Вкажіть час у майбутньому.".to_string());
                }

                let target_dt_str = Local
                    .timestamp_opt(target_epoch, 0)
                    .single()
                    .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| target_epoch.to_string());

                // Захист від дублікатів: якщо активне нагадування з таким самим текстом та часом (±60с) вже існує — оновлюємо його
                let lower_text = clean_text.to_lowercase();
                if let Some(existing) = reminders.iter_mut().find(|r| {
                    !r.completed
                        && r.text.to_lowercase() == lower_text
                        && (r.timestamp_epoch - target_epoch).abs() <= 60
                }) {
                    existing.timestamp_epoch = target_epoch;
                    existing.target_time = target_dt_str.clone();
                    existing.is_timer = is_timer;
                } else {
                    let id = format!("rem_{}", Local::now().timestamp_millis());
                    reminders.push(ReminderItem {
                        id,
                        text: clean_text.to_string(),
                        target_time: target_dt_str.clone(),
                        timestamp_epoch: target_epoch,
                        completed: false,
                        is_timer,
                    });
                }

                write_reminders_sync(&reminders)?;

                let diff_secs = target_epoch - now_epoch;
                let human_rel = if diff_secs < 60 {
                    format!("через {} сек", diff_secs)
                } else if diff_secs < 3600 {
                    format!("через {} хв", diff_secs / 60)
                } else if diff_secs < 86400 {
                    format!("через {} год {} хв", diff_secs / 3600, (diff_secs % 3600) / 60)
                } else {
                    format!("через {} дн", diff_secs / 86400)
                };

                if is_timer {
                    Ok(format!("Поставив таймер: '{}' на {} ({}) ⏰", clean_text, human_rel, target_dt_str))
                } else {
                    Ok(format!("Запланував нагадування: '{}' на {} ({}) ⏰", clean_text, target_dt_str, human_rel))
                }
            }
            "list" | "show" => {
                let mut active: Vec<_> = reminders
                    .iter()
                    .filter(|r| !r.completed && r.timestamp_epoch > now_epoch)
                    .collect();

                if active.is_empty() {
                    return Ok("Немає активних нагадувань або таймерів.".to_string());
                }

                active.sort_by_key(|r| r.timestamp_epoch);

                let mut lines = Vec::new();
                for r in active {
                    let diff_secs = r.timestamp_epoch - now_epoch;
                    let human_rel = if diff_secs < 60 {
                        format!("через {} сек", diff_secs)
                    } else if diff_secs < 3600 {
                        format!("через {} хв", diff_secs / 60)
                    } else if diff_secs < 86400 {
                        format!("через {} год {} хв", diff_secs / 3600, (diff_secs % 3600) / 60)
                    } else {
                        format!("через {} дн", diff_secs / 86400)
                    };
                    lines.push(format!("• '{}' — {} ({})", r.text, r.target_time, human_rel));
                }
                Ok(format!("Активні нагадування:\n{}", lines.join("\n")))
            }
            "cancel" | "delete" | "remove" => {
                let raw_text = text.unwrap_or_default();
                let clean_text = raw_text.trim().to_lowercase();
                if clean_text.is_empty() || clean_text == "all" || clean_text == "все" || clean_text == "всі" {
                    let count = reminders.iter().filter(|r| !r.completed).count();
                    for r in &mut reminders {
                        r.completed = true;
                    }
                    write_reminders_sync(&reminders)?;
                    return Ok(format!("Скасовано всі активні нагадування ({}) ❌", count));
                }

                let mut cancelled_count = 0;
                for r in &mut reminders {
                    if !r.completed && (r.text.to_lowercase().contains(&clean_text) || r.id == clean_text) {
                        r.completed = true;
                        cancelled_count += 1;
                    }
                }
                write_reminders_sync(&reminders)?;

                if cancelled_count > 0 {
                    Ok(format!("Скасовано нагадувань: {} ❌", cancelled_count))
                } else {
                    Ok(format!("Не знайдено активного нагадування за запитом '{}'.", raw_text))
                }
            }
            _ => Err("Невідома дія для нагадувань. Доступні: create, list, cancel.".to_string()),
        }
    })
    .await
    .map_err(|e| format!("Помилка обробки нагадування: {}", e))?
}

/// Відкриває зовнішнє посилання у системному браузері за замовчуванням.
#[tauri::command]
async fn open_external_url(url: String) -> Result<(), String> {
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
async fn set_window_expanded(app: tauri::AppHandle, expanded: bool) -> Result<(), String> {
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

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct ActiveWorkspaceInfo {
    pub workspace_path: Option<String>,
    pub active_file: Option<String>,
    pub source: String,
}

fn detect_active_workspace_internal() -> ActiveWorkspaceInfo {
    let py_script = r#"
import os, sys, json, glob, sqlite3, urllib.parse

def detect():
    home = os.path.expanduser('~')
    workspace_path = None
    active_file = None
    source = 'none'

    for app_name in ['Code', 'VSCodium', 'Cursor']:
        code_config = os.path.join(home, '.config', app_name)
        storage_json = os.path.join(code_config, 'User', 'globalStorage', 'storage.json')
        if not os.path.isdir(code_config):
            continue

        if os.path.isfile(storage_json):
            try:
                with open(storage_json, 'r', encoding='utf-8') as f:
                    data = json.load(f)
                win_state = data.get('windowsState', {})
                last_win = win_state.get('lastActiveWindow', {})
                folder_uri = last_win.get('folder')
                if not folder_uri and win_state.get('openedWindows'):
                    folder_uri = win_state['openedWindows'][0].get('folder')
                if folder_uri and folder_uri.startswith('file://'):
                    workspace_path = urllib.parse.unquote(folder_uri[7:])
                    source = f'{app_name}_storage'
            except Exception:
                pass

        ws_storage = os.path.join(code_config, 'User', 'workspaceStorage')
        if os.path.isdir(ws_storage):
            matched_ws_dir = None
            best_mtime = 0
            for ws_json in glob.glob(os.path.join(ws_storage, '*', 'workspace.json')):
                try:
                    with open(ws_json, 'r', encoding='utf-8') as f:
                        ws_d = json.load(f)
                    f_uri = ws_d.get('folder', '')
                    f_path = urllib.parse.unquote(f_uri[7:]) if f_uri.startswith('file://') else ''
                    mtime = os.path.getmtime(ws_json)
                    if workspace_path and f_path == workspace_path:
                        matched_ws_dir = os.path.dirname(ws_json)
                        break
                    elif mtime > best_mtime:
                        best_mtime = mtime
                        matched_ws_dir = os.path.dirname(ws_json)
                except Exception:
                    pass

            if matched_ws_dir:
                db_path = os.path.join(matched_ws_dir, 'state.vscdb')
                if os.path.isfile(db_path):
                    try:
                        conn = sqlite3.connect(db_path)
                        cur = conn.cursor()
                        cur.execute("SELECT value FROM ItemTable WHERE key = 'history.entries'")
                        row = cur.fetchone()
                        if row:
                            val = row[0]
                            val_str = val.decode('utf-8') if isinstance(val, bytes) else str(val)
                            entries = json.loads(val_str)
                            if entries and isinstance(entries, list):
                                first = entries[0]
                                res = first.get('editor', {}).get('resource', '')
                                if res.startswith('file://'):
                                    active_file = urllib.parse.unquote(res[7:])
                        conn.close()
                    except Exception:
                        pass
        if workspace_path or active_file:
            break

    print(json.dumps({'workspace_path': workspace_path, 'active_file': active_file, 'source': source}))

detect()
"#;

    if let Ok(output) = Command::new("python3").args(["-c", py_script]).output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Ok(info) = serde_json::from_str::<ActiveWorkspaceInfo>(stdout.trim()) {
                if info.workspace_path.is_some() || info.active_file.is_some() {
                    return info;
                }
            }
        }
    }

    // Резервний варіант (чистий Rust без Python)
    let home = env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let storage_path = PathBuf::from(&home).join(".config/Code/User/globalStorage/storage.json");
    if storage_path.exists() {
        if let Ok(content) = fs::read_to_string(storage_path) {
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(folder) = data.pointer("/windowsState/lastActiveWindow/folder").and_then(|v| v.as_str()) {
                    if let Some(stripped) = folder.strip_prefix("file://") {
                        return ActiveWorkspaceInfo {
                            workspace_path: Some(stripped.to_string()),
                            active_file: None,
                            source: "code_storage_rust".to_string(),
                        };
                    }
                }
            }
        }
    }

    ActiveWorkspaceInfo {
        workspace_path: None,
        active_file: None,
        source: "none".to_string(),
    }
}

/// Повертає поточний активний проект (робочу папку) та відкритий файл у VS Code
#[tauri::command]
async fn get_active_workspace() -> Result<ActiveWorkspaceInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        Ok(detect_active_workspace_internal())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

/// Отримує чистий текст активної або вказаної веб-сторінки та метадані для створення саммарі.
#[tauri::command]
async fn summarize_webpage(
    url: Option<String>,
    focus: Option<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let clean_url = url.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or("");
        let clean_focus = focus.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or("");

        let py_script = r#"
import sys, os, glob, json, re, urllib.request, urllib.parse, ssl, subprocess, time, struct, sqlite3, shutil, tempfile

target_url = sys.argv[1].strip() if len(sys.argv) > 1 and sys.argv[1].strip() else None
focus_query = sys.argv[2].strip() if len(sys.argv) > 2 and sys.argv[2].strip() else None

def is_valid_web_url(u):
    if not u or not isinstance(u, str):
        return False
    u = u.strip()
    if not (u.startswith('http://') or u.startswith('https://')):
        return False
    lower = u.lower()
    ignore = (
        'chrome-extension://', 'chrome://', 'moz-extension://', 'about:',
        'devtools://', 'edge://', 'brave://', 'vivaldi://', 'opera://',
        'newtab', 'new-tab-page', 'favicon', 'google.com/search',
        'accounts.google.com/rotatecookiespage', 'ogs.google.com'
    )
    return not any(x in lower for x in ignore)

def decompress_mozlz4(data):
    if data[:8] != b'mozLz40\0':
        raise ValueError('Not mozLz4')
    try:
        import lz4.block
        return lz4.block.decompress(data[8:])
    except Exception:
        pass
    expected_size = struct.unpack('<I', data[8:12])[0]
    src = memoryview(data)[12:]
    out = bytearray()
    i = 0
    n = len(src)
    while i < n:
        token = src[i]
        i += 1
        lit_len = token >> 4
        if lit_len == 15:
            while i < n:
                b = src[i]
                i += 1
                lit_len += b
                if b != 255:
                    break
        out.extend(src[i:i + lit_len])
        i += lit_len
        if i >= n:
            break
        offset = src[i] | (src[i + 1] << 8)
        i += 2
        if offset == 0:
            break
        match_len = (token & 0x0F) + 4
        if match_len == 19:
            while i < n:
                b = src[i]
                i += 1
                match_len += b
                if b != 255:
                    break
        start = len(out) - offset
        for j in range(match_len):
            out.append(out[start + j])
    return bytes(out[:expected_size])

def get_live_browser_info():
    ff_running = False
    ch_running = False
    try:
        out = subprocess.check_output(['ps', '-eo', 'comm,args'], stderr=subprocess.DEVNULL, timeout=1.2).decode('utf-8', errors='ignore')
        for line in out.splitlines():
            l = line.lower().strip()
            if not l or 'python3' in l:
                continue
            parts = l.split(None, 1)
            comm = parts[0] if parts else ''
            if any(k in comm for k in ('firefox', 'zen', 'librewolf', 'waterfox')) or '/firefox' in l or 'snap/firefox' in l:
                ff_running = True
            if any(k in comm for k in ('chrome', 'chromium', 'brave', 'msedge', 'vivaldi', 'opera')):
                if '--headless' not in l and '--type=' not in l:
                    ch_running = True
    except Exception:
        pass

    ff_titles = []
    ch_titles = []
    atspi_code = r'''
import json, gi
gi.require_version('Atspi', '2.0')
from gi.repository import Atspi
ff, ch = [], []
desktop = Atspi.get_desktop(0)
for i in range(desktop.get_child_count()):
    app = desktop.get_child_at_index(i)
    if not app:
        continue
    for j in range(app.get_child_count()):
        frame = app.get_child_at_index(j)
        if not frame:
            continue
        title = (frame.get_name() or '').strip()
        if not title:
            continue
        for suf in (' — Mozilla Firefox', ' - Mozilla Firefox', ' — Firefox', ' - Firefox', ' — Zen Browser', ' - Zen Browser'):
            if title.endswith(suf):
                t = title[:-len(suf)].strip()
                if t:
                    ff.append(t)
                break
        for suf in (' - Google Chrome', ' - Chromium', ' - Brave', ' - Microsoft Edge'):
            if title.endswith(suf):
                t = title[:-len(suf)].strip()
                if t:
                    ch.append(t)
                break
print(json.dumps({'ff': ff, 'ch': ch}))
'''
    try:
        raw = subprocess.check_output(['python3', '-c', atspi_code], stderr=subprocess.DEVNULL, timeout=1.0).decode('utf-8', errors='ignore').strip()
        if raw:
            parsed = json.loads(raw.splitlines()[-1])
            ff_titles = parsed.get('ff') or []
            ch_titles = parsed.get('ch') or []
    except Exception:
        pass

    return ff_running, ch_running, ff_titles, ch_titles

def query_sqlite_readonly(db_path, sql):
    if not os.path.exists(db_path):
        return []
    try:
        conn = sqlite3.connect(f'file:{db_path}?mode=ro', uri=True, timeout=0.8)
        rows = conn.execute(sql).fetchall()
        conn.close()
        return rows
    except Exception:
        pass
    tmpdir = tempfile.mkdtemp(prefix='lh_db_')
    try:
        base = os.path.basename(db_path)
        dst = os.path.join(tmpdir, base)
        shutil.copy2(db_path, dst)
        for ext in ('-wal', '-shm', '-journal'):
            if os.path.exists(db_path + ext):
                shutil.copy2(db_path + ext, dst + ext)
        conn = sqlite3.connect(dst, timeout=0.8)
        rows = conn.execute(sql).fetchall()
        conn.close()
        return rows
    except Exception:
        return []
    finally:
        shutil.rmtree(tmpdir, ignore_errors=True)

def title_matches(tab_title, live_titles):
    if not tab_title or not live_titles:
        return False
    tt = tab_title.strip().lower()
    if not tt:
        return False
    for lt in live_titles:
        l = lt.strip().lower()
        if l and (tt == l or tt in l or l in tt):
            return True
    return False

def get_firefox_candidate(home, ff_running, ff_titles):
    ff_roots = [
        os.path.join(home, 'snap/firefox/common/.mozilla/firefox'),
        os.path.join(home, '.mozilla/firefox'),
        os.path.join(home, '.var/app/org.mozilla.firefox/.mozilla/firefox'),
        os.path.join(home, '.config/mozilla/firefox'),
        os.path.join(home, '.zen'),
        os.path.join(os.environ.get('APPDATA', ''), 'Mozilla/Firefox/Profiles'),
        os.path.join(home, 'Library/Application Support/Firefox/Profiles'),
    ]
    session_files = []
    for root in ff_roots:
        if not root or not os.path.isdir(root):
            continue
        for sub in (
            '*/sessionstore-backups/recovery.jsonlz4',
            '*/sessionstore-backups/recovery.baklz4',
            '*/sessionstore.jsonlz4',
            '*/sessionstore-backups/previous.jsonlz4',
        ):
            for f in glob.glob(os.path.join(root, sub)):
                if os.path.isfile(f):
                    session_files.append(f)

    if not session_files:
        return None

    # Сортуємо всі знайдені сесії Firefox за часом модифікації (найновіші першими)
    session_files.sort(key=lambda p: (
        1 if os.path.basename(p).startswith('recovery.') else 0,
        os.path.getmtime(p)
    ), reverse=True)

    now = time.time()
    for fpath in session_files:
        try:
            prof_dir = os.path.dirname(os.path.dirname(fpath)) if 'sessionstore-backups' in fpath else os.path.dirname(fpath)
            has_lock = os.path.islink(os.path.join(prof_dir, 'lock')) or os.path.exists(os.path.join(prof_dir, '.parentlock'))
            is_live_recovery = os.path.basename(fpath).startswith('recovery.')

            with open(fpath, 'rb') as f:
                raw = f.read()
            decomp = decompress_mozlz4(raw)
            j = json.loads(decomp.decode('utf-8', errors='ignore'))

            file_mtime = os.path.getmtime(fpath)
            session_update = (j.get('session', {}).get('lastUpdate', 0) or 0) / 1000.0
            windows = j.get('windows', [])
            sel_win_idx = (j.get('selectedWindow', 1) or 1) - 1

            candidates = []
            for w_idx, win in enumerate(windows):
                tabs = win.get('tabs', [])
                sel_tab_idx = (win.get('selected', 1) or 1) - 1
                for t_idx, tab in enumerate(tabs):
                    entries = tab.get('entries', [])
                    if not entries:
                        continue
                    e_idx = (tab.get('index', len(entries)) or len(entries)) - 1
                    if not (0 <= e_idx < len(entries)):
                        e_idx = len(entries) - 1
                    entry = entries[e_idx]
                    u = (entry.get('url') or '').strip()
                    if not is_valid_web_url(u):
                        continue
                    t_title = (entry.get('title') or '').strip()
                    last_acc = (tab.get('lastAccessed', 0) or 0) / 1000.0
                    is_sel_active = 1 if (w_idx == sel_win_idx and t_idx == sel_tab_idx) else 0
                    is_sel_any = 1 if (t_idx == sel_tab_idx) else 0
                    matched_live_title = 1 if title_matches(t_title, ff_titles) else 0
                    not_music = 0 if 'music.youtube.com' in u.lower() else 1
                    eff_ts = now if (matched_live_title and not is_sel_active) else max(file_mtime, session_update, last_acc)
                    candidates.append((matched_live_title, is_sel_active, not_music, is_sel_any, last_acc, eff_ts, u))

            best_url = None
            best_ts = max(file_mtime, session_update)
            best_matched_live = 0
            if candidates:
                candidates.sort(key=lambda x: (x[0], x[1], x[2], x[3], x[4]), reverse=True)
                best = candidates[0]
                best_matched_live = best[0]
                best_url = best[6]
                best_ts = best[5]

            # Перевіряємо places.sqlite на випадок переходу на нову сторінку < 30 с тому (до скидання recovery.jsonlz4 на диск)
            places_path = os.path.join(prof_dir, 'places.sqlite')
            rows = query_sqlite_readonly(
                places_path,
                'SELECT p.url, p.title, v.visit_date FROM moz_historyvisits v JOIN moz_places p ON v.place_id = p.id WHERE p.hidden = 0 AND v.visit_type IN (1, 2, 3, 5, 6, 9) ORDER BY v.visit_date DESC LIMIT 10'
            )
            valid_rows = [
                (r_url.strip(), r_title or '', (r_date or 0) / 1_000_000.0)
                for r_url, r_title, r_date in rows
                if is_valid_web_url(r_url) and 'music.youtube.com' not in r_url.lower()
            ]
            if valid_rows:
                matched_places = False
                if ff_titles and not best_matched_live:
                    for r_url, r_title, v_ts in valid_rows:
                        if title_matches(r_title, ff_titles) and (now - v_ts) < 600.0:
                            best_url = r_url
                            best_ts = max(best_ts, v_ts, now)
                            matched_places = True
                            break
                if not matched_places:
                    r_url, r_title, v_ts = valid_rows[0]
                    if (v_ts > best_ts + 0.5 and (now - v_ts) < 30.0) or not best_url:
                        best_url = r_url
                        best_ts = max(best_ts, v_ts)

            if best_url:
                recently_active = (now - best_ts) < 120.0 and (is_live_recovery or has_lock)
                running = 1 if (ff_running or bool(ff_titles) or recently_active) else 0
                non_music = 0 if 'music.youtube.com' in best_url.lower() else 1
                live_win = 1 if ff_titles else 0
                return {
                    'url': best_url,
                    'ts': best_ts,
                    'running': running,
                    'live_win': live_win,
                    'non_music': non_music,
                }
        except Exception:
            pass
    return None

def get_chromium_candidate(home, ch_running, ch_titles):
    # Важливо: читаємо ЛИШЕ Session_* (поточні відкриті вкладки), а НЕ Tabs_* (закриті вкладки з минулих днів)
    session_dir_patterns = [
        os.path.join(home, '.config/google-chrome/*/Sessions'),
        os.path.join(home, '.config/google-chrome/Default/Sessions'),
        os.path.join(home, '.config/chromium/*/Sessions'),
        os.path.join(home, '.config/BraveSoftware/Brave-Browser/*/Sessions'),
        os.path.join(home, '.config/microsoft-edge/*/Sessions'),
        os.path.join(os.environ.get('LOCALAPPDATA', ''), 'Google/Chrome/User Data/*/Sessions'),
        os.path.join(home, 'Library/Application Support/Google/Chrome/*/Sessions'),
    ]
    seen_dirs = set()
    latest_sessions = []
    for pat in session_dir_patterns:
        for d in glob.glob(pat):
            real_d = os.path.realpath(d)
            if real_d in seen_dirs or not os.path.isdir(real_d):
                continue
            seen_dirs.add(real_d)
            s_files = [f for f in glob.glob(os.path.join(real_d, 'Session_*')) if os.path.isfile(f)]
            if s_files:
                # У кожній папці Sessions лише один найновіший Session_* відповідає поточній сесії
                newest = max(s_files, key=os.path.getmtime)
                latest_sessions.append(newest)

    latest_sessions.sort(key=os.path.getmtime, reverse=True)
    now = time.time()

    for s_file in latest_sessions:
        try:
            prof_dir = os.path.dirname(os.path.dirname(s_file))
            user_data_dir = os.path.dirname(prof_dir)
            lock_link = os.path.join(user_data_dir, 'SingletonLock')
            lock_alive = False
            if os.path.islink(lock_link):
                target = os.readlink(lock_link)
                pid_str = target.rsplit('-', 1)[-1] if '-' in target else ''
                if pid_str.isdigit() and os.path.exists(f'/proc/{pid_str}'):
                    lock_alive = True

            with open(s_file, 'rb') as f:
                data = f.read()
            matches = re.findall(rb'https?://[^\x00-\x1f \"\'<>]+', data)
            valid = []
            for m in matches:
                try:
                    u = m.decode('utf-8', errors='ignore').strip()
                    # Канонічні URL сторінок у Chrome завжди мають шлях '/' після домену (відсікаємо initiator_origin без '/')
                    if u.count('/') >= 3 and is_valid_web_url(u):
                        valid.append(u)
                except Exception:
                    pass

            # Якщо у поточній сесії Chrome відкрита лише порожня вкладка (chrome://newtab/), valid буде порожнім
            if not valid:
                continue

            non_music_valid = [u for u in valid if 'music.youtube.com' not in u.lower()]
            best_url = non_music_valid[-1] if non_music_valid else valid[-1]
            best_ts = os.path.getmtime(s_file)

            hist_path = os.path.join(prof_dir, 'History')
            open_set = set(valid)
            rows = query_sqlite_readonly(
                hist_path,
                'SELECT url, title, last_visit_time FROM urls ORDER BY last_visit_time DESC LIMIT 15'
            )
            valid_hist = [
                (h_url.strip(), h_title or '', ((h_time or 0) / 1_000_000.0) - 11644473600.0)
                for h_url, h_title, h_time in rows
                if is_valid_web_url(h_url) and 'music.youtube.com' not in h_url.lower()
            ]
            matched_hist = False
            if ch_titles and valid_hist:
                for h_url, h_title, h_ts in valid_hist:
                    if title_matches(h_title, ch_titles) and (h_url in open_set or (now - h_ts) < 600.0):
                        best_url = h_url
                        best_ts = max(best_ts, h_ts)
                        matched_hist = True
                        break
            if not matched_hist and valid_hist:
                for h_url, h_title, h_ts in valid_hist:
                    if (h_url in open_set and h_ts > best_ts - 3600) or (h_ts > best_ts and (now - h_ts) < 30.0):
                        best_url = h_url
                        best_ts = max(best_ts, h_ts)
                        break

            running = 1 if (ch_running or lock_alive or bool(ch_titles)) else 0
            non_music = 0 if 'music.youtube.com' in best_url.lower() else 1
            live_win = 1 if ch_titles else 0
            return {
                'url': best_url,
                'ts': best_ts,
                'running': running,
                'live_win': live_win,
                'non_music': non_music,
            }
        except Exception:
            pass
    return None

def resolve_active_url(explicit_url=None):
    if explicit_url and (explicit_url.startswith('http://') or explicit_url.startswith('https://')):
        return explicit_url.strip()

    home = os.path.expanduser('~')
    ff_running, ch_running, ff_titles, ch_titles = get_live_browser_info()

    candidates = []
    ff_cand = get_firefox_candidate(home, ff_running, ff_titles)
    if ff_cand:
        candidates.append(ff_cand)

    ch_cand = get_chromium_candidate(home, ch_running, ch_titles)
    if ch_cand:
        candidates.append(ch_cand)

    if candidates:
        candidates.sort(
            key=lambda c: (c['running'], c['live_win'], c['non_music'], c['ts']),
            reverse=True
        )
        return candidates[0]['url']

    for cmd in [['wl-paste'], ['xclip', '-o', '-selection', 'clipboard']]:
        try:
            out = subprocess.check_output(cmd, stderr=subprocess.DEVNULL, timeout=1).decode('utf-8', errors='ignore').strip()
            if out.startswith('http://') or out.startswith('https://'):
                return out
        except Exception:
            pass

    return None

url = resolve_active_url(target_url)
if not url:
    print(json.dumps({
        'success': False,
        'error': 'Не вдалося знайти активну сторінку у браузері. Будь ласка, відкрийте вкладку або вкажіть URL чи скопіюйте його в буфер обміну.'
    }, ensure_ascii=False))
    sys.exit(0)

domain = urllib.parse.urlparse(url).netloc
ua = 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36'
headers = {
    'User-Agent': ua,
    'Accept': 'text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8',
    'Accept-Language': 'uk-UA,uk;q=0.9,en-US;q=0.8,en;q=0.7',
}

html = ''
try:
    ctx = ssl.create_default_context()
    ctx.check_hostname = False
    ctx.verify_mode = ssl.CERT_NONE
    req = urllib.request.Request(url, headers=headers)
    with urllib.request.urlopen(req, context=ctx, timeout=10) as resp:
        raw = resp.read()
        charset = resp.headers.get_content_charset() or 'utf-8'
        html = raw.decode(charset, errors='ignore')
except Exception as e:
    try:
        cmd = ['curl', '-s', '-L', '-A', ua, '--max-time', '10', url]
        out = subprocess.check_output(cmd, timeout=12)
        html = out.decode('utf-8', errors='ignore')
    except Exception as e2:
        print(json.dumps({
            'success': False,
            'url': url,
            'error': f'Не вдалося завантажити сторінку ({url}): {e}'
        }, ensure_ascii=False))
        sys.exit(0)

if not html.strip():
    print(json.dumps({
        'success': False,
        'url': url,
        'error': f'Сторінка повернула порожній вміст: {url}'
    }, ensure_ascii=False))
    sys.exit(0)

title = ''
clean_text = ''

try:
    from bs4 import BeautifulSoup
    soup = BeautifulSoup(html, 'html.parser')
    for tag in soup(['script', 'style', 'nav', 'footer', 'header', 'aside', 'noscript', 'form', 'svg', 'iframe', 'button', 'input']):
        tag.decompose()
    for el in soup.find_all(class_=re.compile(r'(ad|banner|cookie|share|social|sidebar|newsletter|promo|popup|modal)', re.I)):
        el.decompose()
    for el in soup.find_all(id=re.compile(r'(ad|banner|cookie|share|social|sidebar|newsletter|promo|popup|modal)', re.I)):
        el.decompose()

    if soup.title and soup.title.string:
        title = soup.title.string.strip()
    og_title = soup.find('meta', property='og:title')
    if og_title and og_title.get('content'):
        title = og_title['content'].strip()
    h1 = soup.find('h1')
    if not title and h1:
        title = h1.get_text().strip()

    container = soup.find('article') or soup.find('main') or soup.find(role='main') or soup.find(id='content') or soup.body
    paragraphs = []
    if container:
        for p in container.find_all(['p', 'h2', 'h3', 'li']):
            text = p.get_text().strip()
            if len(text) > 25 and not any(text.lower().startswith(x) for x in ['cookie', 'copyright', 'subscribe', 'всі права']):
                paragraphs.append(text)

    clean_text = '\n\n'.join(paragraphs)
    if not clean_text:
        clean_text = soup.get_text(separator='\n\n', strip=True)
except Exception:
    title_m = re.search(r'<title[^>]*>(.*?)</title>', html, re.I | re.S)
    title = title_m.group(1).strip() if title_m else domain
    clean = re.sub(r'<(script|style|nav|header|footer|aside)[^>]*>.*?</\1>', '', html, flags=re.I | re.S)
    clean = re.sub(r'<[^>]+>', ' ', clean)
    clean_text = re.sub(r'\s+', ' ', clean).strip()

is_clipboard_fallback = False
if len(clean_text) < 75:
    clip_text = ''
    clip_cmds = []
    if sys.platform == 'darwin':
        clip_cmds = [['pbpaste']]
    elif sys.platform.startswith('win'):
        clip_cmds = [['powershell', '-NoProfile', '-Command', 'Get-Clipboard']]
    else:
        clip_cmds = [['wl-paste'], ['xclip', '-o', '-selection', 'clipboard']]

    for c in clip_cmds:
        try:
            out = subprocess.check_output(c, stderr=subprocess.DEVNULL, timeout=1).decode('utf-8', errors='ignore').strip()
            if len(out) > 50 and not out.startswith('http://') and not out.startswith('https://'):
                clip_text = out
                break
        except Exception:
            pass

    if clip_text:
        clean_text = clip_text
        is_clipboard_fallback = True

if len(clean_text) < 75:
    print(json.dumps({
        'success': False,
        'url': url,
        'title': title or domain,
        'domain': domain,
        'error': f"Сторінка ('{title or domain}') є клієнтським веб-додатком (SPA/React) або вимагає авторизації в системі. Її текстовий вміст генерується через JavaScript і недоступний для фонового скачування. Будь ласка, виділіть потрібний текст на сторінці та скопіюйте його (Ctrl+C), або надішліть скриншот."
    }, ensure_ascii=False))
    sys.exit(0)

if len(clean_text) > 12000:
    clean_text = clean_text[:12000] + '\n\n[... текст скорочено для саммарі ...]'

print(json.dumps({
    'success': True,
    'url': url,
    'title': title or domain,
    'domain': domain,
    'content': clean_text,
    'focus': focus_query,
    'clipboard_fallback': is_clipboard_fallback
}, ensure_ascii=False))
"#;

        let output = Command::new("python3")
            .args(["-c", py_script, clean_url, clean_focus])
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("Не вдалося запустити парсер сторінки (python3): {}", e))?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Помилка екстрактора: {}", err.trim()));
        }

        let raw_json = String::from_utf8_lossy(&output.stdout);
        let parsed: serde_json::Value = serde_json::from_str(raw_json.trim())
            .map_err(|e| format!("Некоректна відповідь парсера: {}. Вивід: {}", e, raw_json))?;

        let success = parsed.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
        if !success {
            let error_msg = parsed
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("Не вдалося витягнути статтю");
            return Err(error_msg.to_string());
        }

        let found_url = parsed.get("url").and_then(|v| v.as_str()).unwrap_or("");
        let title = parsed.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let domain = parsed.get("domain").and_then(|v| v.as_str()).unwrap_or("");
        let content = parsed.get("content").and_then(|v| v.as_str()).unwrap_or("");
        let focus_info = parsed.get("focus").and_then(|v| v.as_str());
        let is_clip = parsed.get("clipboard_fallback").and_then(|v| v.as_bool()).unwrap_or(false);

        let mut res = format!("Стаття: \"{}\"\nURL: {}\nДомен: {}\n", title, found_url, domain);
        if is_clip {
            res.push_str("Джерело тексту: виділений фрагмент із буфера обміну (SPA сторінка)\n");
        }
        if let Some(f) = focus_info {
            if !f.is_empty() {
                res.push_str(&format!("Фокус користувача: {}\n", f));
            }
        }
        res.push_str(&format!(
            "\n<untrusted_web_content source=\"{}\">\n{}\n</untrusted_web_content>",
            found_url, content
        ));

        Ok(res)
    })
    .await
    .map_err(|e| format!("Помилка фонового потоку: {}", e))?
}

#[cfg(target_os = "linux")]
fn ensure_linux_window_interactive(window: &tauri::WebviewWindow) {
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                // У Mutter (GNOME Wayland + XWayland) негайний виклик _NET_WM_STATE_ABOVE до завершення
                // прив'язки wl_surface викликає збій meta_window_set_stack_position_no_sync (stack_position == -1),
                // через що вікно рендериться, але стає неклікабельним (події миші проходять крізь нього).
                // Тому застосовуємо always-on-top, sticky та скидання input_shape вже після мапінгу вікна.
                #[cfg(target_os = "linux")]
                {
                    for delay_ms in [350_u64, 1200_u64, 3000_u64] {
                        let w_delayed = window.clone();
                        gtk::glib::timeout_add_local_once(
                            Duration::from_millis(delay_ms),
                            move || {
                                ensure_linux_window_interactive(&w_delayed);
                            },
                        );
                    }
                }

                #[cfg(not(target_os = "linux"))]
                {
                    let _ = window.set_ignore_cursor_events(false);
                    let _ = window.set_always_on_top(true);
                    let _ = window.set_visible_on_all_workspaces(true);
                }

                // Забезпечуємо, щоб вікно маскота завжди залишалося поверх усіх інших програм
                let startup_instant = std::time::Instant::now();
                let w = window.clone();
                window.on_window_event(move |event| {
                    if startup_instant.elapsed() < Duration::from_millis(1200) {
                        return;
                    }
                    if let tauri::WindowEvent::Focused(_) = event {
                        let _ = w.set_ignore_cursor_events(false);
                        let _ = w.set_always_on_top(true);
                    }
                });
            }

            // Фоновий планувальник нагадувань і таймерів
            let app_handle = app.handle().clone();
            std::thread::spawn(move || {
                // Чекаємо завершення ініціалізації вікна та WebView перед перевіркою пропущених нагадувань
                std::thread::sleep(Duration::from_secs(2));

                // Перевірка пропущених нагадувань при старті
                {
                    let mut reminders = read_reminders_sync();
                    let now_epoch = Local::now().timestamp();
                    let mut missed = Vec::new();
                    for r in &mut reminders {
                        if !r.completed && r.timestamp_epoch <= now_epoch {
                            r.completed = true;
                            missed.push(r.text.clone());
                        }
                    }
                    if !missed.is_empty() {
                        let _ = write_reminders_sync(&reminders);
                        let missed_text = missed.join(", ");
                        trigger_system_alert(
                            "⚠️ Пропущене нагадування!",
                            &format!("Поки маскот був вимкнений, минув час: {}", missed_text),
                        );
                        if let Some(window) = app_handle.get_webview_window("main") {
                            let _ = window.unminimize();
                            let _ = window.show();
                            let _ = window.set_always_on_top(true);
                            let _ = window.set_focus();
                        }
                        let _ = app_handle.emit(
                            "reminder-alert",
                            serde_json::json!({
                                "text": format!("Поки я був вимкнений, минув час: {}", missed_text),
                                "missed": true
                            }),
                        );
                    }
                }

                // Головний цикл відліку часу та моніторингу фонової музики
                let mut tick: u64 = 0;
                let mut last_emitted_playing = check_is_music_playing();
                let _ = app_handle.emit("media-playback-changed", last_emitted_playing);

                loop {
                    std::thread::sleep(Duration::from_secs(1));
                    tick += 1;

                    // Кожні 2 секунди перевіряємо статус відтворення музики у браузері / системі
                    if tick % 2 == 0 {
                        let is_playing = check_is_music_playing();
                        if is_playing != last_emitted_playing {
                            last_emitted_playing = is_playing;
                            let _ = app_handle.emit("media-playback-changed", is_playing);
                        }
                    }

                    let now_epoch = Local::now().timestamp();
                    let reminders_to_alert = {
                        let _guard = REMINDERS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
                        let mut reminders = read_reminders_sync();
                        let mut changed = false;
                        let mut to_alert = Vec::new();

                        for r in &mut reminders {
                            if !r.completed && r.timestamp_epoch <= now_epoch {
                                r.completed = true;
                                changed = true;
                                to_alert.push(r.clone());
                            }
                        }

                        if changed {
                            let _ = write_reminders_sync(&reminders);
                        }
                        to_alert
                    };

                    for r in reminders_to_alert {
                        trigger_system_alert("🔔 Нагадування!", &r.text);

                        if let Some(window) = app_handle.get_webview_window("main") {
                            let _ = window.unminimize();
                            let _ = window.show();
                            let _ = window.set_always_on_top(true);
                            let _ = window.set_focus();
                        }

                        let _ = app_handle.emit(
                            "reminder-alert",
                            serde_json::json!({
                                "text": r.text,
                                "id": r.id,
                                "target_time": r.target_time
                            }),
                        );
                    }
                }
            });

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
                                #[cfg(target_os = "linux")]
                                ensure_linux_window_interactive(&window);
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
            run_cli_command,
            start_voice_recording,
            stop_voice_recording,
            cancel_voice_recording,
            read_image_base64,
            read_clipboard_image,
            get_app_config,
            control_media,
            get_agent_memory,
            append_conversation_turn,
            manage_memory,
            manage_reminders,
            is_music_playing,
            summarize_webpage,
            open_external_url,
            set_window_expanded,
            get_active_workspace
        ])
        .run(tauri::generate_context!())
        .expect("Помилка запуску Tauri-застосунку");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_active_workspace_internal() {
        let info = detect_active_workspace_internal();
        // На машині розробника має визначатися workspace_path або fallback "none"
        println!("Detected workspace: {:?}", info);
        assert!(!info.source.is_empty());
    }

    #[test]
    fn test_parse_target_timestamp_various_formats() {
        assert!(parse_target_timestamp("2026-10-25T10:00:00").is_ok());
        assert!(parse_target_timestamp("2026-10-25 10:00").is_ok());
        assert!(parse_target_timestamp("10:00").is_ok());
        assert!(parse_target_timestamp("4:44").is_ok());
        assert!(parse_target_timestamp("4 44").is_ok());
        assert!(parse_target_timestamp("4;40").is_ok());
        assert!(parse_target_timestamp("04:44:00").is_ok());
    }

    #[test]
    fn test_real_append_to_obsidian() {
        let res = tauri::async_runtime::block_on(append_to_obsidian(
            None,
            "TODO.md".to_string(),
            "Тестова задача".to_string(),
            true,
            Some("TO DO".to_string()),
            Some("2026-10-25".to_string()),
        ));
        println!("append_to_obsidian res: {:?}", res);
        // У тестовому середовищі sandbox файлова система може бути read-only
        if let Err(ref e) = res {
            if e.contains("Read-only file system") {
                return;
            }
        }
        assert!(res.is_ok(), "Expected Ok, got: {:?}", res);
    }

    #[test]
    fn test_insert_into_obsidian_markdown_default_todo_section() {
        let original = "# Kanban\n";
        let out = insert_into_obsidian_markdown(original, "- [ ] Нова задача", true, None);
        assert!(out.contains("## TO DO"), "Expected ## TO DO when as_todo=true, got:\n{}", out);
        assert!(!out.contains("## Ideas"));
    }

    #[test]
    fn test_insert_into_obsidian_markdown_no_cross_section_deletion() {
        let original = "## IN PROGRESS\n- [ ] Робота 1\n\n## DONE\n- [ ] Спільна задача\n";
        let out = insert_into_obsidian_markdown(original, "- [ ] Спільна задача", true, Some("IN PROGRESS"));
        assert!(out.contains("## IN PROGRESS\n\n- [ ] Робота 1\n- [ ] Спільна задача\n"));
        assert!(out.contains("## DONE\n- [ ] Спільна задача\n"), "Entry in ## DONE must not be deleted! Result:\n{}", out);
    }

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

    #[test]
    fn test_reminder_item_dedup_logic() {
        let mut reminders = vec![ReminderItem {
            id: "rem_1".to_string(),
            text: "Випити води".to_string(),
            target_time: "2026-10-22 18:00".to_string(),
            timestamp_epoch: 1792684800,
            completed: false,
            is_timer: false,
        }];

        let new_text = "випити води";
        let new_epoch = 1792684830; // +30с різниці
        let found = reminders.iter_mut().find(|r| {
            !r.completed
                && r.text.to_lowercase() == new_text
                && (r.timestamp_epoch - new_epoch).abs() <= 60
        });
        assert!(found.is_some());
    }
}

