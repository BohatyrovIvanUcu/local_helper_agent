// Модуль вилучення та очищення вмісту веб-сторінок з активного браузера або вказаного URL

use std::process::Command;

/// Отримує чистий текст активної або вказаної веб-сторінки та метадані для створення саммарі.
#[tauri::command]
pub async fn summarize_webpage(
    url: Option<String>,
    focus: Option<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let clean_url = url.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or("");
        let clean_focus = focus.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or("");

        let py_script = r#"
import sys, os, glob, json, re, urllib.request, urllib.parse, ssl, subprocess, time, struct, sqlite3, shutil, tempfile

target_url = sys.argv[1].strip() if len(sys.argv) > 1 and sys.argv[1].strip() else None
focus_query = sys.argv[2].strip() if len(sys.argv) > 2 and sys.argv[2].strip() else None

def is_valid_web_url(u):
    if not u or not isinstance(u, str):
        return False
    u = u.strip()
    if not (u.startswith('http://') or u.startswith('https://')):
        return False
    lower = u.lower()
    ignore = (
        'chrome-extension://', 'chrome://', 'moz-extension://', 'about:',
        'devtools://', 'edge://', 'brave://', 'vivaldi://', 'opera://',
        'newtab', 'new-tab-page', 'favicon', 'google.com/search',
        'accounts.google.com/rotatecookiespage', 'ogs.google.com'
    )
    return not any(x in lower for x in ignore)

def decompress_mozlz4(data):
    if data[:8] != b'mozLz40\0':
        raise ValueError('Not mozLz4')
    try:
        import lz4.block
        return lz4.block.decompress(data[8:])
    except Exception:
        pass
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

def get_live_browser_info():
    ff_running = False
    ch_running = False
    try:
        out = subprocess.check_output(['ps', '-eo', 'comm,args'], stderr=subprocess.DEVNULL, timeout=1.2).decode('utf-8', errors='ignore')
        for line in out.splitlines():
            l = line.lower().strip()
            if not l or 'python3' in l:
                continue
            parts = l.split(None, 1)
            comm = parts[0] if parts else ''
            if any(k in comm for k in ('firefox', 'zen', 'librewolf', 'waterfox')) or '/firefox' in l or 'snap/firefox' in l:
                ff_running = True
            if any(k in comm for k in ('chrome', 'chromium', 'brave', 'msedge', 'vivaldi', 'opera')):
                if '--headless' not in l and '--type=' not in l:
                    ch_running = True
    except Exception:
        pass

    ff_titles = []
    ch_titles = []
    atspi_code = r'''
import json, gi
gi.require_version('Atspi', '2.0')
from gi.repository import Atspi
ff, ch = [], []
desktop = Atspi.get_desktop(0)
for i in range(desktop.get_child_count()):
    app = desktop.get_child_at_index(i)
    if not app:
        continue
    for j in range(app.get_child_count()):
        frame = app.get_child_at_index(j)
        if not frame:
            continue
        title = (frame.get_name() or '').strip()
        if not title:
            continue
        for suf in (' — Mozilla Firefox', ' - Mozilla Firefox', ' — Firefox', ' - Firefox', ' — Zen Browser', ' - Zen Browser'):
            if title.endswith(suf):
                t = title[:-len(suf)].strip()
                if t:
                    ff.append(t)
                break
        for suf in (' - Google Chrome', ' - Chromium', ' - Brave', ' - Microsoft Edge'):
            if title.endswith(suf):
                t = title[:-len(suf)].strip()
                if t:
                    ch.append(t)
                break
print(json.dumps({'ff': ff, 'ch': ch}))
'''
    try:
        raw = subprocess.check_output(['python3', '-c', atspi_code], stderr=subprocess.DEVNULL, timeout=1.0).decode('utf-8', errors='ignore').strip()
        if raw:
            parsed = json.loads(raw.splitlines()[-1])
            ff_titles = parsed.get('ff') or []
            ch_titles = parsed.get('ch') or []
    except Exception:
        pass

    return ff_running, ch_running, ff_titles, ch_titles

def query_sqlite_readonly(db_path, sql):
    if not os.path.exists(db_path):
        return []
    try:
        conn = sqlite3.connect(f'file:{db_path}?mode=ro', uri=True, timeout=0.8)
        rows = conn.execute(sql).fetchall()
        conn.close()
        return rows
    except Exception:
        pass
    tmpdir = tempfile.mkdtemp(prefix='lh_db_')
    try:
        base = os.path.basename(db_path)
        dst = os.path.join(tmpdir, base)
        shutil.copy2(db_path, dst)
        for ext in ('-wal', '-shm', '-journal'):
            if os.path.exists(db_path + ext):
                shutil.copy2(db_path + ext, dst + ext)
        conn = sqlite3.connect(dst, timeout=0.8)
        rows = conn.execute(sql).fetchall()
        conn.close()
        return rows
    except Exception:
        return []
    finally:
        shutil.rmtree(tmpdir, ignore_errors=True)

def title_matches(tab_title, live_titles):
    if not tab_title or not live_titles:
        return False
    tt = tab_title.strip().lower()
    if not tt:
        return False
    for lt in live_titles:
        lt_c = lt.strip().lower()
        if not lt_c:
            continue
        if tt == lt_c or tt.startswith(lt_c) or lt_c.startswith(tt):
            return True
        if len(tt) > 15 and (tt[:15] in lt_c or lt_c[:15] in tt):
            return True
    return False

def get_active_browser_tab():
    home = os.path.expanduser('~')
    ff_running, ch_running, ff_titles, ch_titles = get_live_browser_info()

    # 1. Спроба Firefox Sessionstore
    ff_paths = [
        os.path.join(home, '.mozilla', 'firefox'),
        os.path.join(home, 'snap', 'firefox', 'common', '.mozilla', 'firefox'),
        os.path.join(home, '.var', 'app', 'org.mozilla.firefox', '.mozilla', 'firefox'),
        os.path.join(home, '.zen'),
    ]
    ff_candidates = []
    for base in ff_paths:
        if not os.path.isdir(base):
            continue
        for root, dirs, files in os.walk(base):
            for f in files:
                if f in ('recovery.jsonlz4', 'previous.jsonlz4', 'recovery.js'):
                    fp = os.path.join(root, f)
                    try:
                        mtime = os.path.getmtime(fp)
                        ff_candidates.append((mtime, fp))
                    except Exception:
                        pass
    ff_candidates.sort(key=lambda x: x[0], reverse=True)

    matched_by_title = None
    most_recent_ff = None

    for mtime, fp in ff_candidates[:6]:
        try:
            with open(fp, 'rb') as f_obj:
                raw = f_obj.read()
            if raw.startswith(b'mozLz40\0'):
                data = json.loads(decompress_mozlz4(raw).decode('utf-8', errors='ignore'))
            else:
                data = json.loads(raw.decode('utf-8', errors='ignore'))
            windows = data.get('windows', [])
            for w in windows:
                tabs = w.get('tabs', [])
                sel_idx = w.get('selected', 1) - 1
                for idx, t in enumerate(tabs):
                    entries = t.get('entries', [])
                    if not entries:
                        continue
                    cur_idx = t.get('index', len(entries)) - 1
                    cur_idx = max(0, min(cur_idx, len(entries) - 1))
                    entry = entries[cur_idx]
                    u = entry.get('url', '')
                    t_title = entry.get('title', '')
                    if is_valid_web_url(u):
                        if ff_titles and title_matches(t_title, ff_titles):
                            matched_by_title = (u, t_title, 'firefox_live_window')
                            break
                        if idx == sel_idx and not most_recent_ff:
                            most_recent_ff = (u, t_title, 'firefox_session')
                if matched_by_title:
                    break
            if matched_by_title:
                break
        except Exception:
            pass

    if matched_by_title:
        return matched_by_title

    # 2. Спроба Chromium History
    ch_candidates = []
    patterns = [
        os.path.join(home, '.config', 'google-chrome', '*', 'History'),
        os.path.join(home, '.config', 'chromium', '*', 'History'),
        os.path.join(home, '.config', 'BraveSoftware', 'Brave-Browser', '*', 'History'),
        os.path.join(home, 'snap', 'chromium', 'common', 'chromium', '*', 'History'),
    ]
    for pat in patterns:
        for hp in glob.glob(pat):
            try:
                mtime = os.path.getmtime(hp)
                ch_candidates.append((mtime, hp))
            except Exception:
                pass
    ch_candidates.sort(key=lambda x: x[0], reverse=True)

    matched_ch_title = None
    most_recent_ch = None

    for mtime, hp in ch_candidates[:4]:
        rows = query_sqlite_readonly(
            hp,
            "SELECT url, title, last_visit_time FROM urls WHERE url LIKE 'http%' ORDER BY last_visit_time DESC LIMIT 40"
        )
        for u, t, _ in rows:
            if is_valid_web_url(u):
                if ch_titles and title_matches(t, ch_titles):
                    matched_ch_title = (u, t, 'chrome_live_window')
                    break
                if not most_recent_ch:
                    most_recent_ch = (u, t, 'chrome_history')
        if matched_ch_title:
            break

    if matched_ch_title:
        return matched_ch_title

    if ff_running and most_recent_ff:
        return most_recent_ff
    if ch_running and most_recent_ch:
        return most_recent_ch

    if most_recent_ff and most_recent_ch:
        return most_recent_ff
    if most_recent_ff:
        return most_recent_ff
    if most_recent_ch:
        return most_recent_ch

    # 3. Фолбек на буфер обміну
    try:
        clip_out = subprocess.check_output(['xclip', '-o', '-selection', 'clipboard'], stderr=subprocess.DEVNULL, timeout=0.8).decode('utf-8', errors='ignore').strip()
        if is_valid_web_url(clip_out):
            return (clip_out, '', 'clipboard')
    except Exception:
        pass

    return (None, None, 'not_found')

final_url = target_url
page_title = None
detect_source = 'explicit'

if not final_url:
    detected_url, detected_title, detect_source = get_active_browser_tab()
    final_url = detected_url
    page_title = detected_title

if not final_url or not is_valid_web_url(final_url):
    print(json.dumps({'error': 'Не вдалося знайти активну вкладку або коректний URL.'}))
    sys.exit(0)

# Отримання та екстракція контенту
ctx = ssl.create_default_context()
ctx.check_hostname = False
ctx.verify_mode = ssl.CERT_NONE

req = urllib.request.Request(
    final_url,
    headers={
        'User-Agent': 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36',
        'Accept': 'text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8',
        'Accept-Language': 'uk-UA,uk;q=0.9,en-US;q=0.8,en;q=0.7',
    }
)

html_text = ""
try:
    with urllib.request.urlopen(req, context=ctx, timeout=7.0) as resp:
        charset = resp.headers.get_content_charset() or 'utf-8'
        raw = resp.read(1_500_000)
        html_text = raw.decode(charset, errors='replace')
except Exception as e:
    print(json.dumps({'error': f'Помилка завантаження сторінки {final_url}: {str(e)}'}))
    sys.exit(0)

if not page_title:
    m = re.search(r'<title[^>]*>(.*?)</title>', html_text, re.IGNORECASE | re.DOTALL)
    if m:
        page_title = re.sub(r'\s+', ' ', m.group(1)).strip()

def clean_html_to_markdown(html):
    h = re.sub(r'<!--.*?-->', '', html, flags=re.DOTALL)
    h = re.sub(r'<(script|style|svg|noscript|header|footer|nav|aside|form|iframe)[^>]*>.*?</\1>', '', h, flags=re.DOTALL | re.IGNORECASE)
    
    article_m = re.search(r'<(article|main)[^>]*>(.*?)</\1>', h, flags=re.DOTALL | re.IGNORECASE)
    if article_m and len(article_m.group(2).strip()) > 500:
        h = article_m.group(2)

    h = re.sub(r'<h1[^>]*>(.*?)</h1>', r'\n\n# \1\n\n', h, flags=re.DOTALL | re.IGNORECASE)
    h = re.sub(r'<h2[^>]*>(.*?)</h2>', r'\n\n## \1\n\n', h, flags=re.DOTALL | re.IGNORECASE)
    h = re.sub(r'<h3[^>]*>(.*?)</h3>', r'\n\n### \1\n\n', h, flags=re.DOTALL | re.IGNORECASE)
    h = re.sub(r'<li[^>]*>(.*?)</li>', r'\n* \1', h, flags=re.DOTALL | re.IGNORECASE)
    h = re.sub(r'<p[^>]*>(.*?)</p>', r'\n\n\1\n\n', h, flags=re.DOTALL | re.IGNORECASE)
    h = re.sub(r'<br\s*/?>', r'\n', h, flags=re.IGNORECASE)
    h = re.sub(r'<[^>]+>', ' ', h)

    # HTML сутності
    h = h.replace('&nbsp;', ' ').replace('&amp;', '&').replace('&lt;', '<').replace('&gt;', '>').replace('&quot;', '"').replace('&#39;', "'")

    lines = [line.strip() for line in h.splitlines()]
    clean_lines = []
    prev_blank = False
    for l in lines:
        if not l:
            if not prev_blank:
                clean_lines.append('')
                prev_blank = True
        else:
            clean_lines.append(l)
            prev_blank = False
    
    res = '\n'.join(clean_lines).strip()
    return res

text_content = clean_html_to_markdown(html_text)
if len(text_content) > 14_000:
    text_content = text_content[:14_000] + "\n\n... [Контент обрізано до 14 000 символів]"

print(json.dumps({
    'url': final_url,
    'title': page_title or final_url,
    'source': detect_source,
    'content': text_content
}))
"#;

        let output = Command::new("python3")
            .args(["-c", py_script, clean_url, clean_focus])
            .output()
            .map_err(|e| format!("Не вдалося запустити парсер сторінки: {}", e))?;

        if !output.status.success() {
            let err_msg = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Помилка екстрактора: {}", err_msg));
        }

        let raw_stdout = String::from_utf8_lossy(&output.stdout);
        let parsed: serde_json::Value = serde_json::from_str(raw_stdout.trim())
            .map_err(|e| format!("Некоректна відповідь екстрактора: {}", e))?;

        if let Some(err) = parsed.get("error").and_then(|e| e.as_str()) {
            return Err(err.to_string());
        }

        let found_url = parsed.get("url").and_then(|u| u.as_str()).unwrap_or(clean_url);
        let title = parsed.get("title").and_then(|t| t.as_str()).unwrap_or("");
        let content = parsed.get("content").and_then(|c| c.as_str()).unwrap_or("");

        let mut res = format!("Джерело: {}\nЗаголовок: {}\n", found_url, title);
        if let Some(f) = focus {
            if !f.trim().is_empty() {
                res.push_str(&format!("Фокус користувача: {}\n", f));
            }
        }
        res.push_str(&format!(
            "\n<untrusted_web_content source=\"{}\">\n{}\n</untrusted_web_content>",
            found_url, content
        ));

        Ok(res)
    })
    .await
    .map_err(|e| format!("Помилка фонового потоку: {}", e))?
}

/// Завантажує зображення за вказаним веб-URL та повертає його Base64 вміст і MIME-тип.
#[tauri::command]
pub async fn fetch_image_from_url(url: String) -> Result<crate::clipboard::ClipboardImageResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let clean_url = url.trim();
        if !clean_url.starts_with("http://") && !clean_url.starts_with("https://") {
            return Err("Некоректний URL: очікується протокол http або https".to_string());
        }

        // Обчислюємо Referer для обходу hotlink-захисту (YouTube, Google, AliExpress тощо)
        let referer = if let Some(idx) = clean_url.get(8..).and_then(|s| s.find('/')) {
            format!("{}/", &clean_url[..8 + idx])
        } else {
            clean_url.to_string()
        };

        // 1. Спроба завантаження через curl
        let curl_output = Command::new("curl")
            .args([
                "-sSLf",
                "--max-time", "15",
                "-H", "User-Agent: Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36",
                "-H", "Accept: image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8",
                "-H", "Sec-Fetch-Dest: image",
                "-H", "Sec-Fetch-Mode: no-cors",
                "-e", &referer,
                "-w", "\n__MIME__:%{content_type}",
                clean_url,
            ])
            .output();

        let mut data_bytes: Vec<u8> = Vec::new();
        let mut mime_str = String::new();

        if let Ok(out) = curl_output {
            if out.status.success() && !out.stdout.is_empty() {
                let stdout = out.stdout;
                let marker = b"\n__MIME__:";
                if let Some(pos) = stdout.windows(marker.len()).rposition(|w| w == marker) {
                    data_bytes = stdout[..pos].to_vec();
                    mime_str = String::from_utf8_lossy(&stdout[pos + marker.len()..]).trim().to_string();
                } else {
                    data_bytes = stdout;
                }
            }
        }

        // 2. Фолбек через Python urllib, якщо curl повернув порожні дані
        if data_bytes.is_empty() {
            let py_downloader = r#"
import sys, urllib.request, urllib.parse, ssl

url = sys.argv[1].strip()
parsed = urllib.parse.urlparse(url)
referer = f"{parsed.scheme}://{parsed.netloc}/"

ctx = ssl.create_default_context()
ctx.check_hostname = False
ctx.verify_mode = ssl.CERT_NONE

req = urllib.request.Request(
    url,
    headers={
        'User-Agent': 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36',
        'Accept': 'image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8',
        'Sec-Fetch-Dest': 'image',
        'Sec-Fetch-Mode': 'no-cors',
        'Referer': referer
    }
)
try:
    with urllib.request.urlopen(req, context=ctx, timeout=12.0) as resp:
        b = resp.read(25_000_000)
        ct = resp.headers.get_content_type() or 'image/png'
        sys.stdout.buffer.write(b)
        sys.stdout.buffer.write(b'\n__MIME__:' + ct.encode('utf-8'))
except Exception:
    sys.exit(1)
"#;
            if let Ok(py_out) = Command::new("python3").args(["-c", py_downloader, clean_url]).output() {
                if py_out.status.success() && !py_out.stdout.is_empty() {
                    let stdout = py_out.stdout;
                    let marker = b"\n__MIME__:";
                    if let Some(pos) = stdout.windows(marker.len()).rposition(|w| w == marker) {
                        data_bytes = stdout[..pos].to_vec();
                        mime_str = String::from_utf8_lossy(&stdout[pos + marker.len()..]).trim().to_string();
                    } else {
                        data_bytes = stdout;
                    }
                }
            }
        }

        if data_bytes.is_empty() {
            return Err("Не вдалося завантажити зображення: ресурс недоступний або порожній".to_string());
        }

        // Перевіряємо, чи це не HTML сторінка з помилкою
        let is_html = data_bytes.starts_with(b"<!DOCTYPE") || data_bytes.starts_with(b"<html") || data_bytes.starts_with(b"<HTML");
        if is_html {
            return Err("За вказаним посиланням отримано веб-сторінку замість зображення".to_string());
        }

        // 3. Визначення та валідація MIME-типу
        let detected_mime = if mime_str.to_lowercase().starts_with("image/") {
            mime_str.split(';').next().unwrap_or("image/jpeg").trim().to_lowercase()
        } else if data_bytes.starts_with(b"\x89PNG") {
            "image/png".to_string()
        } else if data_bytes.starts_with(b"\xff\xd8\xff") {
            "image/jpeg".to_string()
        } else if data_bytes.starts_with(b"GIF8") {
            "image/gif".to_string()
        } else if data_bytes.starts_with(b"RIFF") && data_bytes.len() >= 12 && &data_bytes[8..12] == b"WEBP" {
            "image/webp".to_string()
        } else if data_bytes.len() >= 12 && &data_bytes[4..8] == b"ftyp" {
            "image/avif".to_string()
        } else if data_bytes.starts_with(b"BM") {
            "image/bmp".to_string()
        } else if data_bytes.starts_with(b"<?xml") || data_bytes.windows(4).any(|w| w == b"<svg") {
            "image/svg+xml".to_string()
        } else {
            let lower_url = clean_url.to_lowercase();
            if lower_url.contains(".png") {
                "image/png".to_string()
            } else if lower_url.contains(".webp") {
                "image/webp".to_string()
            } else if lower_url.contains(".gif") {
                "image/gif".to_string()
            } else if lower_url.contains(".svg") {
                "image/svg+xml".to_string()
            } else if lower_url.contains(".avif") {
                "image/avif".to_string()
            } else {
                "image/jpeg".to_string()
            }
        };

        // 4. Формування назви файлу
        let path_part = clean_url.split('?').next().unwrap_or(clean_url);
        let raw_filename = path_part.split('/').last().unwrap_or("image.png");
        let name = if raw_filename.is_empty() || !raw_filename.contains('.') {
            match detected_mime.as_str() {
                "image/jpeg" => "image.jpg",
                "image/webp" => "image.webp",
                "image/gif" => "image.gif",
                "image/svg+xml" => "image.svg",
                "image/avif" => "image.avif",
                _ => "image.png",
            }
            .to_string()
        } else {
            raw_filename.to_string()
        };

        Ok(crate::clipboard::ClipboardImageResult {
            mime_type: detected_mime,
            base64_data: crate::voice::encode_base64(&data_bytes),
            name,
        })
    })
    .await
    .map_err(|e| format!("Помилка фонового потоку: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_image_from_url_invalid_protocol() {
        tauri::async_runtime::block_on(async {
            let res = fetch_image_from_url("ftp://example.com/test.png".to_string()).await;
            assert!(res.is_err());
            assert!(res.unwrap_err().contains("очікується протокол http або https"));
        });
    }

    #[test]
    fn test_fetch_image_from_url_empty() {
        tauri::async_runtime::block_on(async {
            let res = fetch_image_from_url("   ".to_string()).await;
            assert!(res.is_err());
        });
    }
}


