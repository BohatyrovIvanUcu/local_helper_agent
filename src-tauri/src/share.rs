// Модуль локального Wi-Fi шерингу файлів (Quick HTTP Share)

use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

use crate::config::expand_tilde;
use crate::window::open_or_focus_window;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ShareSessionInfo {
    pub file_path: String,
    pub file_name: String,
    pub file_size: u64,
    pub is_dir: bool,
    pub ip_address: String,
    pub port: u16,
    pub share_url: String,
    pub local_url: String,
    pub status: String,
    pub temp_zip_path: Option<String>,
}

struct ActiveServer {
    is_running: Arc<AtomicBool>,
    info: ShareSessionInfo,
    thread_handle: Option<thread::JoinHandle<()>>,
    tunnel_child: Option<std::process::Child>,
}

static SERVER_STATE: Mutex<Option<ActiveServer>> = Mutex::new(None);

/// Визначає локальну IP-адресу в LAN/Wi-Fi без надсилання трафіку
pub fn get_local_ip() -> String {
    if let Ok(socket) = UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = socket.local_addr() {
                let ip = addr.ip().to_string();
                if ip != "0.0.0.0" && !ip.starts_with("127.") {
                    return ip;
                }
            }
        }
    }

    if let Ok(out) = Command::new("hostname").arg("-I").output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            if let Some(first_ip) = s.split_whitespace().next() {
                if !first_ip.starts_with("127.") {
                    return first_ip.to_string();
                }
            }
        }
    }

    "127.0.0.1".to_string()
}

/// Форматує байти у людинозрозумілий рядок
fn format_file_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}

/// Кодує назву файлу згідно з RFC 5987 / RFC 6266 для сумісності з Android Chrome та UTF-8
fn encode_rfc5987(filename: &str) -> String {
    let mut out = String::new();
    for b in filename.bytes() {
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

/// Визначає MIME-тип файлу
fn get_mime_type(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "mp4" => "video/mp4",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "tar" | "gz" | "tgz" => "application/gzip",
        "txt" => "text/plain; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Генерує мобільну сторінку для скачування файлу
fn generate_landing_html(
    file_name: &str,
    file_size_formatted: &str,
    is_dir: bool,
    is_media: bool,
    mime_type: &str,
) -> String {
    let preview_tag = if is_media {
        if mime_type.starts_with("image/") {
            r#"<div class="preview"><img src="/preview" alt="Прев'ю" /></div>"#
        } else if mime_type.starts_with("video/") {
            r#"<div class="preview"><video controls src="/preview"></video></div>"#
        } else if mime_type.starts_with("audio/") {
            r#"<div class="preview"><audio controls src="/preview"></audio></div>"#
        } else {
            ""
        }
    } else {
        ""
    };

    let item_type_desc = if is_dir {
        "📁 Папка (буде завантажена як ZIP)"
    } else {
        "📄 Файл"
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="uk">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Завантаження — Local Helper Mascot</title>
    <style>
        * {{ box-sizing: border-box; margin: 0; padding: 0; }}
        body {{
            background: #0f172a;
            color: #f8fafc;
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
            display: flex;
            align-items: center;
            justify-content: center;
            min-height: 100vh;
            padding: 20px;
        }}
        .card {{
            background: #1e293b;
            border: 1px solid #334155;
            border-radius: 20px;
            padding: 28px 24px;
            max-width: 440px;
            width: 100%;
            text-align: center;
            box-shadow: 0 20px 25px -5px rgba(0, 0, 0, 0.5), 0 8px 10px -6px rgba(0, 0, 0, 0.4);
        }}
        .badge {{
            display: inline-block;
            background: rgba(99, 102, 241, 0.15);
            color: #818cf8;
            font-size: 13px;
            font-weight: 600;
            padding: 6px 14px;
            border-radius: 9999px;
            margin-bottom: 18px;
            border: 1px solid rgba(99, 102, 241, 0.3);
        }}
        h1 {{
            font-size: 20px;
            font-weight: 700;
            margin-bottom: 8px;
            word-break: break-all;
            color: #ffffff;
            line-height: 1.3;
        }}
        .size {{
            color: #94a3b8;
            font-size: 15px;
            margin-bottom: 20px;
        }}
        .preview {{
            margin-bottom: 22px;
            border-radius: 14px;
            overflow: hidden;
            background: #0f172a;
            border: 1px solid #334155;
        }}
        .preview img, .preview video {{
            max-width: 100%;
            max-height: 280px;
            display: block;
            margin: 0 auto;
        }}
        .preview audio {{
            width: 100%;
            margin-top: 10px;
        }}
        .btn {{
            display: block;
            width: 100%;
            background: linear-gradient(135deg, #6366f1 0%, #4f46e5 100%);
            color: #ffffff;
            font-size: 17px;
            font-weight: 600;
            padding: 16px;
            border-radius: 14px;
            text-decoration: none;
            transition: all 0.2s ease;
            box-shadow: 0 4px 14px rgba(79, 70, 229, 0.4);
        }}
        .btn:hover {{
            background: linear-gradient(135deg, #4f46e5 0%, #4338ca 100%);
            transform: translateY(-2px);
        }}
        .btn:active {{
            transform: translateY(0);
        }}
        .footer {{
            margin-top: 22px;
            font-size: 12px;
            color: #64748b;
        }}
    </style>
</head>
<body>
    <div class="card">
        <div class="badge">🤖 Local Helper Wi-Fi Share</div>
        <h1>{}</h1>
        <div class="size">{} • {}</div>
        {}
        <a href="/download" class="btn" download>⬇️ Завантажити ({})</a>
        <div class="footer">Локальна пряма передача без інтернету</div>
    </div>
</body>
</html>"#,
        file_name, item_type_desc, file_size_formatted, preview_tag, file_size_formatted
    )
}

/// Запускає безпечний публічний HTTPS-тунель через Tunnelmole
fn spawn_tunnel(port: u16) -> (Option<String>, Option<std::process::Child>) {
    let local_tmole_path = expand_tilde("~/local_helper_ai/node_modules/tunnelmole/dist/bin/tunnelmole.js")
        .unwrap_or_else(|_| PathBuf::from("./node_modules/tunnelmole/dist/bin/tunnelmole.js"));

    let mut cmd = if Path::new("./node_modules/tunnelmole/dist/bin/tunnelmole.js").exists() {
        let mut c = Command::new("node");
        c.arg("./node_modules/tunnelmole/dist/bin/tunnelmole.js").arg(port.to_string());
        c
    } else if local_tmole_path.exists() {
        let mut c = Command::new("node");
        c.arg(local_tmole_path).arg(port.to_string());
        c
    } else if let Ok(out) = Command::new("which").arg("tmole").output() {
        if out.status.success() {
            let mut c = Command::new("tmole");
            c.arg(port.to_string());
            c
        } else {
            let mut c = Command::new("npx");
            c.args(["--yes", "tunnelmole", &port.to_string()]);
            c
        }
    } else {
        let mut c = Command::new("npx");
        c.args(["--yes", "tunnelmole", &port.to_string()]);
        c
    };

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[Wi-Fi Share Tunnel] Не вдалося запустити тунель: {}", e);
            return (None, None);
        }
    };

    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => return (None, Some(child)),
    };
    let stderr = child.stderr.take();

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        let mut sent = false;
        for line in reader.lines() {
            if let Ok(l) = line {
                eprintln!("[Wi-Fi Share Tunnel OUT] {}", l);
                if !sent {
                    if let Some(start_idx) = l.find("https://") {
                        let sub = &l[start_idx..];
                        let end_idx = sub
                            .find(|c: char| c.is_whitespace() || c == '⟶')
                            .unwrap_or(sub.len());
                        let url = sub[..end_idx].trim().to_string();
                        if url.contains(".tunnelmole.net") || url.contains(".loca.lt") {
                            let _ = tx.send(url);
                            sent = true;
                        }
                    }
                }
            }
        }
        eprintln!("[Wi-Fi Share Tunnel OUT] stdout closed (child exited)");
    });

    if let Some(err_stream) = stderr {
        std::thread::spawn(move || {
            let reader = BufReader::new(err_stream);
            for line in reader.lines() {
                if let Ok(l) = line {
                    eprintln!("[Wi-Fi Share Tunnel ERR] {}", l);
                }
            }
            eprintln!("[Wi-Fi Share Tunnel ERR] stderr closed");
        });
    }

    let tunnel_url = rx.recv_timeout(Duration::from_millis(3500)).ok();
    if let Some(ref u) = tunnel_url {
        eprintln!("[Wi-Fi Share Tunnel] Успішно піднято HTTPS-тунель: {}", u);
    } else {
        eprintln!("[Wi-Fi Share Tunnel] Тунель не відповів за 3.5с, використовуємо локальну LAN адресу");
    }
    (tunnel_url, Some(child))
}

/// Запускає локальний HTTP сервер для роздачі файлу
#[tauri::command]
pub async fn start_file_share(
    app: AppHandle,
    file_path: String,
) -> Result<ShareSessionInfo, String> {
    let target = expand_tilde(&file_path)?;
    let expanded = target.to_string_lossy().to_string();

    if !target.exists() {
        return Err(format!("Файл або папку не знайдено: {}", expanded));
    }

    // Перевіряємо, чи цей самий файл вже активно роздається на поточному сервері
    {
        let lock = SERVER_STATE.lock().unwrap();
        if let Some(ref server) = *lock {
            if server.is_running.load(Ordering::SeqCst) && server.info.file_path == expanded {
                eprintln!("[Wi-Fi Share] Сервер уже активно роздає цей файл на {}", server.info.share_url);
                return Ok(server.info.clone());
            }
        }
    }

    // Тільки якщо роздається інший файл або сервера немає — зупиняємо попередній
    stop_server_internal();

    let is_dir = target.is_dir();
    let file_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();

    let (serve_path, file_size, temp_zip_path) = if is_dir {
        // Якщо це папка, створюємо тимчасовий ZIP-архів
        let tmp_zip = std::env::temp_dir().join(format!("share_{}_{}.zip", file_name, std::process::id()));
        let parent = target.parent().unwrap_or(&target);
        
        let output = Command::new("zip")
            .current_dir(parent)
            .args(["-r", tmp_zip.to_str().unwrap_or(""), &file_name])
            .output()
            .map_err(|e| format!("Помилка створення ZIP для роздачі: {}", e))?;

        if !output.status.success() {
            return Err("Не вдалося запакувати папку в ZIP".to_string());
        }

        let size = fs::metadata(&tmp_zip).map(|m| m.len()).unwrap_or(0);
        (tmp_zip.clone(), size, Some(tmp_zip.to_string_lossy().to_string()))
    } else {
        let size = fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
        (target.clone(), size, None)
    };

    let ip = get_local_ip();
    let listener = TcpListener::bind("0.0.0.0:1422")
        .or_else(|_| TcpListener::bind("0.0.0.0:0"))
        .map_err(|e| format!("Не вдалося відкрити TCP-порт для роздачі: {}", e))?;
    
    // Встановлюємо non-blocking або короткий timeout для можливості перевірки is_running
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;

    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let local_url = format!("http://{}:{}/", ip, port);

    // Спроба підняти безпечний публічний HTTPS-тунель через Tunnelmole
    let (tunnel_url, tunnel_child) = spawn_tunnel(port);
    let share_url = tunnel_url.unwrap_or_else(|| local_url.clone());

    let session_info = ShareSessionInfo {
        file_path: expanded,
        file_name: file_name.clone(),
        file_size,
        is_dir,
        ip_address: ip,
        port,
        share_url,
        local_url,
        status: "active".to_string(),
        temp_zip_path,
    };

    let is_running = Arc::new(AtomicBool::new(true));
    let is_running_clone = is_running.clone();
    let serve_path_clone = serve_path.clone();
    let file_name_clone = file_name.clone();
    let session_info_clone = session_info.clone();
    let app_handle = app.clone();

    let thread_handle = thread::spawn(move || {
        let mime_type = get_mime_type(&serve_path_clone);
        let is_media = mime_type.starts_with("image/")
            || mime_type.starts_with("video/")
            || mime_type.starts_with("audio/");
        let formatted_size = format_file_size(file_size);
        let landing_html = Arc::new(generate_landing_html(
            &file_name_clone,
            &formatted_size,
            is_dir,
            is_media,
            mime_type,
        ));

        while is_running_clone.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let serve_path = serve_path_clone.clone();
                    let file_name = file_name_clone.clone();
                    let landing = landing_html.clone();
                    let app = app_handle.clone();
                    let session = session_info_clone.clone();

                    thread::spawn(move || {
                        handle_client(
                            stream,
                            &serve_path,
                            &file_name,
                            file_size,
                            is_dir,
                            &landing,
                            &app,
                            &session,
                        );
                    });
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(_) => {
                    thread::sleep(Duration::from_millis(100));
                }
            }
        }
    });

    let mut lock = SERVER_STATE.lock().unwrap();
    *lock = Some(ActiveServer {
        is_running,
        info: session_info.clone(),
        thread_handle: Some(thread_handle),
        tunnel_child,
    });

    Ok(session_info)
}

/// Коректно відправляє FIN клієнту і вичитує підтвердження (Graceful Teardown),
/// запобігаючи надсиланню ядром Linux пакетів TCP RST до Android Chrome.
fn graceful_close(stream: &mut std::net::TcpStream) {
    let _ = stream.flush();
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
    let mut drain = [0u8; 1024];
    while let Ok(n) = stream.read(&mut drain) {
        if n == 0 {
            break;
        }
    }
}

fn handle_client(
    mut stream: std::net::TcpStream,
    serve_path: &Path,
    file_name: &str,
    file_size: u64,
    is_dir: bool,
    landing_html: &str,
    app_handle: &AppHandle,
    session_info: &ShareSessionInfo,
) {
    let client_addr = stream
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(60)));

    let mut reader = BufReader::new(&stream);
    let mut request_line = String::new();
    if let Err(e) = reader.read_line(&mut request_line) {
        eprintln!("[Wi-Fi Share HTTP] [from {}] Read error: {}", client_addr, e);
        return;
    }
    if request_line.trim().is_empty() {
        return;
    }

    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        eprintln!("[Wi-Fi Share HTTP] [from {}] Malformed line: {:?}", client_addr, request_line);
        return;
    }

    let method = parts[0];
    let raw_uri = parts[1];

    // Нормалізація URI: підтримка абсолютних URI від Android Chrome (наприклад "http://10.10.231.85:1422/") та query-параметрів
    let path_no_scheme = if let Some(stripped) = raw_uri
        .strip_prefix("http://")
        .or_else(|| raw_uri.strip_prefix("https://"))
    {
        if let Some(slash_idx) = stripped.find('/') {
            &stripped[slash_idx..]
        } else {
            "/"
        }
    } else {
        raw_uri
    };

    let clean_path = path_no_scheme.split('?').next().unwrap_or("/");
    let path = if clean_path.is_empty() { "/" } else { clean_path };

    // Обов'язково вичитуємо всі HTTP-заголовки клієнта до пустого рядка (\r\n\r\n)
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                if line.trim().is_empty() {
                    break;
                }
            }
            Err(e) => {
                eprintln!("[Wi-Fi Share HTTP] [from {}] Header read error: {}", client_addr, e);
                break;
            }
        }
    }

    // 1. Обробка CORS / Private Network Access preflight запитів від Android Chrome
    if method == "OPTIONS" {
        eprintln!("[Wi-Fi Share HTTP] [from {}] OPTIONS {} -> 204 No Content (PNA Preflight)", client_addr, path);
        let headers = "HTTP/1.1 204 No Content\r\n\
                       Access-Control-Allow-Origin: *\r\n\
                       Access-Control-Allow-Methods: GET, HEAD, OPTIONS\r\n\
                       Access-Control-Allow-Headers: *\r\n\
                       Access-Control-Allow-Private-Network: true\r\n\
                       Access-Control-Max-Age: 86400\r\n\
                       Content-Length: 0\r\n\
                       Connection: close\r\n\r\n";
        let _ = stream.write_all(headers.as_bytes());
        graceful_close(&mut stream);
        return;
    }

    let is_head = method == "HEAD";

    if path == "/" {
        eprintln!("[Wi-Fi Share HTTP] [from {}] {} / -> 200 OK (Landing Page)", client_addr, method);
        let body = landing_html.as_bytes();
        let headers = format!(
            "HTTP/1.1 200 OK\r\n\
             Content-Type: text/html; charset=utf-8\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\
             Access-Control-Allow-Origin: *\r\n\
             Access-Control-Allow-Private-Network: true\r\n\
             Cache-Control: no-cache, no-store, must-revalidate\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(headers.as_bytes());
        if !is_head {
            let _ = stream.write_all(body);
        }
        graceful_close(&mut stream);
    } else if path == "/favicon.ico" {
        eprintln!("[Wi-Fi Share HTTP] [from {}] {} /favicon.ico -> 204 No Content", client_addr, method);
        let headers = "HTTP/1.1 204 No Content\r\n\
                       Content-Length: 0\r\n\
                       Connection: close\r\n\
                       Access-Control-Allow-Origin: *\r\n\
                       Access-Control-Allow-Private-Network: true\r\n\r\n";
        let _ = stream.write_all(headers.as_bytes());
        graceful_close(&mut stream);
    } else if path.starts_with("/preview") {
        eprintln!("[Wi-Fi Share HTTP] [from {}] {} /preview -> 200 OK", client_addr, method);
        let mime = get_mime_type(serve_path);
        let headers = format!(
            "HTTP/1.1 200 OK\r\n\
             Content-Type: {}\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\
             Accept-Ranges: bytes\r\n\
             Access-Control-Allow-Origin: *\r\n\
             Access-Control-Allow-Private-Network: true\r\n\r\n",
            mime, file_size
        );
        let _ = stream.write_all(headers.as_bytes());
        if !is_head {
            if let Ok(mut f) = File::open(serve_path) {
                let mut buffer = [0u8; 64 * 1024];
                while let Ok(n) = f.read(&mut buffer) {
                    if n == 0 || stream.write_all(&buffer[..n]).is_err() {
                        break;
                    }
                }
            }
        }
        graceful_close(&mut stream);
    } else if path.starts_with("/download") {
        eprintln!("[Wi-Fi Share HTTP] [from {}] {} /download -> 200 OK (Serving file)", client_addr, method);
        let download_name = if is_dir {
            format!("{}.zip", file_name)
        } else {
            file_name.to_string()
        };

        let ascii_name: String = download_name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' { c } else { '_' })
            .collect();
        let rfc5987_name = encode_rfc5987(&download_name);

        let _ = app_handle.emit("share_download_started", session_info);

        let headers = format!(
            "HTTP/1.1 200 OK\r\n\
             Content-Type: application/octet-stream\r\n\
             Content-Disposition: attachment; filename=\"{}\"; filename*=UTF-8''{}\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\
             Accept-Ranges: bytes\r\n\
             Access-Control-Allow-Origin: *\r\n\
             Access-Control-Allow-Private-Network: true\r\n\r\n",
            ascii_name, rfc5987_name, file_size
        );
        let _ = stream.write_all(headers.as_bytes());
        let mut transferred: u64 = 0;
        if !is_head {
            if let Ok(mut f) = File::open(serve_path) {
                let mut buffer = [0u8; 64 * 1024];
                while let Ok(n) = f.read(&mut buffer) {
                    if n == 0 || stream.write_all(&buffer[..n]).is_err() {
                        break;
                    }
                    transferred += n as u64;
                }
            }
        }
        graceful_close(&mut stream);
        if transferred >= file_size {
            let _ = app_handle.emit("share_download_completed", session_info);
        }
    } else {
        eprintln!("[Wi-Fi Share HTTP] [from {}] {} {} -> 404 Not Found", client_addr, method, path);
        let not_found = "HTTP/1.1 404 Not Found\r\n\
                         Content-Length: 0\r\n\
                         Connection: close\r\n\r\n";
        let _ = stream.write_all(not_found.as_bytes());
        graceful_close(&mut stream);
    }
}

/// Внутрішня зупинка сервера роздачі та видалення тимчасового ZIP-архіву
fn stop_server_internal() {
    let mut lock = SERVER_STATE.lock().unwrap();
    if let Some(mut server) = lock.take() {
        server.is_running.store(false, Ordering::SeqCst);
        if let Some(mut child) = server.tunnel_child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(handle) = server.thread_handle.take() {
            let _ = handle.join();
        }
        if let Some(ref tmp_zip) = server.info.temp_zip_path {
            let _ = fs::remove_file(tmp_zip);
        }
    }
}

/// Зупиняє активний сервер роздачі та закриває вікно роздачі
#[tauri::command]
pub async fn stop_file_share(app: AppHandle) -> Result<(), String> {
    stop_server_internal();
    if let Some(win) = app.get_webview_window("share") {
        let _ = win.close();
    }
    Ok(())
}

/// Отримує поточний статус роздачі
#[tauri::command]
pub async fn get_share_status() -> Result<Option<ShareSessionInfo>, String> {
    let lock = SERVER_STATE.lock().unwrap();
    Ok(lock.as_ref().map(|s| s.info.clone()))
}

/// Відкриває окреме нативне вікно Wi-Fi Share
#[tauri::command]
pub async fn open_share_window(
    app: AppHandle,
    file_path: Option<String>,
) -> Result<(), String> {
    let mut url = "index.html?view=share".to_string();
    if let Some(p) = file_path {
        url = format!("{}&file={}", url, crate::disk::urlencoding(&p));
    }

    open_or_focus_window(
        &app,
        "share",
        "Wi-Fi Quick Share — Роздача файлу",
        &url,
        420.0,
        540.0,
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_local_ip() {
        let ip = get_local_ip();
        assert!(!ip.is_empty());
        assert!(!ip.starts_with("0.0.0.0"));
    }

    #[test]
    fn test_format_file_size() {
        assert_eq!(format_file_size(500), "500 B");
        assert_eq!(format_file_size(1024), "1.0 KB");
        assert_eq!(format_file_size(1024 * 1024 * 5), "5.0 MB");
        assert_eq!(format_file_size(1024 * 1024 * 1024 * 2), "2.0 GB");
    }

    #[test]
    fn test_get_mime_type() {
        assert_eq!(get_mime_type(Path::new("video.mp4")), "video/mp4");
        assert_eq!(get_mime_type(Path::new("photo.png")), "image/png");
        assert_eq!(get_mime_type(Path::new("doc.pdf")), "application/pdf");
        assert_eq!(get_mime_type(Path::new("unknown.xyz")), "application/octet-stream");
    }

    #[test]
    fn test_encode_rfc5987() {
        assert_eq!(encode_rfc5987("test.pdf"), "test.pdf");
        assert_eq!(encode_rfc5987("файл.txt"), "%D1%84%D0%B0%D0%B9%D0%BB.txt");
        assert_eq!(encode_rfc5987("my file.zip"), "my%20file.zip");
    }

    #[test]
    fn test_generate_landing_html() {
        let html = generate_landing_html("test.mp4", "10.5 MB", false, true, "video/mp4");
        assert!(html.contains("Local Helper Wi-Fi Share"));
        assert!(html.contains("test.mp4"));
        assert!(html.contains("10.5 MB"));
        assert!(html.contains("href=\"/download\""));
        assert!(html.contains("<video controls src=\"/preview\"></video>"));
    }
}

