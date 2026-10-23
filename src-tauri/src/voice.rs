// Модуль запису аудіо з мікрофона (PipeWire / ALSA) та кодування у WAV Base64

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

pub struct RecordingSession {
    child: Child,
    file_path: PathBuf,
}

static VOICE_RECORDER: OnceLock<Mutex<Option<RecordingSession>>> = OnceLock::new();

fn get_recorder() -> &'static Mutex<Option<RecordingSession>> {
    VOICE_RECORDER.get_or_init(|| Mutex::new(None))
}

/// Кодує байти у стандартний Base64 (RFC 4648).
pub fn encode_base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(((input.len() + 2) / 3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Гарантує коректні розміри RIFF та data-чанків у WAV-файлі після зупинки запису.
pub fn patch_wav_header(bytes: &mut [u8]) {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return;
    }
    let riff_size = ((bytes.len() - 8) as u32).to_le_bytes();
    bytes[4..8].copy_from_slice(&riff_size);

    let mut offset = 12usize;
    while offset + 8 <= bytes.len() {
        if &bytes[offset..offset + 4] == b"data" {
            let data_size = ((bytes.len() - (offset + 8)) as u32).to_le_bytes();
            bytes[offset + 4..offset + 8].copy_from_slice(&data_size);
            break;
        }
        let chunk_size = u32::from_le_bytes([
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ]) as usize;
        let next = offset
            .saturating_add(8)
            .saturating_add(chunk_size)
            .saturating_add(chunk_size % 2);
        if next <= offset {
            break;
        }
        offset = next;
    }
}

pub fn terminate_recording_session(mut session: RecordingSession) -> PathBuf {
    let pid = session.child.id();
    let _ = Command::new("kill")
        .args(["-s", "INT", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    for _ in 0..12 {
        if let Ok(Some(_)) = session.child.try_wait() {
            break;
        }
        thread::sleep(Duration::from_millis(40));
    }

    let _ = session.child.kill();
    let _ = session.child.wait();
    session.file_path
}

/// 5. Запускає фоновий запис голосу з мікрофона через PipeWire (`pw-record`) або ALSA (`arecord`).
#[tauri::command]
pub async fn start_voice_recording() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = get_recorder()
            .lock()
            .map_err(|_| "Помилка блокування аудіорекордера".to_string())?;

        if let Some(old_session) = guard.take() {
            let old_path = terminate_recording_session(old_session);
            let _ = fs::remove_file(old_path);
        }

        let file_path = env::temp_dir().join("local_helper_voice.wav");
        let _ = fs::remove_file(&file_path);
        let path_str = file_path.to_string_lossy().to_string();

        let child = Command::new("pw-record")
            .args(["--rate=16000", "--channels=1", "--format=s16", &path_str])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .or_else(|_| {
                Command::new("arecord")
                    .args([
                        "-f", "S16_LE", "-r", "16000", "-c", "1", "-t", "wav", &path_str,
                    ])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
            })
            .map_err(|e| {
                format!(
                    "Не вдалося запустити запис мікрофона (pw-record / arecord): {}",
                    e
                )
            })?;

        *guard = Some(RecordingSession { child, file_path });
        Ok(())
    })
    .await
    .map_err(|e| format!("Помилка потоку: {}", e))?
}

/// 6. Зупиняє запис голосу та повертає Base64-рядок `audio/wav`.
#[tauri::command]
pub async fn stop_voice_recording() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let session = {
            let mut guard = get_recorder()
                .lock()
                .map_err(|_| "Помилка блокування аудіорекордера".to_string())?;
            guard
                .take()
                .ok_or_else(|| "Активний запис голосу не знайдено".to_string())?
        };

        let file_path = terminate_recording_session(session);
        let mut bytes = fs::read(&file_path)
            .map_err(|e| format!("Не вдалося прочитати записаний аудіофайл: {}", e))?;
        let _ = fs::remove_file(&file_path);

        if bytes.len() <= 200 {
            return Err("Запис занадто короткий або мікрофон не передав звук.".to_string());
        }

        patch_wav_header(&mut bytes);
        Ok(encode_base64(&bytes))
    })
    .await
    .map_err(|e| format!("Помилка потоку: {}", e))?
}

/// 7. Скасовує активний запис голосу без повернення даних.
#[tauri::command]
pub async fn cancel_voice_recording() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = get_recorder()
            .lock()
            .map_err(|_| "Помилка блокування аудіорекордера".to_string())?;

        if let Some(session) = guard.take() {
            let file_path = terminate_recording_session(session);
            let _ = fs::remove_file(file_path);
        }
        Ok(())
    })
    .await
    .map_err(|e| format!("Помилка потоку: {}", e))?
}

