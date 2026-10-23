// Модуль керування мультимедіа: MPRIS (D-Bus), YouTube Music, системна гучність (audio.rs)

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use tauri::Emitter;
use crate::audio;

pub static LAST_KNOWN_MUSIC_PLAYING: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "linux")]
pub fn run_python_mpris_cmd(py_code: &str) -> Option<std::process::ExitStatus> {
    if Path::new("/usr/bin/aa-exec").exists() {
        if let Ok(status) = Command::new("/usr/bin/aa-exec")
            .args(["-p", "unconfined", "python3", "-c", py_code])
            .stdin(Stdio::null())
            .status()
        {
            return Some(status);
        }
    }
    Command::new("python3")
        .args(["-c", py_code])
        .stdin(Stdio::null())
        .status()
        .ok()
}

pub fn resume_media_player() -> bool {
    #[cfg(target_os = "linux")]
    {
        let py_code = r#"
import sys, dbus
try:
    bus = dbus.SessionBus()
    players = [s for s in bus.list_names() if s.startswith('org.mpris.MediaPlayer2.')]
    if not players:
        sys.exit(1)
    resumed = False
    for p in players:
        try:
            player_obj = bus.get_object(p, '/org/mpris/MediaPlayer2', introspect=False)
            iface = dbus.Interface(player_obj, 'org.mpris.MediaPlayer2.Player')
            try:
                iface.Play()
                resumed = True
            except Exception:
                try:
                    iface.PlayPause()
                    resumed = True
                except Exception:
                    pass
        except Exception:
            pass
    sys.exit(0 if resumed else 1)
except Exception:
    sys.exit(1)
"#;
        if let Some(status) = run_python_mpris_cmd(py_code) {
            if status.success() {
                return true;
            }
        }
        return false;
    }

    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("powershell")
            .args(["-Command", "$w = New-Object -ComObject WScript.Shell; $w.SendKeys([char]179)"])
            .status();
        return true;
    }

    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("osascript")
            .args(["-e", "tell application \"Spotify\" to play"])
            .status();
        let _ = Command::new("osascript")
            .args(["-e", "tell application \"Music\" to play"])
            .status();
        return true;
    }

    #[allow(unreachable_code)]
    false
}

/// Якщо нова вкладка YouTube Music відкрилась зі спливаючим блокуванням "Почати відтворення",
/// MPRIS-сесія ще не активна до першого кліку/натискання клавіші у вкладці.
/// Ця функція перевіряє через D-Bus, чи плеєр ще не грає, і надсилає апаратне натискання Space
/// через ядерний інтерфейс /dev/uinput (працює у Wayland для Firefox та Chrome) або XTest.
pub fn kickstart_browser_playback() -> bool {
    if resume_media_player() {
        return true;
    }

    #[cfg(target_os = "linux")]
    {
        let py_kick = r#"
import sys, os, time, fcntl, struct, subprocess, ctypes, ctypes.util

def is_mpris_playing():
    try:
        import dbus
        bus = dbus.SessionBus()
        for p in bus.list_names():
            if p.startswith('org.mpris.MediaPlayer2.'):
                try:
                    obj = bus.get_object(p, '/org/mpris/MediaPlayer2')
                    props = dbus.Interface(obj, 'org.freedesktop.DBus.Properties')
                    if str(props.Get('org.mpris.MediaPlayer2.Player', 'PlaybackStatus')) == 'Playing':
                        return True
                except Exception:
                    pass
    except Exception:
        pass
    return False

if is_mpris_playing():
    sys.exit(0)

# 1. Нативний Wayland / X11 метод через /dev/uinput (працює для Mozilla Firefox та Chrome у Wayland)
try:
    fd = os.open("/dev/uinput", os.O_WRONLY | os.O_NONBLOCK)
    UI_SET_EVBIT = 0x40045564
    UI_SET_KEYBIT = 0x40045565
    UI_DEV_CREATE = 0x5501
    UI_DEV_DESTROY = 0x5502
    EV_SYN = 0x00
    EV_KEY = 0x01
    KEY_SPACE = 57

    fcntl.ioctl(fd, UI_SET_EVBIT, EV_KEY)
    fcntl.ioctl(fd, UI_SET_EVBIT, EV_SYN)
    fcntl.ioctl(fd, UI_SET_KEYBIT, KEY_SPACE)

    udev = struct.pack("80sHHHHI", b"local-helper-media-kbd", 0x03, 0x1234, 0x5678, 1, 0) + b"\x00" * (64 * 4 * 4)
    os.write(fd, udev)
    fcntl.ioctl(fd, UI_DEV_CREATE)
    time.sleep(0.3)

    if not is_mpris_playing():
        os.write(fd, struct.pack("llHHi", 0, 0, EV_KEY, KEY_SPACE, 1))
        os.write(fd, struct.pack("llHHi", 0, 0, EV_SYN, 0, 0))
        time.sleep(0.05)
        os.write(fd, struct.pack("llHHi", 0, 0, EV_KEY, KEY_SPACE, 0))
        os.write(fd, struct.pack("llHHi", 0, 0, EV_SYN, 0, 0))
        time.sleep(0.1)

    fcntl.ioctl(fd, UI_DEV_DESTROY)
    os.close(fd)
    time.sleep(0.6)
    if is_mpris_playing():
        sys.exit(0)
    sys.exit(1)
except Exception:
    pass

# 2. Фолбек через X11 XTest (якщо /dev/uinput недоступний)
try:
    active_out = subprocess.check_output(["xprop", "-root", "_NET_ACTIVE_WINDOW"], timeout=2).decode("utf-8", errors="ignore")
    parts = active_out.strip().split()
    win_id = parts[-1] if parts else ""
    if not win_id.startswith("0x") or win_id == "0x0":
        sys.exit(1)
    win_info = subprocess.check_output(["xprop", "-id", win_id, "_NET_WM_NAME", "WM_NAME", "WM_CLASS"], timeout=2).decode("utf-8", errors="ignore").lower()
    if not any(k in win_info for k in ("youtube music", "chrome", "chromium", "brave", "opera", "firefox")):
        sys.exit(1)

    x11_path = ctypes.util.find_library("X11")
    xtst_path = ctypes.util.find_library("Xtst")
    if not x11_path or not xtst_path:
        sys.exit(1)

    x11 = ctypes.cdll.LoadLibrary(x11_path)
    xtst = ctypes.cdll.LoadLibrary(xtst_path)

    x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
    x11.XOpenDisplay.restype = ctypes.c_void_p
    x11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
    x11.XKeysymToKeycode.restype = ctypes.c_ubyte
    xtst.XTestFakeKeyEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
    xtst.XTestFakeKeyEvent.restype = ctypes.c_int
    x11.XFlush.argtypes = [ctypes.c_void_p]
    x11.XFlush.restype = ctypes.c_int
    x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
    x11.XCloseDisplay.restype = ctypes.c_int

    dpy = x11.XOpenDisplay(None)
    if not dpy:
        sys.exit(1)

    keycode = x11.XKeysymToKeycode(dpy, 0x0020)
    if keycode != 0:
        xtst.XTestFakeKeyEvent(dpy, keycode, 1, 0)
        x11.XFlush(dpy)
        time.sleep(0.05)
        xtst.XTestFakeKeyEvent(dpy, keycode, 0, 0)
        x11.XFlush(dpy)

    x11.XCloseDisplay(dpy)
    time.sleep(0.35)
    sys.exit(0)
except Exception:
    sys.exit(1)
"#;
        if let Ok(status) = Command::new("python3").args(["-c", py_kick]).status() {
            if status.success() {
                return true;
            }
        }
    }

    false
}

pub fn pause_media_player() -> bool {
    #[cfg(target_os = "linux")]
    {
        let py_code = r#"
import sys, dbus
try:
    bus = dbus.SessionBus()
    players = [s for s in bus.list_names() if s.startswith('org.mpris.MediaPlayer2.')]
    if not players:
        sys.exit(1)
    paused = False
    for p in players:
        try:
            player_obj = bus.get_object(p, '/org/mpris/MediaPlayer2', introspect=False)
            iface = dbus.Interface(player_obj, 'org.mpris.MediaPlayer2.Player')
            try:
                iface.Pause()
                paused = True
            except Exception:
                try:
                    iface.PlayPause()
                    paused = True
                except Exception:
                    pass
        except Exception:
            pass
    sys.exit(0 if paused else 1)
except Exception:
    sys.exit(1)
"#;
        if let Some(status) = run_python_mpris_cmd(py_code) {
            if status.success() {
                return true;
            }
        }
        return false;
    }

    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("powershell")
            .args(["-Command", "$w = New-Object -ComObject WScript.Shell; $w.SendKeys([char]179)"])
            .status();
        return true;
    }

    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("osascript")
            .args(["-e", "tell application \"Spotify\" to pause"])
            .status();
        let _ = Command::new("osascript")
            .args(["-e", "tell application \"Music\" to pause"])
            .status();
        return true;
    }

    #[allow(unreachable_code)]
    false
}

pub fn next_media_player() -> bool {
    #[cfg(target_os = "linux")]
    {
        let py_code = r#"
import sys, dbus
try:
    bus = dbus.SessionBus()
    players = [s for s in bus.list_names() if s.startswith('org.mpris.MediaPlayer2.')]
    if not players:
        sys.exit(1)
    success = False
    for p in players:
        try:
            player_obj = bus.get_object(p, '/org/mpris/MediaPlayer2', introspect=False)
            iface = dbus.Interface(player_obj, 'org.mpris.MediaPlayer2.Player')
            try:
                iface.Next()
                success = True
            except Exception:
                pass
        except Exception:
            pass
    sys.exit(0 if success else 1)
except Exception:
    sys.exit(1)
"#;
        if let Some(status) = run_python_mpris_cmd(py_code) {
            if status.success() {
                return true;
            }
        }
        return false;
    }

    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("powershell")
            .args(["-Command", "$w = New-Object -ComObject WScript.Shell; $w.SendKeys([char]176)"])
            .status();
        return true;
    }

    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("osascript")
            .args(["-e", "tell application \"Spotify\" to next track"])
            .status();
        let _ = Command::new("osascript")
            .args(["-e", "tell application \"Music\" to next track"])
            .status();
        return true;
    }

    #[allow(unreachable_code)]
    false
}

pub fn prev_media_player() -> bool {
    #[cfg(target_os = "linux")]
    {
        let py_code = r#"
import sys, dbus
try:
    bus = dbus.SessionBus()
    players = [s for s in bus.list_names() if s.startswith('org.mpris.MediaPlayer2.')]
    if not players:
        sys.exit(1)
    success = False
    for p in players:
        try:
            player_obj = bus.get_object(p, '/org/mpris/MediaPlayer2', introspect=False)
            iface = dbus.Interface(player_obj, 'org.mpris.MediaPlayer2.Player')
            try:
                iface.Previous()
                success = True
            except Exception:
                pass
        except Exception:
            pass
    sys.exit(0 if success else 1)
except Exception:
    sys.exit(1)
"#;
        if let Some(status) = run_python_mpris_cmd(py_code) {
            if status.success() {
                return true;
            }
        }
        return false;
    }

    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("powershell")
            .args(["-Command", "$w = New-Object -ComObject WScript.Shell; $w.SendKeys([char]177)"])
            .status();
        return true;
    }

    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("osascript")
            .args(["-e", "tell application \"Spotify\" to previous track"])
            .status();
        let _ = Command::new("osascript")
            .args(["-e", "tell application \"Music\" to previous track"])
            .status();
        return true;
    }

    #[allow(unreachable_code)]
    false
}

pub fn open_browser_detached(url: &str) {
    #[cfg(target_os = "windows")]
    let _ = Command::new("cmd")
        .args(["/C", "start", url])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();

    #[cfg(target_os = "macos")]
    let _ = Command::new("open")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();

    #[cfg(target_os = "linux")]
    {
        let spawned = Command::new("xdg-open")
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();

        let ok = match spawned {
            Ok(mut child) => {
                thread::sleep(Duration::from_millis(150));
                match child.try_wait() {
                    Ok(Some(status)) => status.success(),
                    Ok(None) => true,
                    Err(_) => false,
                }
            }
            Err(_) => false,
        };

        if !ok {
            let fallbacks = [
                "/snap/bin/firefox",
                "firefox",
                "sensible-browser",
                "google-chrome",
                "google-chrome-stable",
                "chromium",
                "chromium-browser",
                "brave-browser",
            ];
            for fb in fallbacks {
                if let Ok(mut c) = Command::new(fb)
                    .arg(url)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                {
                    thread::sleep(Duration::from_millis(100));
                    match c.try_wait() {
                        Ok(Some(status)) if !status.success() => continue,
                        _ => break,
                    }
                }
            }
        }
    }
}

pub fn is_liked_music_query(q: &str) -> bool {
    let lower = q.trim().to_lowercase();
    lower == "liked"
        || lower == "liked_music"
        || lower == "favorites"
        || lower == "my music"
        || lower == "my songs"
        || lower == "my tracks"
        || lower.contains("лайк")
        || lower.contains("улюблен")
        || lower.contains("люблю")
        || lower.contains("сподоба")
        || lower.contains("мої треки")
        || lower.contains("мої пісні")
        || lower.contains("моя музика")
        || lower.contains("мій плейлист")
        || lower.contains("мої аудіо")
        || lower.contains("те що я слухаю")
}

pub fn is_mix_music_query(q: &str) -> bool {
    let lower = q.trim().to_lowercase();
    lower == "mix"
        || lower == "my_mix"
        || lower.contains("мікс")
        || lower.contains("рекомендац")
        || lower.contains("підбірк")
}

pub fn open_and_play_query(query: &str) -> Result<String, String> {
    let clean = query.trim();

    // 1. Миттєве визначення URL для стандартних плейлистів без спавну сторонніх процесів
    let (play_url, reply_msg) = if clean.is_empty() || is_liked_music_query(clean) {
        (
            "https://music.youtube.com/watch?list=LM&index=1".to_string(),
            "Вмикаю твій плейлист 'Музика, яка сподобалась' у YouTube Music 🎵".to_string(),
        )
    } else if is_mix_music_query(clean) {
        (
            "https://music.youtube.com/watch?list=RDMM".to_string(),
            "Вмикаю твій персональний мікс рекомендацій у YouTube Music 🎵".to_string(),
        )
    } else if clean.starts_with("http://") || clean.starts_with("https://") {
        let mut url = clean.to_string();
        if url.contains("playlist?list=") {
            url = url.replace("playlist?list=", "watch?list=");
        }
        if url.contains("watch?list=") && !url.contains("&index=") && !url.contains("&v=") {
            url.push_str("&index=1");
        }
        (url, "Відкриваю вказане медіа-посилання 🎵".to_string())
    } else {
        // Довільний запит (назва пісні / виконавця) — швидко знаходимо прямий video id без відкриття браузера
        let py_search = r#"
import sys, urllib.request, urllib.parse, re

raw = sys.argv[1].strip() if len(sys.argv) > 1 else ""
play_url = None
if raw:
    try:
        q = urllib.parse.quote_plus(raw)
        url = f"https://www.youtube.com/results?search_query={q}"
        req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36"})
        html = urllib.request.urlopen(req, timeout=3).read().decode('utf-8', errors='ignore')
        matches = re.findall(r"watch\?v=([a-zA-Z0-9_-]{11})", html)
        if matches:
            play_url = f"https://music.youtube.com/watch?v={matches[0]}"
    except Exception:
        pass

if not play_url:
    play_url = f"https://music.youtube.com/search?q={urllib.parse.quote_plus(raw)}"

print("URL:" + play_url)
"#;
        let found_url = if let Ok(out) = Command::new("python3")
            .args(["-c", py_search, clean])
            .stdin(Stdio::null())
            .output()
        {
            let s = String::from_utf8_lossy(&out.stdout);
            s.lines()
                .find(|l| l.starts_with("URL:"))
                .map(|l| l.trim_start_matches("URL:").trim().to_string())
        } else {
            None
        };

        let final_url = found_url.unwrap_or_else(|| {
            format!("https://music.youtube.com/search?q={}", clean)
        });

        (final_url, format!("Увімкнено відтворення музики ('{}') 🎵", clean))
    };

    // 2. Запускаємо браузер повністю від'єднаним від Rust, щоб жодні дескриптори stdout/stderr не утримували виклик
    open_browser_detached(&play_url);

    // 3. У фоновому потоці через uinput та MPRIS знімаємо паузу/автоплей, якщо браузер заблокував старт
    std::thread::spawn(move || {
        for delay in [1200_u64, 2200, 3600] {
            thread::sleep(Duration::from_millis(delay));
            if kickstart_browser_playback() {
                break;
            }
        }
    });

    Ok(reply_msg)
}

pub fn is_generic_music_query(q: &str) -> bool {
    let lower = q.trim().to_lowercase();
    q.trim().is_empty()
        || is_liked_music_query(q)
        || is_mix_music_query(q)
        || matches!(
            lower.as_str(),
            "музика"
                | "музику"
                | "музики"
                | "музичко"
                | "music"
                | "track"
                | "трек"
                | "пісня"
                | "пісню"
                | "song"
                | "плей"
                | "play"
        )
}

/// Керує відтворенням медіа: натискає Play/Pause на активному плеєрі або запускає прямий трек.
#[tauri::command]
pub async fn control_media(
    app: tauri::AppHandle,
    action: String,
    query: Option<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let action_clean = action.trim().to_lowercase();
        let query_clean = query.as_deref().map(str::trim).filter(|s| !s.is_empty());

        if let Some(q) = query_clean {
            let is_generic = is_generic_music_query(q);
            // Якщо запитано відтворення улюблених треків або загальну музику, спочатку пробуємо відновити вже відкритий плеєр на паузі
            if (action_clean == "play" || action_clean == "open_and_play" || action_clean == "play_query")
                && is_generic
            {
                if resume_media_player() {
                    LAST_KNOWN_MUSIC_PLAYING.store(true, Ordering::Relaxed);
                    let _ = app.emit("media-playback-changed", true);
                    return Ok("Продовжую відтворення музики ▶️".to_string());
                }
            }

            if action_clean == "play_query" || action_clean == "open_and_play" || (action_clean == "play" && !q.is_empty()) {
                let res = open_and_play_query(q);
                if res.is_ok() {
                    LAST_KNOWN_MUSIC_PLAYING.store(true, Ordering::Relaxed);
                    let _ = app.emit("media-playback-changed", true);
                }
                return res;
            }
        }

        if action_clean == "pause" {
            let handled = pause_media_player();
            LAST_KNOWN_MUSIC_PLAYING.store(false, Ordering::Relaxed);
            let _ = app.emit("media-playback-changed", false);
            if handled {
                return Ok("Музику поставлено на паузу ⏸️".to_string());
            } else {
                return Ok("Команда паузи надіслана ⏸️".to_string());
            }
        }

        if action_clean == "next" {
            let handled = next_media_player();
            LAST_KNOWN_MUSIC_PLAYING.store(true, Ordering::Relaxed);
            let _ = app.emit("media-playback-changed", true);
            if handled {
                return Ok("Увімкнено наступний трек ⏭️".to_string());
            } else {
                return Ok("Команда наступного треку надіслана ⏭️".to_string());
            }
        }

        if action_clean == "prev" || action_clean == "previous" {
            let handled = prev_media_player();
            LAST_KNOWN_MUSIC_PLAYING.store(true, Ordering::Relaxed);
            let _ = app.emit("media-playback-changed", true);
            if handled {
                return Ok("Увімкнено попередній трек ⏮️".to_string());
            } else {
                return Ok("Команда попереднього треку надіслана ⏮️".to_string());
            }
        }

        // Керування системною гучністю звуку через audio.rs
        if action_clean == "volume_up" || action_clean == "louder" {
            let delta = query_clean.and_then(|q| q.parse::<i32>().ok()).unwrap_or(10).abs();
            let cur = audio::adjust_system_volume(delta)?;
            return Ok(format!("Збільшив гучність до {}% 🔊", cur));
        }

        if action_clean == "volume_down" || action_clean == "quieter" {
            let delta = query_clean.and_then(|q| q.parse::<i32>().ok()).unwrap_or(10).abs();
            let cur = audio::adjust_system_volume(-delta)?;
            return Ok(format!("Зменшив гучність до {}% 🔉", cur));
        }

        if action_clean == "set_volume" {
            let val = query_clean
                .and_then(|q| {
                    let digits: String = q.chars().filter(|c| c.is_ascii_digit()).collect();
                    digits.parse::<i32>().ok()
                })
                .unwrap_or(50);
            let actual = audio::set_system_volume(val)?;
            if actual == 0 {
                return Ok("Повна тиша на майданчику, звук вимкнено до 0% 🔇".to_string());
            }
            return Ok(format!("Встановив гучність на {}% 🔉", actual));
        }

        if action_clean == "mute" {
            return audio::set_system_mute(true);
        }

        if action_clean == "unmute" {
            return audio::set_system_mute(false);
        }

        if action_clean == "toggle_mute" {
            let cur = audio::get_system_volume().unwrap_or(50);
            if cur == 0 {
                return audio::set_system_mute(false);
            } else {
                return audio::set_system_mute(true);
            }
        }

        if action_clean == "get_volume" {
            let cur = audio::get_system_volume()?;
            return Ok(format!("Поточний рівень гучності: {}% 🔊", cur));
        }

        // Дія "play" без query:
        let resumed = resume_media_player();
        if resumed {
            LAST_KNOWN_MUSIC_PLAYING.store(true, Ordering::Relaxed);
            let _ = app.emit("media-playback-changed", true);
            return Ok("Продовжую відтворення музики ▶️".to_string());
        }

        let res = open_and_play_query("liked_music");
        if res.is_ok() {
            LAST_KNOWN_MUSIC_PLAYING.store(true, Ordering::Relaxed);
            let _ = app.emit("media-playback-changed", true);
        }
        res
    })
    .await
    .map_err(|e| format!("Помилка обробки медіа: {}", e))?
}

pub fn check_is_music_playing() -> bool {
    #[cfg(target_os = "linux")]
    {
        let py_code = r#"
import sys, dbus
try:
    bus = dbus.SessionBus()
    players = [s for s in bus.list_names() if s.startswith('org.mpris.MediaPlayer2.')]
    if not players:
        sys.exit(1)
    found_any = False
    for p in players:
        try:
            player_obj = bus.get_object(p, '/org/mpris/MediaPlayer2', introspect=False)
            props = dbus.Interface(player_obj, 'org.freedesktop.DBus.Properties')
            status = str(props.Get('org.mpris.MediaPlayer2.Player', 'PlaybackStatus'))
            found_any = True
            if status.lower() == 'playing':
                sys.exit(0)
        except Exception:
            pass
    if found_any:
        sys.exit(2)
except Exception:
    pass
sys.exit(1)
"#;
        if let Some(status) = run_python_mpris_cmd(py_code) {
            let is_playing = status.code() == Some(0);
            LAST_KNOWN_MUSIC_PLAYING.store(is_playing, Ordering::Relaxed);
            return is_playing;
        } else {
            LAST_KNOWN_MUSIC_PLAYING.store(false, Ordering::Relaxed);
            return false;
        }
    }

    #[cfg(target_os = "macos")]
    {
        let script = r#"
tell application "System Events"
    if (exists process "Spotify") then
        tell application "Spotify" to if player state is playing then return "playing"
    end if
    if (exists process "Music") then
        tell application "Music" to if player state is playing then return "playing"
    end if
end tell
"#;
        if let Ok(out) = Command::new("osascript").args(["-e", script]).output() {
            let s = String::from_utf8_lossy(&out.stdout);
            if s.trim() == "playing" {
                LAST_KNOWN_MUSIC_PLAYING.store(true, Ordering::Relaxed);
                return true;
            } else {
                LAST_KNOWN_MUSIC_PLAYING.store(false, Ordering::Relaxed);
                return false;
            }
        }
    }

    #[allow(unreachable_code)]
    LAST_KNOWN_MUSIC_PLAYING.load(Ordering::Relaxed)
}

#[tauri::command]
pub async fn is_music_playing() -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(check_is_music_playing)
        .await
        .map_err(|e| format!("Помилка перевірки музики: {}", e))
}

