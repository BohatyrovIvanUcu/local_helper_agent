// Модуль конфігурації: керування ~/.config/local-helper-ai/config.env та змінними середовища

use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Повертає домашню директорію користувача (`HOME` або `USERPROFILE`).
pub fn get_home_dir() -> Result<PathBuf, String> {
    env::var("HOME")
        .or_else(|_| env::var("USERPROFILE"))
        .map(PathBuf::from)
        .map_err(|_| "Не вдалося визначити домашню директорію користувача (HOME/USERPROFILE)".to_string())
}

/// Розгортає префікс `~` або `~/` у повний шлях до домашньої директорії.
pub fn expand_tilde(raw_path: &str) -> Result<PathBuf, String> {
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
pub fn sanitize_relative_path(rel: &str) -> Result<PathBuf, String> {
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

/// Читає значення змінної з оточення або локального конфіг-файлу.
pub fn get_env_or_dotenv(key: &str) -> Option<String> {
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
pub async fn get_app_config() -> Result<AppConfig, String> {
    tauri::async_runtime::spawn_blocking(read_app_config)
        .await
        .map_err(|e| format!("Помилка читання конфігурації: {}", e))
}
