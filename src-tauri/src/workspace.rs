// Модуль визначення активного робочого простору та відкритого файлу у VS Code

use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ActiveWorkspaceInfo {
    pub workspace_path: Option<String>,
    pub active_file: Option<String>,
    pub source: String,
}

pub fn detect_active_workspace_internal() -> ActiveWorkspaceInfo {
    let py_script = r#"
import os, sys, json, glob, sqlite3, urllib.parse

def detect():
    home = os.path.expanduser('~')
    workspace_path = None
    active_file = None
    source = 'none'

    # 1. Спроба через VS Code window state
    vscode_storage = os.path.join(home, '.config', 'Code', 'User', 'globalStorage', 'storage.json')
    if os.path.isfile(vscode_storage):
        try:
            with open(vscode_storage, 'r', encoding='utf-8') as f:
                data = json.load(f)
                last_win = data.get('windowsState', {}).get('lastActiveWindow')
                if last_win:
                    folder = last_win.get('folder')
                    if folder and folder.startswith('file://'):
                        workspace_path = urllib.parse.unquote(folder[7:])
                        source = 'vscode_storage'
                    elif folder:
                        workspace_path = folder
                        source = 'vscode_storage'
        except Exception:
            pass

    # 2. Пошук останнього відкритого файлу у VS Code workspaceStorage
    ws_storage_dir = os.path.join(home, '.config', 'Code', 'User', 'workspaceStorage')
    if os.path.isdir(ws_storage_dir):
        ws_folders = [os.path.join(ws_storage_dir, d) for d in os.listdir(ws_storage_dir)]
        ws_folders.sort(key=lambda x: os.path.getmtime(x) if os.path.exists(x) else 0, reverse=True)
        
        matched_ws_dir = None
        best_mtime = 0
        for wsd in ws_folders[:10]:
            ws_json = os.path.join(wsd, 'workspace.json')
            if os.path.isfile(ws_json):
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
            pass

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
pub async fn get_active_workspace() -> Result<ActiveWorkspaceInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        Ok(detect_active_workspace_internal())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_active_workspace_internal() {
        let info = detect_active_workspace_internal();
        println!("Detected workspace: {:?}", info);
        assert!(!info.source.is_empty());
    }
}
