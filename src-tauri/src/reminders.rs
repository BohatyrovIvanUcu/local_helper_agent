// Модуль планування таймерів та нагадувань з фоновим циклом перевірки

use chrono::{DateTime, Duration as ChronoDuration, Local, NaiveDateTime, NaiveTime, TimeZone};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use crate::config::{get_home_dir, get_user_config_path};
use crate::media::check_is_music_playing;

pub static REMINDERS_LOCK: Mutex<()> = Mutex::new(());

#[derive(Serialize, Deserialize, Clone, Debug)]
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

pub fn read_reminders_sync() -> Vec<ReminderItem> {
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

pub fn write_reminders_sync(items: &[ReminderItem]) -> Result<(), String> {
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

pub fn trigger_system_alert(title: &str, message: &str) {
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

pub fn parse_target_timestamp(target_time_str: &str) -> Result<i64, String> {
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
pub async fn manage_reminders(
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

/// Запускає фоновий планувальник нагадувань і таймерів у окремому системному потоці
pub fn start_reminders_scheduler(app_handle: AppHandle) {
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
}

#[cfg(test)]
mod tests {
    use super::*;

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

