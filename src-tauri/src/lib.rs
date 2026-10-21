use chrono::Local;
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;
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
    let vault = get_env_or_dotenv("OBSIDIAN_VAULT_PATH").unwrap_or_default();
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
async fn append_to_obsidian(
    subfolder: Option<String>,
    filename: String,
    content: String,
    as_todo: bool,
    section: Option<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let vault_raw = get_env_or_dotenv("OBSIDIAN_VAULT_PATH")
            .unwrap_or_else(|| "~/Obsidian/Ivan".to_string());
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
            fs::create_dir_all(parent).map_err(|e| {
                format!("Не вдалося створити папку '{}': {}", parent.display(), e)
            })?;
        }

        let is_ideas_section = section
            .as_deref()
            .map(|s| {
                let upper = s.trim().to_uppercase();
                upper == "IDEAS" || upper == "IDEA" || upper == "ІДЕЇ" || upper == "ІДЕЯ"
            })
            .unwrap_or(false);

        let mut clean_content = content.trim();
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
            let existing = fs::read_to_string(&target_path).map_err(|e| {
                format!("Помилка читання нотатки '{}': {}", target_path.display(), e)
            })?;
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

/// 2. Створює або повністю перезаписує файл за вказаним шляхом (підтримує `~`).
#[tauri::command(rename_all = "snake_case")]
async fn create_or_update_file(filepath: String, content: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
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
    })
    .await
    .map_err(|e| format!("Помилка потоку: {}", e))?
}

/// 3. Читає вміст файлу або список елементів директорії (підтримує `~`).
#[tauri::command(rename_all = "snake_case")]
async fn read_file_or_dir(target_path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = expand_tilde(&target_path)?;

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
    let lower = cmd_str.trim().to_lowercase();
    if lower.ends_with('&') {
        return true;
    }
    const GUI_KEYWORDS: &[&str] = &[
        "xdg-open",
        "gio open",
        "sensible-browser",
        "x-www-browser",
        "gnome-open",
        "kde-open",
        "nohup ",
        "setsid ",
        "firefox",
        "google-chrome",
        "chromium",
        "brave",
        "vivaldi",
        "opera",
        "nautilus",
        "dolphin",
        "thunar",
        "code ",
        "obsidian",
        "telegram",
        "spotify",
        "discord",
        "slack",
        "vlc",
        "mpv",
    ];
    GUI_KEYWORDS.iter().any(|kw| {
        lower.starts_with(kw)
            || lower.contains(&format!(" {}", kw))
            || lower.contains(&format!(";{}", kw))
            || lower.contains(&format!("&{}", kw))
    })
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
            let status_code = status.code().unwrap_or(0);
            let combined = format!(
                "Exit code: {}\n--- STDOUT ---\n{}\n--- STDERR ---\n{}",
                status_code,
                stdout.trim(),
                stderr.trim()
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

fn resume_media_player() -> bool {
    #[cfg(target_os = "linux")]
    {
        let py_code = r#"
import sys, time, dbus
try:
    bus = dbus.SessionBus()
    players = [s for s in bus.list_names() if s.startswith('org.mpris.MediaPlayer2.')]
    if not players:
        sys.exit(1)
    resumed = False
    for p in players:
        try:
            player_obj = bus.get_object(p, '/org/mpris/MediaPlayer2')
            iface = dbus.Interface(player_obj, 'org.mpris.MediaPlayer2.Player')
            props = dbus.Interface(player_obj, 'org.freedesktop.DBus.Properties')
            try:
                status = str(props.Get('org.mpris.MediaPlayer2.Player', 'PlaybackStatus'))
                if status == 'Playing':
                    resumed = True
                    continue
            except Exception:
                pass
            try:
                iface.Play()
            except Exception:
                try:
                    iface.PlayPause()
                except Exception:
                    pass
            time.sleep(0.25)
            try:
                status_after = str(props.Get('org.mpris.MediaPlayer2.Player', 'PlaybackStatus'))
                if status_after == 'Playing':
                    resumed = True
            except Exception:
                pass
        except Exception:
            pass
    sys.exit(0 if resumed else 1)
except Exception:
    sys.exit(1)
"#;
        if let Ok(status) = Command::new("python3").args(["-c", py_code]).status() {
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
        if let Ok(status) = Command::new("playerctl").arg("pause").status() {
            if status.success() {
                return true;
            }
        }

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
            player_obj = bus.get_object(p, '/org/mpris/MediaPlayer2')
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
        if let Ok(status) = Command::new("python3").args(["-c", py_code]).status() {
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
        if let Ok(status) = Command::new("playerctl").arg("next").status() {
            if status.success() {
                return true;
            }
        }

        let py_code = r#"
import sys, dbus
try:
    bus = dbus.SessionBus()
    players = [s for s in bus.list_names() if s.startswith('org.mpris.MediaPlayer2.')]
    if not players:
        sys.exit(1)
    for p in players:
        try:
            bus.get_object(p, '/org/mpris/MediaPlayer2').Next(dbus_interface='org.mpris.MediaPlayer2.Player')
        except Exception:
            pass
    sys.exit(0)
except Exception:
    sys.exit(1)
"#;
        if let Ok(status) = Command::new("python3").args(["-c", py_code]).status() {
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
        if let Ok(status) = Command::new("playerctl").arg("previous").status() {
            if status.success() {
                return true;
            }
        }

        let py_code = r#"
import sys, dbus
try:
    bus = dbus.SessionBus()
    players = [s for s in bus.list_names() if s.startswith('org.mpris.MediaPlayer2.')]
    if not players:
        sys.exit(1)
    for p in players:
        try:
            bus.get_object(p, '/org/mpris/MediaPlayer2').Previous(dbus_interface='org.mpris.MediaPlayer2.Player')
        except Exception:
            pass
    sys.exit(0)
except Exception:
    sys.exit(1)
"#;
        if let Ok(status) = Command::new("python3").args(["-c", py_code]).status() {
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

fn open_and_play_query(query: &str) -> Result<String, String> {
    let py_search = r#"
import sys, urllib.request, urllib.parse, re, subprocess

raw = sys.argv[1].strip() if len(sys.argv) > 1 else ""
play_url = None

# 1. Пряме посилання на плейлист або трек (наприклад, https://music.youtube.com/playlist?list=LM)
if raw.startswith("http://") or raw.startswith("https://"):
    url = raw
    if "playlist?list=" in url:
        url = url.replace("playlist?list=", "watch?list=")
    if "watch?list=" in url and "&index=" not in url and "&v=" not in url:
        url += "&index=1"
    play_url = url
elif raw.lower() in ("liked", "liked_music", "лайки", "улюблені", "улюблена музика", "мій плейлист", "моя музика", "мої лайки", "музика яка сподобалась"):
    play_url = "https://music.youtube.com/watch?list=LM&index=1"
elif raw.lower() in ("my_mix", "mix", "рекомендації", "мікс", "мої рекомендації", "супермікс"):
    play_url = "https://music.youtube.com/watch?list=RDMM"
elif raw:
    try:
        q = urllib.parse.quote_plus(raw)
        url = f"https://www.youtube.com/results?search_query={q}"
        req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"})
        html = urllib.request.urlopen(req, timeout=4).read().decode('utf-8', errors='ignore')
        matches = re.findall(r"watch\?v=([a-zA-Z0-9_-]{11})", html)
        if matches:
            vid = matches[0]
            play_url = f"https://music.youtube.com/watch?v={vid}"
    except Exception:
        pass
    if not play_url:
        play_url = f"https://music.youtube.com/search?q={urllib.parse.quote_plus(raw)}"

# За замовчуванням відкриваємо персональний плейлист вподобань користувача (Liked Music) з першого треку!
if not play_url:
    play_url = "https://music.youtube.com/watch?list=LM&index=1"

if sys.platform == "win32":
    subprocess.Popen(["cmd", "/C", "start", play_url])
elif sys.platform == "darwin":
    subprocess.Popen(["open", play_url])
else:
    subprocess.Popen(["xdg-open", play_url])

print("OK:" + play_url)
"#;

    let res = Command::new("python3")
        .args(["-c", py_search, query])
        .output();

    // Запускаємо фонову перевірку: чекаємо повного завантаження вкладки та редіректу YouTube Music у Firefox/Chrome,
    // перевіряємо MPRIS або надсилаємо натискання Space через /dev/uinput для обходу "Почати відтворення"
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

    if let Ok(out) = res {
        let stdout = String::from_utf8_lossy(&out.stdout);
        if stdout.contains("OK:") {
            if query.is_empty() || query == "liked_music" || query.to_lowercase().contains("лайк") {
                return Ok("Вмикаю твій плейлист 'Музика, яка сподобалась' у YouTube Music 🎵".to_string());
            } else if query == "my_mix" || query.to_lowercase().contains("рекомендац") {
                return Ok("Вмикаю твій персональний мікс рекомендацій у YouTube Music 🎵".to_string());
            } else {
                return Ok(format!("Увімкнено відтворення музики ('{}') 🎵", query));
            }
        }
    }

    let direct_url = if query.is_empty() || query == "liked_music" {
        "https://music.youtube.com/watch?list=LM&index=1".to_string()
    } else {
        format!("https://music.youtube.com/search?q={}", query)
    };

    #[cfg(target_os = "windows")]
    let _ = Command::new("cmd").args(["/C", "start", &direct_url]).spawn();
    #[cfg(target_os = "macos")]
    let _ = Command::new("open").arg(&direct_url).spawn();
    #[cfg(target_os = "linux")]
    let _ = Command::new("xdg-open").arg(&direct_url).spawn();

    Ok("Відкриваю YouTube Music 🎵".to_string())
}

/// Керує відтворенням медіа: натискає Play/Pause на активному плеєрі або запускає прямий трек.
#[tauri::command]
async fn control_media(action: String, query: Option<String>) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let action_clean = action.trim().to_lowercase();
        let query_clean = query.as_deref().map(str::trim).filter(|s| !s.is_empty());

        if let Some(q) = query_clean {
            // Якщо запитано відтворення улюблених треків, спочатку пробуємо відновити вже відкритий плеєр на паузі
            if (action_clean == "play" || action_clean == "open_and_play" || action_clean == "play_query")
                && (q == "liked_music" || q == "liked")
            {
                if resume_media_player() {
                    return Ok("Продовжую відтворення музики ▶️".to_string());
                }
            }

            if action_clean == "play_query" || action_clean == "open_and_play" || (action_clean == "play" && !q.is_empty()) {
                return open_and_play_query(q);
            }
        }

        if action_clean == "pause" {
            let handled = pause_media_player();
            if handled {
                return Ok("Музику поставлено на паузу ⏸️".to_string());
            } else {
                return Ok("Команда паузи надіслана ⏸️".to_string());
            }
        }

        if action_clean == "next" {
            let handled = next_media_player();
            if handled {
                return Ok("Увімкнено наступний трек ⏭️".to_string());
            } else {
                return Ok("Команда наступного треку надіслана ⏭️".to_string());
            }
        }

        if action_clean == "prev" || action_clean == "previous" {
            let handled = prev_media_player();
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
            return Ok("Продовжую відтворення музики ▶️".to_string());
        }

        // 2. Якщо плеєр не був активний — вмикаємо особистий плейлист уподобань користувача (Liked Music)
        open_and_play_query("liked_music")
    })
    .await
    .map_err(|e| format!("Помилка обробки медіа: {}", e))?
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
    tauri::async_runtime::spawn_blocking(read_agent_memory_sync)
        .await
        .map_err(|e| format!("Помилка читання пам'яті: {}", e))
}

#[tauri::command]
async fn append_conversation_turn(user_text: String, model_text: String) -> Result<AgentMemory, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut mem = read_agent_memory_sync();
        let now = Local::now().format("%Y-%m-%d %H:%M").to_string();

        let clean_user = truncate_memory_text(&user_text, MAX_MESSAGE_CHARS);
        let clean_model = truncate_memory_text(&model_text, MAX_MESSAGE_CHARS);

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
            manage_memory
        ])
        .run(tauri::generate_context!())
        .expect("Помилка запуску Tauri-застосунку");
}

