// Модуль швидкого аналізу дискового простору у стилі WizTree та пошуку важких папок

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Instant;
use tauri::AppHandle;

use crate::config::expand_tilde;
use crate::window::open_or_focus_window;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DiskItem {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub children_count: usize,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub children: Vec<DiskItem>,
    pub extension: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ExtensionStat {
    pub extension: String,
    pub total_size: u64,
    pub count: usize,
    pub color: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DevClutterItem {
    pub path: String,
    pub name: String,
    pub category: String,
    pub size: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DiskScanResult {
    pub root_path: String,
    pub total_space: u64,
    pub available_space: u64,
    pub used_space: u64,
    pub root_item: DiskItem,
    pub extensions: Vec<ExtensionStat>,
    pub dev_clutter: Vec<DevClutterItem>,
    pub scan_duration_ms: u64,
}

/// Прив'язка кольорів розширень у стилі WizTree
fn get_color_for_extension(ext: &str) -> &'static str {
    match ext {
        // Відео
        "mp4" | "mkv" | "avi" | "mov" | "webm" | "wmv" | "flv" | "m4v" => "#f59e0b", // Amber/Gold
        // Аудіо
        "mp3" | "wav" | "flac" | "ogg" | "aac" | "m4a" | "opus" => "#eab308", // Yellow
        // Архіви та образи дисків
        "zip" | "tar" | "gz" | "tgz" | "xz" | "7z" | "rar" | "iso" | "bz2" => "#ef4444", // Red
        // Інсталятори та пакети
        "deb" | "appimage" | "rpm" | "exe" | "msi" | "apk" => "#ec4899", // Pink
        // Зображення
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "svg" | "bmp" | "ico" | "tiff" => "#10b981", // Emerald
        // Документи
        "pdf" | "docx" | "doc" | "xlsx" | "xls" | "pptx" | "ppt" | "odt" | "ods" => "#3b82f6", // Blue
        // Код та скрипти
        "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "cpp" | "c" | "java" | "html" | "css" => "#8b5cf6", // Purple
        // Дані та конфіги
        "json" | "xml" | "yaml" | "yml" | "toml" | "sql" | "sqlite" | "db" => "#06b6d4", // Cyan
        // Текстові
        "txt" | "md" | "log" | "ini" | "cfg" => "#94a3b8", // Slate
        _ => "#64748b",
    }
}

/// Отримує інформацію про загальний, зайнятий і вільний дисковий простір
fn get_filesystem_space(path: &Path) -> (u64, u64, u64) {
    let p_str = path.to_string_lossy();
    if let Ok(output) = Command::new("df").args(["-B1", &p_str]).output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // Формат виводу df -B1:
            // Filesystem 1B-blocks Used Available Use% Mounted on
            // /dev/...   100000000 6000 30000000 60%  /
            let lines: Vec<&str> = stdout.lines().collect();
            if lines.len() >= 2 {
                let parts: Vec<&str> = lines[1].split_whitespace().collect();
                if parts.len() >= 4 {
                    let total = parts[1].parse::<u64>().unwrap_or(0);
                    let used = parts[2].parse::<u64>().unwrap_or(0);
                    let avail = parts[3].parse::<u64>().unwrap_or(0);
                    return (total, used, avail);
                }
            }
        }
    }
    (0, 0, 0)
}

/// Перевіряє, чи є папка важким сміттям розробника
fn detect_clutter_category(name: &str) -> Option<&'static str> {
    match name {
        "node_modules" => Some("node_modules (Node.js)"),
        "target" => Some("target (Rust/Cargo)"),
        ".cache" => Some("System/User Cache"),
        ".next" => Some("Next.js Build Cache"),
        ".turbo" => Some("Turborepo Cache"),
        "__pycache__" => Some("Python Cache"),
        ".pytest_cache" => Some("Pytest Cache"),
        ".venv" | "venv" => Some("Python Virtualenv"),
        ".gradle" => Some("Gradle Cache"),
        "vendor" => Some("PHP/Go Vendor"),
        "dist" | "build" => Some("Build Artifacts"),
        _ => None,
    }
}

/// Рекурсивне сканування папки
fn scan_dir_recursive(
    path: &Path,
    current_depth: usize,
    max_tree_depth: usize,
    ext_map: &mut HashMap<String, (u64, usize)>,
    clutter_list: &mut Vec<DevClutterItem>,
) -> DiskItem {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path.to_str().unwrap_or("root"))
        .to_string();

    let path_buf = path.to_path_buf();
    let path_str = path_buf.to_string_lossy().to_string();

    // Перевірка на категорію мотлоху
    let clutter_category = detect_clutter_category(&name);

    let mut total_size: u64 = 0;
    let mut children = Vec::new();
    let mut children_count = 0;

    let entries = match fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => {
            return DiskItem {
                name,
                path: path_str,
                is_dir: true,
                size: 0,
                children_count: 0,
                children: Vec::new(),
                extension: None,
            };
        }
    };

    for entry_res in entries {
        let entry = match entry_res {
            Ok(e) => e,
            Err(_) => continue,
        };

        let child_path = entry.path();
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };

        // Захист від зациклення символьних посилань
        if file_type.is_symlink() {
            continue;
        }

        children_count += 1;

        if file_type.is_dir() {
            // Якщо це директорія, рекурсивно обробляємо її
            let child_item = scan_dir_recursive(
                &child_path,
                current_depth + 1,
                max_tree_depth,
                ext_map,
                clutter_list,
            );
            total_size += child_item.size;

            // Зберігаємо дітей лише до визначеної глибини дерева для економії пам'яті
            if current_depth < max_tree_depth {
                children.push(child_item);
            }
        } else if file_type.is_file() {
            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let file_size = metadata.len();
            total_size += file_size;

            let ext = child_path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_lowercase())
                .unwrap_or_else(|| "none".to_string());

            let stat = ext_map.entry(ext.clone()).or_insert((0, 0));
            stat.0 += file_size;
            stat.1 += 1;

            if current_depth < max_tree_depth {
                let file_name = child_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("file")
                    .to_string();

                children.push(DiskItem {
                    name: file_name,
                    path: child_path.to_string_lossy().to_string(),
                    is_dir: false,
                    size: file_size,
                    children_count: 0,
                    children: Vec::new(),
                    extension: Some(ext),
                });
            }
        }
    }

    // Якщо це була папка з категорії мотлоху (node_modules, target тощо) і вона важча за 5 МБ
    if let Some(cat) = clutter_category {
        if total_size >= 5 * 1024 * 1024 {
            clutter_list.push(DevClutterItem {
                path: path_str.clone(),
                name: name.clone(),
                category: cat.to_string(),
                size: total_size,
            });
        }
    }

    // Сортуємо дітей за спаданням розміру
    children.sort_by(|a, b| b.size.cmp(&a.size));

    DiskItem {
        name,
        path: path_str,
        is_dir: true,
        size: total_size,
        children_count,
        children,
        extension: None,
    }
}

/// Команда Tauri: Сканування директорії для WizTree
#[tauri::command]
pub async fn scan_disk_space(
    target_path: Option<String>,
    max_depth: Option<usize>,
) -> Result<DiskScanResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let start_time = Instant::now();

        let raw_path = target_path.unwrap_or_else(|| "~".to_string());
        let root = expand_tilde(&raw_path)?;
        let expanded = root.to_string_lossy().to_string();

        if !root.exists() {
            return Err(format!("Шлях не існує: {}", expanded));
        }

        let max_tree_depth = max_depth.unwrap_or(4);
        let (total_space, used_space, available_space) = get_filesystem_space(&root);

        let mut ext_map: HashMap<String, (u64, usize)> = HashMap::new();
        let mut clutter_list: Vec<DevClutterItem> = Vec::new();

        let root_item = scan_dir_recursive(
            &root,
            0,
            max_tree_depth,
            &mut ext_map,
            &mut clutter_list,
        );

        // Перетворюємо мапу розширень у відсортований список статистики
        let mut extensions: Vec<ExtensionStat> = ext_map
            .into_iter()
            .map(|(ext, (size, count))| {
                let color = get_color_for_extension(&ext).to_string();
                ExtensionStat {
                    extension: ext,
                    total_size: size,
                    count,
                    color,
                }
            })
            .collect();

        extensions.sort_by(|a, b| b.total_size.cmp(&a.total_size));

        // Сортуємо знайдений мотлох за спаданням розміру
        clutter_list.sort_by(|a, b| b.size.cmp(&a.size));

        let duration = start_time.elapsed().as_millis() as u64;

        Ok(DiskScanResult {
            root_path: root.to_string_lossy().to_string(),
            total_space,
            available_space,
            used_space,
            root_item,
            extensions,
            dev_clutter: clutter_list,
            scan_duration_ms: duration,
        })
    })
    .await
    .map_err(|e| format!("Помилка фонового потоку сканування: {}", e))?
}

/// Команда Tauri: Видалення файлу або папки (в кошик або назавжди)
#[tauri::command]
pub async fn delete_disk_item(path: String, to_trash: bool) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let p = expand_tilde(&path)?;
        let expanded = p.to_string_lossy().to_string();

        if !p.exists() {
            return Err(format!("Елемент не знайдено: {}", expanded));
        }

        // Захист від видалення кореневих та критичних системних директорій
        let canonical = p.canonicalize().map_err(|e| e.to_string())?;
        let forbidden = ["/", "/home", "/root", "/etc", "/usr", "/bin", "/boot", "/var"];
        for f in forbidden {
            if canonical == Path::new(f) {
                return Err(format!("Видалення системної директорії {} суворо заборонено!", f));
            }
        }

        if to_trash {
            // Використовуємо gio trash для безпечного переміщення у кошик на Linux
            if let Ok(output) = Command::new("gio").args(["trash", &expanded]).output() {
                if output.status.success() {
                    return Ok(format!("Елемент переміщено у кошик: {}", expanded));
                }
            }
        }

        // Якщо видалення назавжди або gio trash не спрацював
        if p.is_dir() {
            fs::remove_dir_all(&p)
                .map_err(|e| format!("Не вдалося видалити папку {}: {}", expanded, e))?;
            Ok(format!("Папку видалено назавжди: {}", expanded))
        } else {
            fs::remove_file(&p)
                .map_err(|e| format!("Не вдалося видалити файл {}: {}", expanded, e))?;
            Ok(format!("Файл видалено назавжди: {}", expanded))
        }
    })
    .await
    .map_err(|e| format!("Помилка фонового потоку видалення: {}", e))?
}

/// Команда Tauri: Відкрити окреме вікно аналізатора диска WizTree
#[tauri::command]
pub async fn open_disk_analyzer_window(
    app: AppHandle,
    path: Option<String>,
) -> Result<(), String> {
    let target = path.unwrap_or_else(|| "~".to_string());
    let url = format!("index.html?view=disk_analyzer&path={}", urlencoding(&target));
    open_or_focus_window(
        &app,
        "disk_analyzer",
        "WizTree — Аналізатор дискового простору",
        &url,
        1050.0,
        700.0,
        true,
    )
}

pub fn urlencoding(input: &str) -> String {
    let mut output = String::new();
    for b in input.bytes() {
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' || b == b'~' || b == b'/' {
            output.push(b as char);
        } else {
            output.push_str(&format!("%{:02X}", b));
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_color_for_extension() {
        assert_eq!(get_color_for_extension("mp4"), "#f59e0b");
        assert_eq!(get_color_for_extension("zip"), "#ef4444");
        assert_eq!(get_color_for_extension("png"), "#10b981");
        assert_eq!(get_color_for_extension("rs"), "#8b5cf6");
        assert_eq!(get_color_for_extension("unknown"), "#64748b");
    }

    #[test]
    fn test_detect_clutter_category() {
        assert_eq!(
            detect_clutter_category("node_modules"),
            Some("node_modules (Node.js)")
        );
        assert_eq!(
            detect_clutter_category("target"),
            Some("target (Rust/Cargo)")
        );
        assert_eq!(
            detect_clutter_category(".cache"),
            Some("System/User Cache")
        );
        assert_eq!(detect_clutter_category("my_documents"), None);
    }

    #[test]
    fn test_urlencoding() {
        assert_eq!(
            urlencoding("/home/user/my file.txt"),
            "/home/user/my%20file.txt"
        );
    }

    #[test]
    fn test_scan_dir_recursive() {
        let temp_dir = std::env::temp_dir().join(format!("test_wiztree_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let sub = temp_dir.join("subdir");
        let _ = fs::create_dir_all(&sub);
        let f1 = temp_dir.join("sample.txt");
        let _ = fs::write(&f1, "1234567890"); // 10 bytes
        let f2 = sub.join("sample.png");
        let _ = fs::write(&f2, "12345"); // 5 bytes

        let mut ext_map = HashMap::new();
        let mut clutter_list = Vec::new();

        let item = scan_dir_recursive(&temp_dir, 0, 4, &mut ext_map, &mut clutter_list);

        assert_eq!(item.size, 15);
        assert_eq!(ext_map.get("txt").unwrap().0, 10);
        assert_eq!(ext_map.get("png").unwrap().0, 5);

        let _ = fs::remove_dir_all(&temp_dir);
    }
}

