// Модуль взаємодії з нотатками Obsidian Vault та Kanban-дошками

use chrono::Local;
use std::fs;
use crate::config::{expand_tilde, get_env_or_dotenv, sanitize_relative_path};

/// Розумно вставляє новий запис або TODO у Markdown/Kanban структуру під вказану секцію (наприклад, ## Ideas або ## TO DO).
pub fn insert_into_obsidian_markdown(
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
pub async fn append_to_obsidian(
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

#[cfg(test)]
mod tests {
    use super::*;

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
}

