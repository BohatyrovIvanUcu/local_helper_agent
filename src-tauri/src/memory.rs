// Модуль довгострокової пам'яті асистента та збереження історії розмов

use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use crate::config::{get_home_dir, get_user_config_path};

pub static MEMORY_LOCK: Mutex<()> = Mutex::new(());

const MAX_MEMORY_FACTS: usize = 25;
const MAX_FACT_CHARS: usize = 220;
const MAX_RECENT_MESSAGES: usize = 10;
const MAX_MESSAGE_CHARS: usize = 450;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct MemoryMessage {
    pub role: String,
    pub text: String,
    pub timestamp: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools_used: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AgentMemory {
    #[serde(default)]
    pub facts: Vec<String>,
    #[serde(default)]
    pub recent_messages: Vec<MemoryMessage>,
}

pub fn truncate_memory_text(input: &str, max_chars: usize) -> String {
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

pub fn append_agent_log_sync(
    user_text: &str,
    model_text: &str,
    tools_used: &[String],
) -> Result<(), String> {
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
        "tools_used": tools_used,
    });
    lines.push(entry.to_string());
    if lines.len() > 20 {
        let excess = lines.len() - 20;
        lines.drain(0..excess);
    }
    let new_content = lines.join("\n") + "\n";
    let _ = fs::write(&log_path, new_content);
    Ok(())
}

pub fn read_agent_memory_sync() -> AgentMemory {
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

pub fn write_agent_memory_sync(mem: &AgentMemory) -> Result<(), String> {
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
pub async fn get_agent_memory() -> Result<AgentMemory, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let _guard = MEMORY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        read_agent_memory_sync()
    })
    .await
    .map_err(|e| format!("Помилка читання пам'яті: {}", e))
}

#[tauri::command]
pub async fn append_conversation_turn(
    user_text: Option<String>,
    model_text: Option<String>,
    tools_used: Option<Vec<String>>,
    #[allow(non_snake_case)]
    userText: Option<String>,
    #[allow(non_snake_case)]
    modelText: Option<String>,
    #[allow(non_snake_case)]
    toolsUsed: Option<Vec<String>>,
) -> Result<AgentMemory, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let clean_user_arg = user_text.or(userText).unwrap_or_default();
        let clean_model_arg = model_text.or(modelText).unwrap_or_default();
        let tools = tools_used.or(toolsUsed).unwrap_or_default();

        let _guard = MEMORY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut mem = read_agent_memory_sync();
        let now = Local::now().format("%Y-%m-%d %H:%M").to_string();

        let clean_user = truncate_memory_text(&clean_user_arg, MAX_MESSAGE_CHARS);
        let clean_model = truncate_memory_text(&clean_model_arg, MAX_MESSAGE_CHARS);

        let _ = append_agent_log_sync(&clean_user, &clean_model, &tools);

        if !clean_user.is_empty() {
            mem.recent_messages.push(MemoryMessage {
                role: "user".to_string(),
                text: clean_user,
                timestamp: now.clone(),
                tools_used: Vec::new(),
            });
        }
        if !clean_model.is_empty() {
            mem.recent_messages.push(MemoryMessage {
                role: "model".to_string(),
                text: clean_model,
                timestamp: now,
                tools_used: tools,
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
pub async fn manage_memory(action: String, fact: Option<String>) -> Result<String, String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_message_tools_used_serialization() {
        let msg_with_tools = MemoryMessage {
            role: "assistant".to_string(),
            text: "Зробив яскравість 50%".to_string(),
            timestamp: "2026-10-24 02:00:00".to_string(),
            tools_used: vec!["control_display (set_brightness: 50)".to_string()],
        };
        let json = serde_json::to_string(&msg_with_tools).expect("serialization failed");
        assert!(json.contains("tools_used"));
        assert!(json.contains("control_display"));

        let deserialized: MemoryMessage = serde_json::from_str(&json).expect("deserialization failed");
        assert_eq!(deserialized.tools_used.len(), 1);
        assert_eq!(deserialized.tools_used[0], "control_display (set_brightness: 50)");

        // Перевірка зворотньої сумісності коли tools_used відсутнє
        let old_json = r#"{"role":"user","text":"Привіт","timestamp":"2026-10-24 01:00:00"}"#;
        let old_msg: MemoryMessage = serde_json::from_str(old_json).expect("backwards compatible");
        assert!(old_msg.tools_used.is_empty());
    }
}

