// Модуль для керування системною гучністю звуку
// Підтримує PipeWire (wpctl - дефолт для Ubuntu/Debian), PulseAudio (pactl), ALSA (amixer), macOS (osascript).

pub fn get_system_volume() -> Result<i32, String> {
    // 1. PipeWire (wpctl)
    if let Ok(out) = std::process::Command::new("wpctl")
        .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            for part in s.split_whitespace() {
                if let Ok(f) = part.parse::<f64>() {
                    return Ok(((f * 100.0).round() as i32).clamp(0, 150));
                }
            }
        }
    }

    // 2. PulseAudio (pactl)
    if let Ok(out) = std::process::Command::new("pactl")
        .args(["get-sink-volume", "@DEFAULT_SINK@"])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            if let Some(pos) = s.find('%') {
                let prefix = &s[..pos];
                if let Some(num_str) = prefix.split_whitespace().last() {
                    if let Ok(val) = num_str.parse::<i32>() {
                        return Ok(val.clamp(0, 150));
                    }
                }
            }
        }
    }

    // 3. ALSA (amixer)
    if let Ok(out) = std::process::Command::new("amixer").args(["sget", "Master"]).output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            if let Some(pos) = s.find('%') {
                let prefix = &s[..pos];
                if let Some(start) = prefix.rfind('[') {
                    if let Ok(val) = prefix[start + 1..].parse::<i32>() {
                        return Ok(val.clamp(0, 150));
                    }
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = std::process::Command::new("osascript")
            .args(["-e", "output volume of (get volume settings)"])
            .output()
        {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if let Ok(val) = s.parse::<i32>() {
                    return Ok(val.clamp(0, 100));
                }
            }
        }
    }

    Err("Не вдалося визначити рівень гучності".to_string())
}

pub fn set_system_volume(pct: i32) -> Result<i32, String> {
    let target = pct.clamp(0, 100);

    // 1. PipeWire (wpctl)
    if let Ok(out) = std::process::Command::new("wpctl")
        .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{}%", target)])
        .output()
    {
        if out.status.success() {
            if target > 0 {
                let _ = std::process::Command::new("wpctl")
                    .args(["set-mute", "@DEFAULT_AUDIO_SINK@", "0"])
                    .output();
            } else {
                let _ = std::process::Command::new("wpctl")
                    .args(["set-mute", "@DEFAULT_AUDIO_SINK@", "1"])
                    .output();
            }
            return Ok(target);
        }
    }

    // 2. PulseAudio (pactl)
    if let Ok(out) = std::process::Command::new("pactl")
        .args(["set-sink-volume", "@DEFAULT_SINK@", &format!("{}%", target)])
        .output()
    {
        if out.status.success() {
            if target > 0 {
                let _ = std::process::Command::new("pactl")
                    .args(["set-sink-mute", "@DEFAULT_SINK@", "0"])
                    .output();
            } else {
                let _ = std::process::Command::new("pactl")
                    .args(["set-sink-mute", "@DEFAULT_SINK@", "1"])
                    .output();
            }
            return Ok(target);
        }
    }

    // 3. ALSA (amixer)
    if let Ok(out) = std::process::Command::new("amixer")
        .args(["set", "Master", &format!("{}%", target)])
        .output()
    {
        if out.status.success() {
            return Ok(target);
        }
    }

    #[cfg(target_os = "macos")]
    {
        let script = format!("set volume output volume {}", target);
        if let Ok(out) = std::process::Command::new("osascript").args(["-e", &script]).output() {
            if out.status.success() {
                return Ok(target);
            }
        }
    }

    Err("Не вдалося встановити гучність (перевірте наявність wpctl, pactl або amixer)".to_string())
}

pub fn adjust_system_volume(delta: i32) -> Result<i32, String> {
    // 1. PipeWire (wpctl)
    let wpctl_arg = if delta >= 0 {
        format!("{}%+", delta.abs())
    } else {
        format!("{}%-", delta.abs())
    };
    if let Ok(out) = std::process::Command::new("wpctl")
        .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &wpctl_arg])
        .output()
    {
        if out.status.success() {
            if delta > 0 {
                let _ = std::process::Command::new("wpctl")
                    .args(["set-mute", "@DEFAULT_AUDIO_SINK@", "0"])
                    .output();
            }
            return get_system_volume().or(Ok(50));
        }
    }

    // 2. PulseAudio (pactl)
    let pactl_arg = if delta >= 0 {
        format!("+{}%", delta.abs())
    } else {
        format!("-{}%", delta.abs())
    };
    if let Ok(out) = std::process::Command::new("pactl")
        .args(["set-sink-volume", "@DEFAULT_SINK@", &pactl_arg])
        .output()
    {
        if out.status.success() {
            if delta > 0 {
                let _ = std::process::Command::new("pactl")
                    .args(["set-sink-mute", "@DEFAULT_SINK@", "0"])
                    .output();
            }
            return get_system_volume().or(Ok(50));
        }
    }

    // 3. ALSA (amixer)
    let amixer_arg = if delta >= 0 {
        format!("{}%+", delta.abs())
    } else {
        format!("{}%-", delta.abs())
    };
    if let Ok(out) = std::process::Command::new("amixer")
        .args(["set", "Master", &amixer_arg])
        .output()
    {
        if out.status.success() {
            return get_system_volume().or(Ok(50));
        }
    }

    Err("Не вдалося змінити гучність".to_string())
}

pub fn set_system_mute(mute: bool) -> Result<String, String> {
    // 1. PipeWire (wpctl)
    let arg = if mute { "1" } else { "0" };
    if let Ok(out) = std::process::Command::new("wpctl")
        .args(["set-mute", "@DEFAULT_AUDIO_SINK@", arg])
        .output()
    {
        if out.status.success() {
            return Ok(if mute {
                "Звук вимкнено (Mute) 🔇".to_string()
            } else {
                "Звук увімкнено 🔊".to_string()
            });
        }
    }

    // 2. PulseAudio (pactl)
    let pactl_arg = if mute { "1" } else { "0" };
    if let Ok(out) = std::process::Command::new("pactl")
        .args(["set-sink-mute", "@DEFAULT_SINK@", pactl_arg])
        .output()
    {
        if out.status.success() {
            return Ok(if mute {
                "Звук вимкнено (Mute) 🔇".to_string()
            } else {
                "Звук увімкнено 🔊".to_string()
            });
        }
    }

    // 3. ALSA (amixer)
    let amixer_arg = if mute { "mute" } else { "unmute" };
    if let Ok(out) = std::process::Command::new("amixer")
        .args(["set", "Master", amixer_arg])
        .output()
    {
        if out.status.success() {
            return Ok(if mute {
                "Звук вимкнено (Mute) 🔇".to_string()
            } else {
                "Звук увімкнено 🔊".to_string()
            });
        }
    }

    #[cfg(target_os = "macos")]
    {
        let script = format!("set volume output muted {}", mute);
        if let Ok(out) = std::process::Command::new("osascript").args(["-e", &script]).output() {
            if out.status.success() {
                return Ok(if mute {
                    "Звук вимкнено (Mute) 🔇".to_string()
                } else {
                    "Звук увімкнено 🔊".to_string()
                });
            }
        }
    }

    Err("Не вдалося перемкнути звук".to_string())
}

