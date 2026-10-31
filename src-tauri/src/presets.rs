// Модуль збереження та відновлення робочих сесій («Пресети вкладок»)

use chrono::Local;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;
use crate::config::{get_home_dir, get_user_config_path};
use crate::media::open_browser_detached;

pub static PRESETS_LOCK: Mutex<()> = Mutex::new(());

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SessionPreset {
    pub name: String,
    pub title: String,
    pub urls: Vec<String>,
    #[serde(default)]
    pub apps: Vec<String>,
    #[serde(default)]
    pub browser: Option<String>,
    #[serde(default)]
    pub created_at: String,
}

/// Повертає шлях до файлу пресетів (~/.config/local-helper-ai/presets.json).
pub fn get_presets_file_path() -> Result<PathBuf, String> {
    let config_path = get_user_config_path()?;
    if let Some(parent) = config_path.parent() {
        return Ok(parent.join("presets.json"));
    }
    let home = get_home_dir()?;
    Ok(home.join(".config").join("local-helper-ai").join("presets.json"))
}

/// Створює стандартний набір пресетів за замовчуванням (зокрема «Навчання»: UCU LMS, Teams, VS Code).
pub fn get_default_presets() -> HashMap<String, SessionPreset> {
    let mut map = HashMap::new();
    let now = Local::now().to_rfc3339();

    map.insert(
        "study".to_string(),
        SessionPreset {
            name: "study".to_string(),
            title: "Навчання".to_string(),
            urls: vec![
                "https://learn.ucu.edu.ua".to_string(),
            ],
            apps: vec!["teams".to_string(), "code".to_string()],
            browser: Some("firefox".to_string()),
            created_at: now.clone(),
        },
    );

    map
}

/// Нормалізує псевдоніми та назви пресетів (наприклад «навчання» -> «study»).
pub fn normalize_preset_name(raw: &str) -> String {
    let clean = raw.trim().to_lowercase();
    match clean.as_str() {
        "навчання" | "study" | "універ" | "уроки" | "урок" | "пари" | "пара" => "study".to_string(),
        "робота" | "work" | "проект" | "проєкт" | "розробка" | "дев" | "dev" => "work".to_string(),
        other => other.to_string(),
    }
}

/// Зчитує збережені пресети із файлу, або ініціалізує дефолтними, якщо файл відсутній.
pub fn load_presets_sync() -> HashMap<String, SessionPreset> {
    let _guard = PRESETS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let path = match get_presets_file_path() {
        Ok(p) => p,
        Err(_) => return get_default_presets(),
    };

    if path.exists() {
        if let Ok(raw) = fs::read_to_string(&path) {
            if let Ok(map) = serde_json::from_str::<HashMap<String, SessionPreset>>(&raw) {
                if !map.is_empty() {
                    return map;
                }
            }
        }
    }

    let defaults = get_default_presets();
    let _ = save_presets_sync_locked(&defaults);
    defaults
}

fn save_presets_sync_locked(map: &HashMap<String, SessionPreset>) -> Result<(), String> {
    let path = get_presets_file_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            format!(
                "Не вдалося створити директорію для пресетів {}: {}",
                parent.display(),
                e
            )
        })?;
    }
    let data = serde_json::to_string_pretty(map)
        .map_err(|e| format!("Помилка серіалізації пресетів: {}", e))?;
    fs::write(&path, data)
        .map_err(|e| format!("Не вдалося записати файл пресетів {}: {}", path.display(), e))
}

pub fn save_presets_sync(map: &HashMap<String, SessionPreset>) -> Result<(), String> {
    let _guard = PRESETS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    save_presets_sync_locked(map)
}

/// Вилучає поточні відкриті посилання з активних сесій браузера (Chrome / Chromium / Firefox).
pub fn extract_current_open_urls() -> Vec<String> {
    let home = match get_home_dir() {
        Ok(h) => h.to_string_lossy().to_string(),
        Err(_) => return Vec::new(),
    };

    let py_code = r#"
import os, glob, re, json, sys, struct

home = sys.argv[1]
urls = []

def is_valid_url(u):
    if not u or not isinstance(u, str):
        return False
    u = u.strip()
    if not (u.startswith('http://') or u.startswith('https://')):
        return False
    lower = u.lower()
    ignore = (
        'google.com/search', 'favicon', 'fonts.gstatic', 'chrome-extension://',
        'chrome://', 'moz-extension://', 'about:', 'accounts.google.com',
        'newtab', 'edge://', 'brave://', 'devtools://'
    )
    return not any(x in lower for x in ignore)

def decompress_mozlz4(data):
    if data[:8] != b'mozLz40\0':
        return None
    try:
        import lz4.block
        return lz4.block.decompress(data[8:])
    except Exception:
        pass
    try:
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
    except Exception:
        return None

# 1. Chrome / Chromium
chrome_patterns = [
    home + '/.config/google-chrome/*/Sessions/Tabs_*',
    home + '/.config/google-chrome/*/Sessions/Session_*',
    home + '/.config/chromium/*/Sessions/Tabs_*',
    home + '/.config/chromium/*/Sessions/Session_*',
    home + '/.config/BraveSoftware/Brave-Browser/*/Sessions/Tabs_*',
]
for pat in chrome_patterns:
    files = glob.glob(pat)
    files.sort(key=os.path.getmtime, reverse=True)
    for latest in files[:3]:
        try:
            with open(latest, 'rb') as f:
                content = f.read()
            found = re.findall(rb'https?://[a-zA-Z0-9\.\-_~:/?#\[\]@!$&\'()*+,;=%]+', content)
            for u in found:
                try:
                    u_str = u.decode('utf-8', errors='ignore')
                    if is_valid_url(u_str) and u_str not in urls:
                        urls.append(u_str)
                except Exception:
                    pass
        except Exception:
            pass

# 2. Firefox
ff_patterns = [
    home + '/.mozilla/firefox/**/*.jsonlz4',
    home + '/snap/firefox/**/recovery.jsonlz4',
    home + '/.zen/**/*.jsonlz4',
]
for pat in ff_patterns:
    for fp in glob.glob(pat, recursive=True):
        if not any(k in fp for k in ('recovery.jsonlz4', 'previous.jsonlz4')):
            continue
        try:
            with open(fp, 'rb') as f_obj:
                raw = f_obj.read()
            decomp = decompress_mozlz4(raw)
            if decomp:
                data = json.loads(decomp.decode('utf-8', errors='ignore'))
                for w in data.get('windows', []):
                    for t in w.get('tabs', []):
                        entries = t.get('entries', [])
                        if not entries:
                            continue
                        cur_idx = t.get('index', len(entries)) - 1
                        cur_idx = max(0, min(cur_idx, len(entries) - 1))
                        u = entries[cur_idx].get('url', '')
                        if is_valid_url(u) and u not in urls:
                            urls.append(u)
        except Exception:
            pass

print(json.dumps(urls))
"#;

    let output = Command::new("python3")
        .args(["-c", py_code, &home])
        .stdin(Stdio::null())
        .output();

    if let Ok(out) = output {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if let Ok(parsed) = serde_json::from_str::<Vec<String>>(&s) {
                return parsed;
            }
        }
    }

    Vec::new()
}

pub fn format_app_name(app: &str) -> String {
    match app.trim().to_lowercase().as_str() {
        "teams" | "teams-for-linux" | "ms-teams" => "MS Teams".to_string(),
        "code" | "vscode" => "VS Code".to_string(),
        other => other.to_string(),
    }
}

pub fn launch_app(app_name: &str) {
    let clean = app_name.trim().to_lowercase();
    match clean.as_str() {
        "code" | "vscode" => {
            let _ = Command::new("code")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
        }
        "teams" | "ms-teams" | "teams-for-linux" => {
            let candidates = [
                "/snap/bin/teams-for-linux",
                "teams-for-linux",
                "teams",
                "/usr/bin/teams-for-linux",
                "/usr/bin/teams",
            ];
            let mut launched = false;
            for bin in candidates {
                if Command::new(bin)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .is_ok()
                {
                    launched = true;
                    break;
                }
            }
            if !launched {
                open_browser_detached("https://teams.microsoft.com");
            }
        }
        other => {
            let _ = Command::new(other)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
        }
    }
}

/// Визначає системний браузер за замовчуванням (Firefox, Chrome тощо) через xdg-settings / xdg-mime.
pub fn detect_default_browser() -> String {
    #[cfg(target_os = "linux")]
    {
        if let Ok(out) = Command::new("xdg-settings")
            .args(["get", "default-web-browser"])
            .output()
        {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout).to_lowercase();
                if s.contains("firefox") || s.contains("zen") || s.contains("librewolf") {
                    return "firefox".to_string();
                }
                if s.contains("chrome") || s.contains("chromium") || s.contains("brave") {
                    return "chrome".to_string();
                }
            }
        }
        if let Ok(out) = Command::new("xdg-mime")
            .args(["query", "default", "x-scheme-handler/https"])
            .output()
        {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout).to_lowercase();
                if s.contains("firefox") || s.contains("zen") || s.contains("librewolf") {
                    return "firefox".to_string();
                }
                if s.contains("chrome") || s.contains("chromium") || s.contains("brave") {
                    return "chrome".to_string();
                }
            }
        }
    }
    "firefox".to_string()
}

/// Відкриває пресет у системі (браузер + застосунки на кшталт VS Code, Teams).
pub fn launch_preset(preset: &SessionPreset) -> Result<String, String> {
    if preset.urls.is_empty() && preset.apps.is_empty() {
        return Err(format!("У пресеті '{}' немає посилань або застосунків для відкриття", preset.title));
    }

    let target_browser = preset
        .browser
        .as_deref()
        .map(|b| b.trim().to_lowercase())
        .unwrap_or_else(detect_default_browser);

    // 1. Запуск посилань у браузері
    if !preset.urls.is_empty() {
        #[cfg(target_os = "linux")]
        {
            let mut launched_in_window = false;

            if target_browser.contains("firefox") || target_browser.contains("zen") || target_browser.contains("librewolf") {
                let candidates = ["firefox", "/snap/bin/firefox", "/usr/bin/firefox"];
                for bin in candidates {
                    let mut cmd = Command::new(bin);
                    for u in &preset.urls {
                        cmd.arg("--new-tab").arg(u);
                    }
                    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
                    if cmd.spawn().is_ok() {
                        launched_in_window = true;
                        break;
                    }
                }
            } else if target_browser.contains("chrome") || target_browser.contains("chromium") || target_browser.contains("brave") {
                let candidates = ["google-chrome", "google-chrome-stable", "chromium", "/usr/bin/google-chrome"];
                for bin in candidates {
                    let mut cmd = Command::new(bin);
                    for u in &preset.urls {
                        cmd.arg(u);
                    }
                    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
                    if cmd.spawn().is_ok() {
                        launched_in_window = true;
                        break;
                    }
                }
            }

            if !launched_in_window {
                for u in &preset.urls {
                    open_browser_detached(u);
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            for u in &preset.urls {
                open_browser_detached(u);
                std::thread::sleep(Duration::from_millis(120));
            }
        }
    }

    // 2. Запуск супутніх застосунків (VS Code, Teams тощо)
    for app in &preset.apps {
        launch_app(app);
    }

    let urls_summary = if preset.urls.len() > 2 {
        format!("{} вкладок", preset.urls.len())
    } else {
        preset.urls.join(", ")
    };

    let apps_summary = if !preset.apps.is_empty() {
        let names: Vec<String> = preset.apps.iter().map(|a| format_app_name(a)).collect();
        format!(" та {}", names.join(", "))
    } else {
        String::new()
    };

    Ok(format!(
        "Стек «{}» успішно відкрито ({}). Гарної продуктивності! 🚀",
        preset.title,
        if apps_summary.is_empty() { urls_summary } else { format!("{}{}", urls_summary, apps_summary) }
    ))
}

/// Головний обробник команд керування сесіями
#[tauri::command]
pub async fn manage_session_presets(
    action: String,
    name: Option<String>,
    urls: Option<Vec<String>>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let clean_action = action.trim().to_lowercase();
        let target_name = name.as_deref().map(normalize_preset_name).unwrap_or_else(|| "study".to_string());

        let mut presets = load_presets_sync();

        match clean_action.as_str() {
            "open" | "launch" | "start" => {
                let preset = presets.get(&target_name).ok_or_else(|| {
                    let available: Vec<&str> = presets.values().map(|p| p.title.as_str()).collect();
                    format!(
                        "Пресет '{}' не знайдено. Доступні пресети: {}",
                        target_name,
                        if available.is_empty() { "немає".to_string() } else { available.join(", ") }
                    )
                })?;

                launch_preset(preset)
            }

            "save" | "create" | "remember" => {
                let resolved_urls = match urls {
                    Some(custom_urls) if !custom_urls.is_empty() => custom_urls,
                    _ => {
                        let extracted = extract_current_open_urls();
                        if extracted.is_empty() {
                            return Err(
                                "Не вдалося автоматично виявити відкриті вкладки браузера. Спробуйте вказати посилання явно."
                                    .to_string(),
                            );
                        }
                        extracted
                    }
                };

                let display_title = match target_name.as_str() {
                    "study" => "Навчання",
                    "work" => "Робота",
                    other => other,
                };

                let new_preset = SessionPreset {
                    name: target_name.clone(),
                    title: display_title.to_string(),
                    urls: resolved_urls.clone(),
                    apps: if target_name == "study" || target_name == "work" {
                        vec!["code".to_string()]
                    } else {
                        Vec::new()
                    },
                    browser: Some(detect_default_browser()),
                    created_at: Local::now().to_rfc3339(),
                };

                presets.insert(target_name.clone(), new_preset);
                save_presets_sync(&presets)?;

                Ok(format!(
                    "Пресет «{}» успішно збережено! У сесії збережено {} вкладок. 📑",
                    display_title,
                    resolved_urls.len()
                ))
            }

            "close" | "stop" => {
                let preset = presets.get(&target_name).ok_or_else(|| {
                    format!("Пресет '{}' не знайдено серед збережених сесій.", target_name)
                })?;

                let items_desc = if !preset.apps.is_empty() {
                    let names: Vec<String> = preset.apps.iter().map(|a| format_app_name(a)).collect();
                    format!("{} вкладок ({}) та застосунки: {}", preset.urls.len(), preset.urls.join(", "), names.join(", "))
                } else {
                    format!("{} вкладок ({})", preset.urls.len(), preset.urls.join(", "))
                };

                Ok(format!(
                    "У робочому стеку «{}» задіяні: {}. Якщо хочете закрити їх, підтвердіть закриття браузера або збережіть потрібне перед виходом! ⚠️",
                    preset.title,
                    items_desc
                ))
            }

            "list" | "show" => {
                if presets.is_empty() {
                    return Ok("У вас ще немає збережених робочих стеків.".to_string());
                }

                let mut list = Vec::new();
                for p in presets.values() {
                    let desc = if !p.apps.is_empty() {
                        let names: Vec<String> = p.apps.iter().map(|a| format_app_name(a)).collect();
                        format!("• **{}** ({}) — {} посилань + {}", p.title, p.name, p.urls.len(), names.join(", "))
                    } else {
                        format!("• **{}** ({}) — {} посилань", p.title, p.name, p.urls.len())
                    };
                    list.push(desc);
                }

                Ok(format!(
                    "Збережені робочі стеки:\n{}\n\nСкажіть «відкрий [назва]» для запуску будь-якого стеку.",
                    list.join("\n")
                ))
            }

            "delete" | "remove" => {
                if presets.remove(&target_name).is_some() {
                    save_presets_sync(&presets)?;
                    Ok(format!("Пресет '{}' видалено.", target_name))
                } else {
                    Err(format!("Пресет '{}' не знайдено для видалення.", target_name))
                }
            }

            unknown => Err(format!(
                "Невідома дія '{}'. Доступні дії: open, save, close, list, delete",
                unknown
            )),
        }
    })
    .await
    .map_err(|e| format!("Помилка виконання таски сесій: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_preset_name() {
        assert_eq!(normalize_preset_name("навчання"), "study");
        assert_eq!(normalize_preset_name("Універ "), "study");
        assert_eq!(normalize_preset_name("РОБОТА"), "work");
        assert_eq!(normalize_preset_name("dev"), "work");
        assert_eq!(normalize_preset_name("дизайн"), "дизайн");
    }

    #[test]
    fn test_default_presets_contain_study() {
        let defaults = get_default_presets();
        assert!(defaults.contains_key("study"));
        let study = defaults.get("study").unwrap();
        assert_eq!(study.title, "Навчання");
        assert!(study.urls.iter().any(|u| u.contains("learn.ucu.edu.ua")));
        assert!(study.apps.contains(&"teams".to_string()));
        assert!(study.apps.contains(&"code".to_string()));
        assert_eq!(study.browser.as_deref(), Some("firefox"));
    }

    #[test]
    fn test_detect_default_browser() {
        let browser = detect_default_browser();
        assert!(!browser.is_empty());
    }
}
