// Модуль взаємодії з нотатками Obsidian Vault та Kanban-дошками

use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use crate::config::{expand_tilde, get_env_or_dotenv, sanitize_relative_path};
use crate::files::{truncate_chars, MAX_READ_CHARS};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObsidianSearchResult {
    pub filename: String,
    pub relative_path: String,
    pub matches: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObsidianEntry {
    pub name: String,
    pub relative_path: String,
    pub is_dir: bool,
    pub size_bytes: u64,
    pub modified_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyNoteResult {
    pub filename: String,
    pub relative_path: String,
    pub content: String,
    pub created_new: bool,
}

/// Повертає базовий шлях до Obsidian Vault з конфігурації або дефолтний `~/Obsidian/Ivan`.
pub fn get_vault_root() -> Result<PathBuf, String> {
    let vault_raw = get_env_or_dotenv("OBSIDIAN_VAULT_PATH")
        .unwrap_or_else(|| "~/Obsidian/Ivan".to_string());
    expand_tilde(&vault_raw)
}

/// Перевіряє, чи є назва файлу/папки системною для Obsidian/Git/ОС, яку слід ігнорувати.
pub fn is_ignored_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == ".obsidian"
        || lower == ".trash"
        || lower == "trash"
        || lower == ".git"
        || lower == ".ds_store"
        || lower.starts_with('.')
}

/// Безпечно формує шлях до нотатки у Vault, гарантуючи розширення `.md` та захист від виходу за межі сховища.
pub fn resolve_vault_path(subfolder: Option<&str>, filename: &str) -> Result<PathBuf, String> {
    let vault_base = get_vault_root()?;
    let mut target = vault_base.clone();

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
            target = target.join(safe_sub);
        }
    }

    let safe_file = sanitize_relative_path(filename)?;
    if safe_file.as_os_str().is_empty() {
        return Err("Ім'я файлу нотатки не може бути порожнім".to_string());
    }

    target = target.join(safe_file);
    if target.extension().and_then(|e| e.to_str()) != Some("md") {
        target.set_extension("md");
    }

    // Захист від Path Traversal
    let target_str = target.to_string_lossy().replace('\\', "/");
    let vault_str = vault_base.to_string_lossy().replace('\\', "/");
    if !target_str.starts_with(&vault_str) {
        return Err(format!(
            "Доступ заборонено: спроба вийти за межі Obsidian Vault ({})",
            target.display()
        ));
    }

    Ok(target)
}

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

/// Знаходить таск `- [ ] ...`, що містить `task_query`, позначає його `- [x]` та переміщує
/// у секцію `## Completed` / `## DONE` (якщо така є у файлі). Якщо такої секції немає, позначає на місці.
pub fn mark_todo_completed_in_markdown(
    original: &str,
    task_query: &str,
) -> Result<(String, String), String> {
    let clean_query = task_query.trim().to_lowercase();
    if clean_query.is_empty() {
        return Err("Пошуковий запит для задачі не може бути порожнім".to_string());
    }

    let lines: Vec<&str> = original.lines().collect();
    let mut target_idx = None;
    let mut found_line = String::new();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("- [ ]") && trimmed.to_lowercase().contains(&clean_query) {
            target_idx = Some(i);
            found_line = trimmed.to_string();
            break;
        }
    }

    let target_idx = match target_idx {
        Some(idx) => idx,
        None => {
            return Err(format!(
                "Не знайдено активної невиконаної задачі (- [ ]), яка містить '{}'",
                task_query.trim()
            ));
        }
    };

    let completed_task = found_line.replacen("- [ ]", "- [x]", 1);

    // Шукаємо секцію Completed / Done / Завершено / Виконано
    let mut completed_heading_idx = None;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            let clean_title = trimmed.trim_start_matches('#').trim().to_uppercase();
            if clean_title == "COMPLETED"
                || clean_title == "DONE"
                || clean_title == "ЗАВЕРШЕНО"
                || clean_title == "ВИКОНАНО"
                || clean_title == "АРХІВ"
            {
                completed_heading_idx = Some(i);
                break;
            }
        }
    }

    let mut clean_lines: Vec<String> = lines
        .iter()
        .enumerate()
        .filter(|&(idx, _)| idx != target_idx)
        .map(|(_, l)| l.to_string())
        .collect();

    if let Some(c_idx) = completed_heading_idx {
        let mut insert_pos = c_idx + 1;
        if insert_pos < lines.len() && lines[insert_pos].trim().is_empty() {
            insert_pos += 1;
        }

        let adjusted_insert_pos = if target_idx < insert_pos {
            insert_pos.saturating_sub(1)
        } else {
            insert_pos
        };

        let final_pos = adjusted_insert_pos.min(clean_lines.len());
        clean_lines.insert(final_pos, completed_task.clone());
    } else {
        clean_lines.insert(target_idx.min(clean_lines.len()), completed_task.clone());
    }

    let mut out = clean_lines.join("\n");
    if original.ends_with('\n') && !out.ends_with('\n') {
        out.push('\n');
    }

    Ok((out, completed_task))
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
        let target_path = resolve_vault_path(subfolder.as_deref(), &filename)?;

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

/// 2. Безпечно читає вміст нотатки Obsidian Vault.
#[tauri::command(rename_all = "snake_case")]
pub async fn read_obsidian_note(
    subfolder: Option<String>,
    filename: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = resolve_vault_path(subfolder.as_deref(), &filename)?;
        if !path.exists() {
            return Err(format!("Нотатку не знайдено: {}", path.display()));
        }
        if !path.is_file() {
            return Err(format!("Вказаний шлях є папкою, а не файлом: {}", path.display()));
        }
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Помилка читання нотатки '{}': {}", path.display(), e))?;

        Ok(truncate_chars(&content, MAX_READ_CHARS))
    })
    .await
    .map_err(|e| format!("Помилка потоку: {}", e))?
}

/// 3. Створює або перезаписує нотатку з повним Markdown-вмістом у Obsidian Vault.
#[tauri::command(rename_all = "snake_case")]
pub async fn create_or_update_obsidian_note(
    subfolder: Option<String>,
    filename: String,
    content: String,
    overwrite: bool,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = resolve_vault_path(subfolder.as_deref(), &filename)?;

        if path.exists() && !overwrite {
            return Err(format!(
                "Нотатка вже існує: {}. Якщо ви дійсно хочете перезаписати її, передайте overwrite=true або використовуйте дописування (append).",
                path.display()
            ));
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                format!("Не вдалося створити директорію '{}': {}", parent.display(), e)
            })?;
        }

        if path.exists() {
            let bak_path = format!("{}.bak", path.display());
            let _ = fs::copy(&path, bak_path);
        }

        fs::write(&path, content.as_bytes())
            .map_err(|e| format!("Помилка запису у нотатку '{}': {}", path.display(), e))?;

        Ok(format!(
            "Нотатку успішно збережено ({} байтів): {}",
            content.len(),
            path.display()
        ))
    })
    .await
    .map_err(|e| format!("Помилка потоку: {}", e))?
}

/// 4. Пошук нотаток у Obsidian Vault за назвою або повнотекстово по вмісту.
#[tauri::command(rename_all = "snake_case")]
pub async fn search_obsidian_notes(
    query: String,
    search_content: bool,
    max_results: Option<usize>,
) -> Result<Vec<ObsidianSearchResult>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let vault_base = get_vault_root()?;
        if !vault_base.exists() {
            return Err(format!("Obsidian Vault не знайдено за шляхом: {}", vault_base.display()));
        }

        let limit = max_results.unwrap_or(20);
        let q_lower = query.trim().to_lowercase();
        if q_lower.is_empty() {
            return Err("Пошуковий запит не може бути порожнім".to_string());
        }

        let mut results = Vec::new();

        fn walk_and_search(
            current_dir: &Path,
            vault_base: &Path,
            q_lower: &str,
            search_content: bool,
            limit: usize,
            results: &mut Vec<ObsidianSearchResult>,
        ) {
            if results.len() >= limit {
                return;
            }

            let entries = match fs::read_dir(current_dir) {
                Ok(e) => e,
                Err(_) => return,
            };

            for entry in entries.flatten() {
                if results.len() >= limit {
                    break;
                }
                let path = entry.path();
                let file_name = entry.file_name().to_string_lossy().to_string();

                if is_ignored_name(&file_name) {
                    continue;
                }

                if path.is_dir() {
                    walk_and_search(&path, vault_base, q_lower, search_content, limit, results);
                } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                    let rel_path = path
                        .strip_prefix(vault_base)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .to_string();

                    let name_matched = file_name.to_lowercase().contains(q_lower);
                    let mut snippet_matches = Vec::new();

                    if search_content {
                        if let Ok(content) = fs::read_to_string(&path) {
                            for line in content.lines() {
                                if snippet_matches.len() >= 3 {
                                    break;
                                }
                                if line.to_lowercase().contains(q_lower) {
                                    let clean_line = line.trim();
                                    if !clean_line.is_empty() {
                                        snippet_matches.push(clean_line.to_string());
                                    }
                                }
                            }
                        }
                    }

                    if name_matched || !snippet_matches.is_empty() {
                        results.push(ObsidianSearchResult {
                            filename: file_name,
                            relative_path: rel_path,
                            matches: snippet_matches,
                        });
                    }
                }
            }
        }

        walk_and_search(&vault_base, &vault_base, &q_lower, search_content, limit, &mut results);

        Ok(results)
    })
    .await
    .map_err(|e| format!("Помилка потоку: {}", e))?
}

/// 5. Перелік нотаток та папок у Obsidian Vault.
#[tauri::command(rename_all = "snake_case")]
pub async fn list_obsidian_notes(
    subfolder: Option<String>,
    recursive: bool,
) -> Result<Vec<ObsidianEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let vault_base = get_vault_root()?;
        let mut target_dir = vault_base.clone();

        if let Some(sub) = subfolder {
            let trimmed = sub.trim();
            if !trimmed.is_empty()
                && !["null", "none", "undefined", "nil", ".", "/", "~"].contains(&trimmed.to_lowercase().as_str())
            {
                let safe_sub = sanitize_relative_path(trimmed)?;
                target_dir = target_dir.join(safe_sub);
            }
        }

        if !target_dir.exists() {
            return Err(format!("Директорію не знайдено: {}", target_dir.display()));
        }

        let mut entries_list = Vec::new();

        fn walk_list(
            dir: &Path,
            vault_base: &Path,
            recursive: bool,
            results: &mut Vec<ObsidianEntry>,
        ) {
            if let Ok(rd) = fs::read_dir(dir) {
                for item in rd.flatten() {
                    let path = item.path();
                    let name = item.file_name().to_string_lossy().to_string();

                    if is_ignored_name(&name) {
                        continue;
                    }

                    let is_dir = path.is_dir();
                    let rel_path = path
                        .strip_prefix(vault_base)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .to_string();

                    let metadata = item.metadata().ok();
                    let size_bytes = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
                    let modified_at = metadata
                        .and_then(|m| m.modified().ok())
                        .map(|t| {
                            let datetime: chrono::DateTime<Local> = t.into();
                            datetime.format("%Y-%m-%d %H:%M").to_string()
                        })
                        .unwrap_or_default();

                    if is_dir {
                        results.push(ObsidianEntry {
                            name: name.clone(),
                            relative_path: rel_path,
                            is_dir: true,
                            size_bytes: 0,
                            modified_at,
                        });
                        if recursive {
                            walk_list(&path, vault_base, recursive, results);
                        }
                    } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                        results.push(ObsidianEntry {
                            name,
                            relative_path: rel_path,
                            is_dir: false,
                            size_bytes,
                            modified_at,
                        });
                    }
                }
            }
        }

        walk_list(&target_dir, &vault_base, recursive, &mut entries_list);
        Ok(entries_list)
    })
    .await
    .map_err(|e| format!("Помилка потоку: {}", e))?
}

/// 6. Створює або повертає щоденну нотатку (Daily Note) за датою `YYYY-MM-DD.md`.
#[tauri::command(rename_all = "snake_case")]
pub async fn get_or_create_daily_note(
    date: Option<String>,
    subfolder: Option<String>,
) -> Result<DailyNoteResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let date_str = match date {
            Some(d) if !d.trim().is_empty() => d.trim().to_string(),
            _ => Local::now().format("%Y-%m-%d").to_string(),
        };

        let filename = format!("{}.md", date_str);
        let path = resolve_vault_path(subfolder.as_deref(), &filename)?;
        let vault_root = get_vault_root()?;
        let rel_path = path
            .strip_prefix(&vault_root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();

        if path.exists() {
            let content = fs::read_to_string(&path)
                .map_err(|e| format!("Помилка читання щоденної нотатки '{}': {}", path.display(), e))?;
            Ok(DailyNoteResult {
                filename,
                relative_path: rel_path,
                content: truncate_chars(&content, MAX_READ_CHARS),
                created_new: false,
            })
        } else {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|e| {
                    format!("Не вдалося створити директорію '{}': {}", parent.display(), e)
                })?;
            }

            let initial_content = format!(
                "# Щоденна нотатка: {}\n\n## Задачі дня\n\n## Нотатки та думки\n\n",
                date_str
            );

            fs::write(&path, initial_content.as_bytes())
                .map_err(|e| format!("Помилка створення щоденної нотатки '{}': {}", path.display(), e))?;

            Ok(DailyNoteResult {
                filename,
                relative_path: rel_path,
                content: initial_content,
                created_new: true,
            })
        }
    })
    .await
    .map_err(|e| format!("Помилка потоку: {}", e))?
}

/// 7. Знаходить задачу за запитом у `TODO.md` (або вказаному файлі) та позначає її виконаною (`[x]`).
#[tauri::command(rename_all = "snake_case")]
pub async fn complete_obsidian_todo(
    subfolder: Option<String>,
    filename: Option<String>,
    task_query: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let file = filename.unwrap_or_else(|| "TODO.md".to_string());
        let path = resolve_vault_path(subfolder.as_deref(), &file)?;

        if !path.exists() {
            return Err(format!("Файл нотатки не знайдено: {}", path.display()));
        }

        let original = fs::read_to_string(&path)
            .map_err(|e| format!("Помилка читання нотатки '{}': {}", path.display(), e))?;

        let (updated, completed_line) = mark_todo_completed_in_markdown(&original, &task_query)?;

        fs::write(&path, updated.as_bytes())
            .map_err(|e| format!("Помилка запису у нотатку '{}': {}", path.display(), e))?;

        Ok(format!(
            "Задачу виконано у '{}': {}",
            path.file_name().unwrap_or_default().to_string_lossy(),
            completed_line
        ))
    })
    .await
    .map_err(|e| format!("Помилка потоку: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_vault_path_basic() {
        let path = resolve_vault_path(None, "my_note").unwrap();
        assert!(path.to_string_lossy().ends_with("my_note.md"));

        let path_with_ext = resolve_vault_path(Some("Subdir"), "nested.md").unwrap();
        assert!(path_with_ext.to_string_lossy().contains("Subdir"));
        assert!(path_with_ext.to_string_lossy().ends_with("nested.md"));
    }

    #[test]
    fn test_resolve_vault_path_traversal_prevention() {
        let err = resolve_vault_path(None, "../../../etc/passwd");
        assert!(err.is_err(), "Should disallow path traversal");
    }

    #[test]
    fn test_is_ignored_name() {
        assert!(is_ignored_name(".obsidian"));
        assert!(is_ignored_name(".git"));
        assert!(is_ignored_name(".trash"));
        assert!(is_ignored_name("trash"));
        assert!(is_ignored_name(".DS_Store"));
        assert!(!is_ignored_name("TODO.md"));
        assert!(!is_ignored_name("MyNote.md"));
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
    fn test_mark_todo_completed_with_completed_section() {
        let original = "## TO DO\n- [ ] Лабораторна робота з ASM @{2026-11-02}\n- [ ] Захист курсової\n\n## Completed\n- [x] Стара лаба\n";
        let (updated, completed) = mark_todo_completed_in_markdown(original, "ASM").unwrap();
        assert!(completed.contains("- [x] Лабораторна робота з ASM"));
        assert!(!updated.contains("## TO DO\n- [ ] Лабораторна робота з ASM"));
        assert!(updated.contains("## Completed\n- [x] Лабораторна робота з ASM"));
        assert!(updated.contains("- [x] Стара лаба"));
    }

    #[test]
    fn test_mark_todo_completed_without_completed_section() {
        let original = "## ЗАДАЧІ\n- [ ] Задача 1\n- [ ] Задача 2\n";
        let (updated, completed) = mark_todo_completed_in_markdown(original, "Задача 2").unwrap();
        assert!(completed.contains("- [x] Задача 2"));
        assert!(updated.contains("- [ ] Задача 1\n- [x] Задача 2"));
    }

    #[test]
    fn test_mark_todo_completed_not_found() {
        let original = "## TO DO\n- [ ] Задача 1\n";
        let res = mark_todo_completed_in_markdown(original, "Неіснуюча");
        assert!(res.is_err());
    }
}
