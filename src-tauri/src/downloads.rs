// Модуль розумного аналізу та прибирання папки ~/Downloads

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use crate::config::expand_tilde;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DownloadFileItem {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub mtime_secs: u64,
    pub category: String,
    pub is_kept_recent: bool,
    pub is_junk: bool,
    pub junk_reason: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CategoryPlan {
    pub category_name: String,
    pub files: Vec<DownloadFileItem>,
    pub total_size: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DownloadsAnalysisResult {
    pub downloads_path: String,
    pub total_files_count: usize,
    pub total_size: u64,
    pub kept_recent_files: Vec<DownloadFileItem>,
    pub categories: Vec<CategoryPlan>,
    pub junk_candidates: Vec<DownloadFileItem>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CleanupExecutionPlan {
    pub move_categories: bool,
    pub delete_junk_paths: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CleanupExecutionResult {
    pub moved_count: usize,
    pub deleted_count: usize,
    pub freed_bytes: u64,
    pub message: String,
}

/// Визначає категорію за розширенням
pub fn categorize_file(ext: &str) -> &'static str {
    match ext {
        "pdf" | "docx" | "doc" | "xlsx" | "xls" | "pptx" | "ppt" | "odt" | "ods" | "txt"
        | "csv" | "rtf" => "Документи",
        "png" | "jpg" | "jpeg" | "webp" | "svg" | "gif" | "ico" | "bmp" | "tiff" | "avif" => {
            "Зображення"
        }
        "mp4" | "mkv" | "avi" | "mov" | "webm" | "wmv" | "flv" | "mp3" | "wav" | "flac"
        | "ogg" | "m4a" | "opus" => "Відео та Аудіо",
        "zip" | "tar" | "gz" | "tgz" | "xz" | "7z" | "rar" | "bz2" | "zst" => "Архіви",
        "deb" | "appimage" | "rpm" | "iso" | "apk" | "exe" | "msi" | "sh" => "Інсталятори",
        "json" | "xml" | "yaml" | "yml" | "sql" | "sqlite" | "py" | "rs" | "ts" | "js"
        | "html" | "css" => "Код та Дані",
        _ => "Інше",
    }
}

/// Перевіряє, чи файл є кандидатом на видалення (мотлох)
pub fn check_junk_candidate(name: &str, ext: &str, mtime_secs: u64) -> (bool, Option<String>) {
    // 1. Недовантажені та тимчасові файли
    if ext == "crdownload" || ext == "part" || ext == "tmp" || ext == "downloading" {
        return (
            true,
            Some("Недовантажений або покинутий тимчасовий файл".to_string()),
        );
    }

    // 2. Дублікати з номерами копій у назві, наприклад "file (1).pdf"
    if is_numbered_duplicate(name) {
        return (
            true,
            Some("Ймовірний дублікат завантаженого файлу (копія з номером)".to_string()),
        );
    }

    // 3. Старі інсталятори (> 30 днів)
    let is_installer = matches!(ext, "deb" | "appimage" | "rpm" | "iso");
    if is_installer {
        let now_secs = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();
        let age_secs = now_secs.saturating_sub(mtime_secs);
        let thirty_days_secs = 30 * 24 * 3600;
        if age_secs > thirty_days_secs {
            let days = age_secs / 86400;
            return (
                true,
                Some(format!("Старий пакет/образ інсталятора (завантажено {} днів тому)", days)),
            );
        }
    }

    (false, None)
}

/// Перевіряє шаблон копії з дублікатом у кінці назви перед розширенням, напр. " (1)" або " (2)"
fn is_numbered_duplicate(name: &str) -> bool {
    let stem = match Path::new(name).file_stem().and_then(|s| s.to_str()) {
        Some(s) => s,
        None => return false,
    };

    if let Some(open_idx) = stem.rfind(" (") {
        if stem.ends_with(')') && open_idx + 2 < stem.len() {
            let num_str = &stem[open_idx + 2..stem.len() - 1];
            return num_str.chars().all(|c| c.is_ascii_digit());
        }
    }
    false
}

/// Аналізує папку ~/Downloads
#[tauri::command]
pub async fn analyze_downloads() -> Result<DownloadsAnalysisResult, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let dir = expand_tilde("~/Downloads")?;
        let downloads_path = dir.to_string_lossy().to_string();

        if !dir.exists() || !dir.is_dir() {
            return Err("Папку ~/Downloads не знайдено".to_string());
        }

        let mut files: Vec<DownloadFileItem> = Vec::new();

        let entries = fs::read_dir(&dir)
            .map_err(|e| format!("Не вдалося прочитати вміст ~/Downloads: {}", e))?;

        for entry_res in entries {
            let entry = match entry_res {
                Ok(e) => e,
                Err(_) => continue,
            };

            let file_type = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };

            // Працюємо з файлами у корені ~/Downloads (ігноруємо вже створені підпапки)
            if !file_type.is_file() {
                continue;
            }

            let path = entry.path();
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file")
                .to_string();

            // Ігноруємо приховані файли .config тощо
            if name.starts_with('.') {
                continue;
            }

            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };

            let size = metadata.len();
            let mtime_secs = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);

            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_lowercase())
                .unwrap_or_default();

            let category = categorize_file(&ext).to_string();
            let (is_junk, junk_reason) = check_junk_candidate(&name, &ext, mtime_secs);

            files.push(DownloadFileItem {
                name,
                path: path.to_string_lossy().to_string(),
                size,
                mtime_secs,
                category,
                is_kept_recent: false,
                is_junk,
                junk_reason,
            });
        }

        // Сортуємо файли від найновіших до найстаріших за mtime
        files.sort_by(|a, b| b.mtime_secs.cmp(&a.mtime_secs));

        // Перші 5 найновіших файлів позначаємо як is_kept_recent: true
        let mut kept_recent_files = Vec::new();
        let total_count = files.len();
        let mut total_size: u64 = 0;

        for (idx, item) in files.iter_mut().enumerate() {
            total_size += item.size;
            if idx < 5 {
                item.is_kept_recent = true;
                kept_recent_files.push(item.clone());
            }
        }

        // Групуємо файли, які не є найновішими, за категоріями для переміщення
        let mut cat_map: std::collections::HashMap<String, Vec<DownloadFileItem>> =
            std::collections::HashMap::new();
        let mut junk_candidates: Vec<DownloadFileItem> = Vec::new();

        for item in &files {
            if item.is_junk {
                junk_candidates.push(item.clone());
            }
            if !item.is_kept_recent {
                cat_map
                    .entry(item.category.clone())
                    .or_default()
                    .push(item.clone());
            }
        }

        let mut categories = Vec::new();
        let order = [
            "Документи",
            "Зображення",
            "Відео та Аудіо",
            "Архіви",
            "Інсталятори",
            "Код та Дані",
            "Інше",
        ];

        for cat_name in order {
            if let Some(items) = cat_map.remove(cat_name) {
                let cat_size: u64 = items.iter().map(|i| i.size).sum();
                categories.push(CategoryPlan {
                    category_name: cat_name.to_string(),
                    files: items,
                    total_size: cat_size,
                });
            }
        }

        Ok(DownloadsAnalysisResult {
            downloads_path,
            total_files_count: total_count,
            total_size,
            kept_recent_files,
            categories,
            junk_candidates,
        })
    })
    .await
    .map_err(|e| format!("Помилка аналізу завантажень: {}", e))?
}

/// Виконує схвалений план прибирання
#[tauri::command]
pub async fn execute_downloads_cleanup(
    plan: CleanupExecutionPlan,
) -> Result<CleanupExecutionResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = expand_tilde("~/Downloads")?;

        let mut moved_count = 0;
        let mut deleted_count = 0;
        let mut freed_bytes: u64 = 0;

        // 1. Спочатку видаляємо обраний мотлох
        for junk_path in plan.delete_junk_paths {
            let p = PathBuf::from(&junk_path);
            if p.exists() {
                let size = fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                if Command::new("gio").args(["trash", &junk_path]).status().is_ok() {
                    deleted_count += 1;
                    freed_bytes += size;
                } else if fs::remove_file(&p).is_ok() {
                    deleted_count += 1;
                    freed_bytes += size;
                }
            }
        }

        // 2. Якщо користувач увімкнув сортування за категоріями
        if plan.move_categories {
            let analysis = tauri::async_runtime::block_on(analyze_downloads())
                .map_err(|e| format!("Помилка оновлення списку перед сортуванням: {}", e))?;

            for cat in analysis.categories {
                if cat.files.is_empty() {
                    continue;
                }

                let target_dir = dir.join(&cat.category_name);
                let _ = fs::create_dir_all(&target_dir);

                for file in cat.files {
                    let source = PathBuf::from(&file.path);
                    if !source.exists() {
                        continue;
                    }

                    let dest = target_dir.join(&file.name);
                    let final_dest = if dest.exists() {
                        // Якщо файл з такою назвою вже є у цільовій папці, додаємо позначку часу
                        let stem = source.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
                        let ext = source.extension().and_then(|e| e.to_str()).unwrap_or("");
                        let unique_name = if ext.is_empty() {
                            format!("{}_{}", stem, SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs())
                        } else {
                            format!("{}_{}.{}", stem, SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs(), ext)
                        };
                        target_dir.join(unique_name)
                    } else {
                        dest
                    };

                    if fs::rename(&source, &final_dest).is_ok() {
                        moved_count += 1;
                    }
                }
            }
        }

        let message = format!(
            "Прибирання завершено! Відсортовано файлів: {}, видалено мотлоху: {}, звільнено: {:.1} MB. 5 найновіших файлів залишено в корені.",
            moved_count,
            deleted_count,
            freed_bytes as f64 / (1024.0 * 1024.0)
        );

        Ok(CleanupExecutionResult {
            moved_count,
            deleted_count,
            freed_bytes,
            message,
        })
    })
    .await
    .map_err(|e| format!("Помилка виконання прибирання: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_categorize_file_types() {
        assert_eq!(categorize_file("pdf"), "Документи");
        assert_eq!(categorize_file("png"), "Зображення");
        assert_eq!(categorize_file("mp4"), "Відео та Аудіо");
        assert_eq!(categorize_file("zip"), "Архіви");
        assert_eq!(categorize_file("deb"), "Інсталятори");
        assert_eq!(categorize_file("rs"), "Код та Дані");
        assert_eq!(categorize_file("xyz"), "Інше");
    }

    #[test]
    fn test_check_junk_candidate_crdownload() {
        let (is_junk, reason) = check_junk_candidate("test.crdownload", "crdownload", 1000);
        assert!(is_junk);
        assert!(reason.unwrap().contains("Недовантажений"));
    }

    #[test]
    fn test_check_junk_candidate_duplicate() {
        let (is_junk, reason) = check_junk_candidate("invoice (1).pdf", "pdf", 1000);
        assert!(is_junk);
        assert!(reason.unwrap().contains("дублікат"));
    }

    #[test]
    fn test_is_numbered_duplicate() {
        assert!(is_numbered_duplicate("doc (1).txt"));
        assert!(is_numbered_duplicate("photo (12).png"));
        assert!(!is_numbered_duplicate("doc (final).txt"));
        assert!(!is_numbered_duplicate("regular_file.pdf"));
    }
}
