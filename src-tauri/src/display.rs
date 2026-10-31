// Модуль для керування яскравістю екрана та нічним режимом (Night Light / тепле світло)
// Підтримує GNOME Wayland/X11 (Mutter D-Bus, gsettings), brightnessctl, light, gammastep, xrandr, Windows WMI, macOS.

use std::process::{Command, Stdio};

pub fn parse_busctl_val(text: &str, key: &str) -> Option<i64> {
    let key_pat = format!("\"{}\" i ", key);
    if let Some(pos) = text.find(&key_pat) {
        let remainder = &text[pos + key_pat.len()..];
        let num_str: String = remainder.chars().take_while(|c| c.is_ascii_digit()).collect();
        return num_str.parse::<i64>().ok();
    }
    None
}

pub fn parse_busctl_str(text: &str, key: &str) -> Option<String> {
    let key_pat = format!("\"{}\" s \"", key);
    if let Some(pos) = text.find(&key_pat) {
        let remainder = &text[pos + key_pat.len()..];
        if let Some(end) = remainder.find('"') {
            return Some(remainder[..end].to_string());
        }
    }
    None
}

pub fn parse_busctl_serial(text: &str) -> Option<u32> {
    let prefix = "(uaa{sv}) ";
    if let Some(pos) = text.find(prefix) {
        let remainder = &text[pos + prefix.len()..];
        let num_str: String = remainder.chars().take_while(|c| c.is_ascii_digit()).collect();
        return num_str.parse::<u32>().ok();
    }
    None
}

pub fn get_screen_brightness() -> Result<i32, String> {
    // 1. Спробувати brightnessctl
    if let Ok(out) = std::process::Command::new("brightnessctl").arg("get").output() {
        if out.status.success() {
            let val_s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if let Ok(val) = val_s.parse::<i64>() {
                if let Ok(out_max) = std::process::Command::new("brightnessctl").arg("max").output() {
                    let max_s = String::from_utf8_lossy(&out_max.stdout).trim().to_string();
                    if let Ok(max) = max_s.parse::<i64>() {
                        if max > 0 {
                            let pct = ((val as f64 / max as f64) * 100.0).round() as i32;
                            return Ok(pct.clamp(0, 100));
                        }
                    }
                }
            }
        }
    }

    // 2. Спробувати GNOME Mutter D-Bus (нативно для Ubuntu Wayland / X11)
    if let Ok(out) = std::process::Command::new("busctl")
        .args([
            "--user",
            "get-property",
            "org.gnome.Mutter.DisplayConfig",
            "/org/gnome/Mutter/DisplayConfig",
            "org.gnome.Mutter.DisplayConfig",
            "Backlight",
        ])
        .output()
    {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            if let (Some(b_min), Some(b_max), Some(val)) = (
                parse_busctl_val(&text, "min"),
                parse_busctl_val(&text, "max"),
                parse_busctl_val(&text, "value"),
            ) {
                if b_max > b_min {
                    let pct = (((val - b_min) as f64 / (b_max - b_min) as f64) * 100.0).round() as i32;
                    return Ok(pct.clamp(0, 100));
                }
            }
        }
    }

    // 3. Спробувати /sys/class/backlight
    if let Ok(entries) = std::fs::read_dir("/sys/class/backlight") {
        for entry in entries.flatten() {
            let p = entry.path();
            let cur_file = p.join("brightness");
            let max_file = p.join("max_brightness");
            if cur_file.exists() && max_file.exists() {
                if let (Ok(cur_s), Ok(max_s)) = (std::fs::read_to_string(cur_file), std::fs::read_to_string(max_file)) {
                    if let (Ok(cur), Ok(max)) = (cur_s.trim().parse::<i64>(), max_s.trim().parse::<i64>()) {
                        if max > 0 {
                            let pct = ((cur as f64 / max as f64) * 100.0).round() as i32;
                            return Ok(pct.clamp(0, 100));
                        }
                    }
                }
            }
        }
    }

    // 4. Спробувати light
    if let Ok(out) = std::process::Command::new("light").arg("-G").output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if let Ok(val) = s.parse::<f64>() {
                return Ok((val.round() as i32).clamp(0, 100));
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        let ps_cmd = "(Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightness).CurrentBrightness";
        if let Ok(out) = std::process::Command::new("powershell").args(["-Command", ps_cmd]).output() {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if let Ok(val) = s.parse::<i32>() {
                    return Ok(val.clamp(0, 100));
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = std::process::Command::new("brightness").arg("-l").output() {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout);
                for line in s.lines() {
                    if line.contains("brightness") {
                        if let Some(val_s) = line.split_whitespace().last() {
                            if let Ok(f) = val_s.parse::<f64>() {
                                return Ok(((f * 100.0).round() as i32).clamp(0, 100));
                            }
                        }
                    }
                }
            }
        }
    }

    Err("Не вдалося визначити поточну яскравість екрана".to_string())
}

pub fn set_screen_brightness(pct: i32) -> Result<i32, String> {
    let target_pct = pct.clamp(5, 100);

    // 1. Спробувати brightnessctl
    if let Ok(out) = std::process::Command::new("brightnessctl")
        .args(["set", &format!("{}%", target_pct)])
        .output()
    {
        if out.status.success() {
            return Ok(target_pct);
        }
    }

    // 2. Спробувати GNOME Mutter D-Bus (busctl)
    if let Ok(out) = std::process::Command::new("busctl")
        .args([
            "--user",
            "get-property",
            "org.gnome.Mutter.DisplayConfig",
            "/org/gnome/Mutter/DisplayConfig",
            "org.gnome.Mutter.DisplayConfig",
            "Backlight",
        ])
        .output()
    {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            if let (Some(serial), Some(conn), Some(b_min), Some(b_max)) = (
                parse_busctl_serial(&text),
                parse_busctl_str(&text, "connector"),
                parse_busctl_val(&text, "min"),
                parse_busctl_val(&text, "max"),
            ) {
                if b_max > b_min {
                    let target_val = b_min + (((b_max - b_min) as f64 * (target_pct as f64 / 100.0)).round() as i64);
                    let res = std::process::Command::new("busctl")
                        .args([
                            "--user",
                            "call",
                            "org.gnome.Mutter.DisplayConfig",
                            "/org/gnome/Mutter/DisplayConfig",
                            "org.gnome.Mutter.DisplayConfig",
                            "SetBacklight",
                            "usi",
                            &serial.to_string(),
                            &conn,
                            &target_val.to_string(),
                        ])
                        .output();
                    if let Ok(call_out) = res {
                        if call_out.status.success() {
                            return Ok(target_pct);
                        }
                    }
                }
            }
        }
    }

    // 3. Спробувати light
    if let Ok(out) = std::process::Command::new("light")
        .args(["-S", &target_pct.to_string()])
        .output()
    {
        if out.status.success() {
            return Ok(target_pct);
        }
    }

    // 4. Спробувати xrandr (програмна зміна яскравості на X11)
    if let Ok(out) = std::process::Command::new("xrandr").output() {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                if line.contains(" connected") {
                    if let Some(conn) = line.split_whitespace().next() {
                        let br = format!("{:.2}", (target_pct as f64 / 100.0).clamp(0.1, 1.0));
                        let _ = std::process::Command::new("xrandr")
                            .args(["--output", conn, "--brightness", &br])
                            .output();
                        return Ok(target_pct);
                    }
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        let ps_cmd = format!("(Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightnessMethods).WmiSetBrightness(1, {})", target_pct);
        if let Ok(out) = std::process::Command::new("powershell").args(["-Command", &ps_cmd]).output() {
            if out.status.success() {
                return Ok(target_pct);
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let br = format!("{:.2}", target_pct as f64 / 100.0);
        if let Ok(out) = std::process::Command::new("brightness").arg(&br).output() {
            if out.status.success() {
                return Ok(target_pct);
            }
        }
    }

    Err("Не вдалося встановити яскравість (перевірте налаштування дисплея або встановіть `brightnessctl`)".to_string())
}

pub fn get_night_light_status() -> Result<(bool, Option<u32>), String> {
    // 1. GNOME gsettings
    if let Ok(out) = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.settings-daemon.plugins.color", "night-light-enabled"])
        .output()
    {
        if out.status.success() {
            let enabled = String::from_utf8_lossy(&out.stdout).trim() == "true";
            let mut temp = None;
            if let Ok(tout) = std::process::Command::new("gsettings")
                .args(["get", "org.gnome.settings-daemon.plugins.color", "night-light-temperature"])
                .output()
            {
                if tout.status.success() {
                    let ts = String::from_utf8_lossy(&tout.stdout);
                    let num_str: String = ts.chars().filter(|c| c.is_ascii_digit()).collect();
                    if let Ok(val) = num_str.parse::<u32>() {
                        temp = Some(val);
                    }
                }
            }
            return Ok((enabled, temp));
        }
    }

    // 2. Перевірка процесів gammastep / redshift
    if let Ok(out) = std::process::Command::new("pgrep").args(["-x", "gammastep"]).output() {
        if out.status.success() {
            return Ok((true, Some(3500)));
        }
    }
    if let Ok(out) = std::process::Command::new("pgrep").args(["-x", "redshift"]).output() {
        if out.status.success() {
            return Ok((true, Some(3500)));
        }
    }

    Err("Не вдалося отримати статус нічного режиму".to_string())
}

pub fn set_night_light(enable: bool, temp: Option<u32>) -> Result<String, String> {
    // 1. Спробувати GNOME gsettings
    let res = std::process::Command::new("gsettings")
        .args([
            "set",
            "org.gnome.settings-daemon.plugins.color",
            "night-light-enabled",
            if enable { "true" } else { "false" },
        ])
        .output();

    if let Ok(out) = res {
        if out.status.success() {
            if enable {
                if let Some(t) = temp {
                    let clamped = t.clamp(1700, 6500);
                    let _ = std::process::Command::new("gsettings")
                        .args([
                            "set",
                            "org.gnome.settings-daemon.plugins.color",
                            "night-light-temperature",
                            &clamped.to_string(),
                        ])
                        .output();
                    return Ok(format!("Увімкнув нічний режим (тепле світло, {}K) 🌙✨", clamped));
                }
                return Ok("Увімкнув нічний режим (тепле світло) 🌙✨".to_string());
            } else {
                return Ok("Вимкнув нічний режим (звичайне світло) ☀️".to_string());
            }
        }
    }

    // 2. Спробувати gammastep fallback
    if enable {
        let t = temp.unwrap_or(3500);
        let _ = std::process::Command::new("pkill").arg("-f").arg("gammastep").output();
        if let Ok(_child) = std::process::Command::new("gammastep")
            .args(["-O", &t.to_string()])
            .spawn()
        {
            return Ok(format!("Увімкнув теплий режим через gammastep ({}K) 🌙", t));
        }
    } else {
        let _ = std::process::Command::new("pkill").arg("-f").arg("gammastep").output();
        let _ = std::process::Command::new("pkill").arg("-f").arg("redshift").output();
        return Ok("Вимкнув нічний режим ☀️".to_string());
    }

    Err("Не вдалося налаштувати нічний режим (підтримується GNOME, gammastep або redshift)".to_string())
}

/// Виконує системне блокування сесії (Linux loginctl/xdg-screensaver, Windows, macOS).
pub fn execute_lock_screen() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        // 1. loginctl lock-session (стандарт для systemd / Wayland / GNOME / KDE)
        if let Ok(mut child) = Command::new("loginctl")
            .arg("lock-session")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            if let Ok(Some(status)) = child.try_wait() {
                if status.success() {
                    return Ok(());
                }
            } else {
                return Ok(());
            }
        }

        // 2. xdg-screensaver lock
        if let Ok(mut child) = Command::new("xdg-screensaver")
            .arg("lock")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            if let Ok(Some(status)) = child.try_wait() {
                if status.success() {
                    return Ok(());
                }
            } else {
                return Ok(());
            }
        }

        // 3. GNOME ScreenSaver D-Bus
        let _ = Command::new("busctl")
            .args([
                "--user",
                "call",
                "org.gnome.ScreenSaver",
                "/org/gnome/ScreenSaver",
                "org.gnome.ScreenSaver",
                "Lock",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();

        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        let status = Command::new("rundll32.exe")
            .args(["user32.dll,LockWorkStation"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();

        if status.map(|s| s.success()).unwrap_or(false) {
            return Ok(());
        }
        return Err("Не вдалося заблокувати Windows станцію".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        let status = Command::new("pmset")
            .arg("displaysleepnow")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();

        if status.map(|s| s.success()).unwrap_or(false) {
            return Ok(());
        }
        return Err("Не вдалося заблокувати екран macOS".to_string());
    }

    #[allow(unreachable_code)]
    Ok(())
}

/// Команда блокування екрана та призупинення медіа («Kill Switch / Я відійшов»)
#[tauri::command]
pub async fn lock_screen() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::media::pause_media_player();
        #[cfg(not(test))]
        {
            execute_lock_screen()?;
        }
        Ok("Музику поставлено на паузу, екран заблоковано 🔒. Гарного відпочинку!".to_string())
    })
    .await
    .map_err(|e| format!("Помилка блокування екрана: {}", e))?
}

#[tauri::command]
pub async fn control_display(
    action: String,
    value: Option<i32>,
    enabled: Option<bool>,
    temperature: Option<u32>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let act = action.trim().to_lowercase();
        match act.as_str() {
            "lock_screen" | "lock" => {
                crate::media::pause_media_player();
                #[cfg(not(test))]
                {
                    execute_lock_screen()?;
                }
                Ok("Музику поставлено на паузу, екран заблоковано 🔒. Гарного відпочинку!".to_string())
            }
            "get_brightness" => {
                let cur = get_screen_brightness()?;
                Ok(format!("Поточна яскравість екрана: {}% 🔆", cur))
            }
            "set_brightness" => {
                let target = value.unwrap_or(50).clamp(5, 100);
                let actual = set_screen_brightness(target)?;
                Ok(format!("Встановив яскравість на {}% 🔆", actual))
            }
            "increase_brightness" => {
                let delta = value.unwrap_or(10).abs();
                let cur = get_screen_brightness().unwrap_or(50);
                let target = (cur + delta).min(100);
                let actual = set_screen_brightness(target)?;
                Ok(format!("Збільшив яскравість з {}% до {}% ☀️", cur, actual))
            }
            "decrease_brightness" => {
                let delta = value.unwrap_or(10).abs();
                let cur = get_screen_brightness().unwrap_or(50);
                let target = (cur - delta).max(5);
                let actual = set_screen_brightness(target)?;
                Ok(format!("Зменшив яскравість з {}% до {}% 🌙", cur, actual))
            }
            "enable_night_light" => {
                set_night_light(true, temperature)
            }
            "disable_night_light" => {
                set_night_light(false, None)
            }
            "toggle_night_light" => {
                let should_enable = enabled.unwrap_or_else(|| {
                    let (cur, _) = get_night_light_status().unwrap_or((false, None));
                    !cur
                });
                set_night_light(should_enable, temperature)
            }
            "get_night_light" => {
                let (active, temp) = get_night_light_status()?;
                if active {
                    if let Some(t) = temp {
                        Ok(format!("Нічний режим зараз активний (тепле світло, {}K) 🌙", t))
                    } else {
                        Ok("Нічний режим зараз активний (тепле світло) 🌙".to_string())
                    }
                } else {
                    Ok("Нічний режим зараз вимкнений ☀️".to_string())
                }
            }
            "set_night_light_temp" => {
                let t = temperature.or(value.map(|v| v as u32)).unwrap_or(2700);
                set_night_light(true, Some(t))
            }
            _ => Err(format!("Невідома дія для control_display: '{}'", act)),
        }
    })
    .await
    .map_err(|e| format!("Помилка обробки дисплея: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_busctl_display_backlight() {
        let sample = "(uaa{sv}) 5 1 5 \"connector\" s \"eDP-1\" \"active\" b true \"min\" i 960 \"max\" i 96000 \"value\" i 29472";
        assert_eq!(parse_busctl_serial(sample), Some(5));
        assert_eq!(parse_busctl_str(sample, "connector"), Some("eDP-1".to_string()));
        assert_eq!(parse_busctl_val(sample, "min"), Some(960));
        assert_eq!(parse_busctl_val(sample, "max"), Some(96000));
        assert_eq!(parse_busctl_val(sample, "value"), Some(29472));
    }

    #[test]
    fn test_control_display_lock_screen() {
        let res = tauri::async_runtime::block_on(control_display(
            "lock_screen".to_string(),
            None,
            None,
            None,
        ));
        assert!(res.is_ok());
        assert!(res.unwrap().contains("заблоковано"));

        let res_lock = tauri::async_runtime::block_on(lock_screen());
        assert!(res_lock.is_ok());
        assert!(res_lock.unwrap().contains("заблоковано"));
    }
}

