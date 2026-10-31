// Модуль універсального конвертера файлів (медіа, зображення, архіви, документи)
// Працює на базі системних рушіїв (FFmpeg, Zip, Tar, LibreOffice, Pandoc)

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use serde::{Deserialize, Serialize};
use crate::config::expand_tilde;
use crate::files::is_sensitive_path;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ConversionResult {
    pub success: bool,
    pub input_path: String,
    pub output_path: String,
    pub output_filename: String,
    pub output_size_bytes: u64,
    pub engine_used: String,
    pub message: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AvailableEngines {
    pub has_ffmpeg: bool,
    pub has_zip: bool,
    pub has_tar: bool,
    pub has_libreoffice: bool,
    pub has_pandoc: bool,
    pub has_markitdown: bool,
}

/// Перевіряє наявність утиліти в системному PATH
pub fn is_command_available(cmd: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        Command::new("where")
            .arg(cmd)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new("which")
            .arg(cmd)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

/// Шукає доступний спосіб запуску Microsoft MarkItDown (CLI-бінарник або python3 -m markitdown)
pub fn get_markitdown_command() -> Option<(String, Vec<String>)> {
    if is_command_available("markitdown") {
        return Some(("markitdown".to_string(), vec![]));
    }
    if let Ok(home) = std::env::var("HOME") {
        let candidate = PathBuf::from(home).join(".local/bin/markitdown");
        if candidate.exists() {
            return Some((candidate.to_string_lossy().to_string(), vec![]));
        }
    }
    if is_command_available("python3") {
        let check = Command::new("python3")
            .args(["-c", "import markitdown"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if let Ok(status) = check {
            if status.success() {
                return Some(("python3".to_string(), vec!["-m".to_string(), "markitdown".to_string()]));
            }
        }
    }
    None
}

/// Отримує інформацію про доступні системні рушії
#[tauri::command]
pub async fn get_available_conversion_formats() -> Result<AvailableEngines, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let has_ffmpeg = is_command_available("ffmpeg");
        let has_zip = is_command_available("zip");
        let has_tar = is_command_available("tar");
        let has_libreoffice = is_command_available("libreoffice") || is_command_available("soffice");
        let has_pandoc = is_command_available("pandoc");
        let has_markitdown = get_markitdown_command().is_some();

        Ok(AvailableEngines {
            has_ffmpeg,
            has_zip,
            has_tar,
            has_libreoffice,
            has_pandoc,
            has_markitdown,
        })
    })
    .await
    .map_err(|e| format!("Помилка перевірки рушіїв: {}", e))?
}

/// Генерує унікальний безпечний шлях для результату конвертації, запобігаючи випадковому перезапису
pub fn generate_unique_target_path(
    source_path: &Path,
    target_ext: &str,
    custom_name: Option<&str>,
) -> Result<PathBuf, String> {
    let clean_ext = target_ext.trim().trim_start_matches('.').to_lowercase();
    if clean_ext.is_empty() {
        return Err("Цільовий формат не може бути порожнім".to_string());
    }

    let parent_dir = if source_path.is_dir() {
        source_path.parent().unwrap_or(source_path)
    } else {
        source_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
    };

    if let Some(custom) = custom_name {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            let custom_path = PathBuf::from(trimmed);
            if custom_path.is_absolute() {
                return Ok(custom_path);
            } else {
                return Ok(parent_dir.join(custom_path));
            }
        }
    }

    let file_stem = if source_path.is_dir() {
        source_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("archive")
    } else {
        source_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("converted")
    };

    let first_attempt = parent_dir.join(format!("{}.{}", file_stem, clean_ext));
    if !first_attempt.exists() {
        return Ok(first_attempt);
    }

    // Якщо файл уже існує, шукаємо унікальний індекс _1, _2 тощо
    for i in 1..1000 {
        let candidate = parent_dir.join(format!("{}_{}.{}", file_stem, i, clean_ext));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }

    Err("Не вдалося знайти вільне ім'я для збереження файлу".to_string())
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConversionCategory {
    VideoToGif,
    VideoToAudio,
    VideoToVideo,
    AudioToAudio,
    ImageToImage,
    ImageToPdf,
    PdfToImage,
    PdfToText,
    ToMarkdown,
    TextNotesToPdf,
    ArchiveFolderOrFiles,
    ExtractArchive,
    DocumentToPdf,
    Unknown,
}

pub fn is_archive_extension(ext: &str) -> bool {
    let clean = ext.trim().trim_start_matches('.').to_lowercase();
    matches!(
        clean.as_str(),
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "tbz2" | "xz" | "txz" | "7z" | "rar" | "zst"
    )
}

pub fn clean_archive_stem(filename: &str) -> String {
    let lower = filename.to_lowercase();
    let archive_exts = [
        ".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst",
        ".tgz", ".tbz2", ".txz",
        ".zip", ".tar", ".7z", ".rar", ".gz", ".bz2", ".xz",
    ];

    for ext in archive_exts {
        if lower.ends_with(ext) {
            let cut_len = filename.len() - ext.len();
            let res = &filename[..cut_len];
            if !res.is_empty() {
                return res.to_string();
            }
        }
    }

    Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename)
        .to_string()
}

pub fn generate_unique_extract_dir(
    source_path: &Path,
    custom_name: Option<&str>,
) -> Result<PathBuf, String> {
    let parent_dir = source_path
        .parent()
        .unwrap_or_else(|| Path::new("."));

    if let Some(custom) = custom_name {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            let custom_path = PathBuf::from(trimmed);
            if custom_path.is_absolute() {
                return Ok(custom_path);
            } else {
                return Ok(parent_dir.join(custom_path));
            }
        }
    }

    let file_name = source_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("extracted");

    let stem = clean_archive_stem(file_name);
    let first_attempt = parent_dir.join(&stem);
    if !first_attempt.exists() {
        return Ok(first_attempt);
    }

    for i in 1..1000 {
        let candidate = parent_dir.join(format!("{}_{}", stem, i));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }

    Err("Не вдалося знайти вільне ім'я директорії для розпакування".to_string())
}

pub fn get_dir_size_recursive(path: &Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                total += get_dir_size_recursive(&p);
            } else if let Ok(meta) = entry.metadata() {
                total += meta.len();
            }
        }
    }
    total
}

pub fn detect_conversion_category(
    src_ext: &str,
    target_ext: &str,
    is_directory: bool,
) -> ConversionCategory {
    let src = src_ext.trim().trim_start_matches('.').to_lowercase();
    let tgt = target_ext.trim().trim_start_matches('.').to_lowercase();

    let extract_targets = [
        "unzip", "extract", "unarchive", "unpack", "folder", "dir", "extract_archive",
    ];
    if extract_targets.contains(&tgt.as_str()) || (is_archive_extension(&src) && extract_targets.contains(&tgt.as_str())) {
        return ConversionCategory::ExtractArchive;
    }

    if is_directory || tgt == "zip" || tgt == "tar.gz" || tgt == "tgz" || tgt == "tar" {
        return ConversionCategory::ArchiveFolderOrFiles;
    }

    let video_exts = ["mp4", "mkv", "avi", "mov", "webm", "flv", "wmv", "m4v", "3gp"];
    let audio_exts = ["mp3", "wav", "flac", "ogg", "aac", "m4a", "wma", "opus"];
    let image_exts = ["png", "jpg", "jpeg", "webp", "bmp", "gif", "ico", "tiff", "avif", "svg"];
    let text_notes_exts = ["md", "markdown", "txt", "csv", "log"];
    let doc_exts = ["docx", "doc", "odt", "rtf", "xlsx", "xls", "pptx", "ppt"];
    let markdown_exts = ["md", "markdown"];
    let to_markdown_sources = [
        "pdf", "docx", "doc", "odt", "rtf", "xlsx", "xls", "pptx", "ppt", "html", "htm", "csv",
        "tsv", "json", "xml", "txt", "log", "md", "markdown",
    ];

    if markdown_exts.contains(&tgt.as_str()) && to_markdown_sources.contains(&src.as_str()) {
        return ConversionCategory::ToMarkdown;
    }

    if video_exts.contains(&src.as_str()) && tgt == "gif" {
        return ConversionCategory::VideoToGif;
    }

    if video_exts.contains(&src.as_str()) && audio_exts.contains(&tgt.as_str()) {
        return ConversionCategory::VideoToAudio;
    }

    if video_exts.contains(&src.as_str()) && video_exts.contains(&tgt.as_str()) {
        return ConversionCategory::VideoToVideo;
    }

    if audio_exts.contains(&src.as_str()) && audio_exts.contains(&tgt.as_str()) {
        return ConversionCategory::AudioToAudio;
    }

    if image_exts.contains(&src.as_str()) && tgt == "pdf" {
        return ConversionCategory::ImageToPdf;
    }

    if src == "pdf" && image_exts.contains(&tgt.as_str()) {
        return ConversionCategory::PdfToImage;
    }

    if src == "pdf" && (tgt == "txt" || tgt == "text") {
        return ConversionCategory::PdfToText;
    }

    if text_notes_exts.contains(&src.as_str()) && tgt == "pdf" {
        return ConversionCategory::TextNotesToPdf;
    }

    if image_exts.contains(&src.as_str()) && image_exts.contains(&tgt.as_str()) {
        return ConversionCategory::ImageToImage;
    }

    if doc_exts.contains(&src.as_str()) && (tgt == "pdf" || tgt == "html") {
        return ConversionCategory::DocumentToPdf;
    }

    ConversionCategory::Unknown
}

/// Виконує конвертацію файлу або директорії за принципом Filee з оптимізованими пресетами
#[tauri::command(rename_all = "snake_case")]
pub async fn convert_file(
    input_path: String,
    target_format: String,
    custom_output_path: Option<String>,
) -> Result<ConversionResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let source = expand_tilde(&input_path)?;
        if !source.exists() {
            return Err(format!("Файл або папка не існує: {}", source.display()));
        }

        if is_sensitive_path(&source) {
            return Err(format!(
                "Доступ заборонено політикою безпеки: заборонено конвертувати конфіденційні шляхи ({})",
                source.display()
            ));
        }

        let is_dir = source.is_dir();
        let src_ext = if is_dir {
            ""
        } else {
            source
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
        };

        let target_ext_clean = target_format
            .trim()
            .trim_start_matches('.')
            .to_lowercase();
        if target_ext_clean.is_empty() {
            return Err("Цільовий формат конвертації не вказано".to_string());
        }

        let is_extract = target_ext_clean == "unzip"
            || target_ext_clean == "extract"
            || target_ext_clean == "unarchive"
            || target_ext_clean == "unpack"
            || target_ext_clean == "folder"
            || target_ext_clean == "dir"
            || target_ext_clean == "extract_archive";

        let category = detect_conversion_category(src_ext, &target_ext_clean, is_dir);

        let dest_path = if is_extract || category == ConversionCategory::ExtractArchive {
            generate_unique_extract_dir(&source, custom_output_path.as_deref())?
        } else {
            generate_unique_target_path(
                &source,
                &target_ext_clean,
                custom_output_path.as_deref(),
            )?
        };

        if is_sensitive_path(&dest_path) {
            return Err(format!(
                "Доступ заборонено політикою безпеки для вихідного шляху ({})",
                dest_path.display()
            ));
        }

        let (engine_used, message) = match category {
            ConversionCategory::VideoToGif => {
                if !is_command_available("ffmpeg") {
                    return Err(
                        "Для конвертації відео в GIF потрібен системний FFmpeg. Будь ласка, встановіть його: `sudo apt install ffmpeg`"
                            .to_string(),
                    );
                }

                // Високоякісний двопрохідний пресет палітри (palettegen + paletteuse)
                // Забезпечує чітке зображення без артефактів та дизерингу
                let vf_filter = "fps=15,scale=480:-1:flags=lanczos,split[s0][s1];[s0]palettegen=max_colors=256:stats_mode=diff[p];[s1][p]paletteuse=dither=bayer:bayer_scale=3";

                let output = Command::new("ffmpeg")
                    .args([
                        "-y",
                        "-i",
                        source.to_str().unwrap_or_default(),
                        "-vf",
                        vf_filter,
                        dest_path.to_str().unwrap_or_default(),
                    ])
                    .output()
                    .map_err(|e| format!("Помилка запуску FFmpeg: {}", e))?;

                if !output.status.success() {
                    let err_text = String::from_utf8_lossy(&output.stderr);
                    return Err(format!("Помилка FFmpeg при створенні GIF: {}", err_text.chars().take(400).collect::<String>()));
                }

                (
                    "FFmpeg (HQ Palettegen)".to_string(),
                    "Відео успішно перетворено на високоякісну GIF-анімацію".to_string(),
                )
            }

            ConversionCategory::VideoToAudio => {
                if !is_command_available("ffmpeg") {
                    return Err("Для вилучення аудіо з відео потрібен FFmpeg (`sudo apt install ffmpeg`)".to_string());
                }

                let output = Command::new("ffmpeg")
                    .args([
                        "-y",
                        "-i",
                        source.to_str().unwrap_or_default(),
                        "-vn",
                        "-q:a",
                        "2",
                        dest_path.to_str().unwrap_or_default(),
                    ])
                    .output()
                    .map_err(|e| format!("Помилка запуску FFmpeg: {}", e))?;

                if !output.status.success() {
                    let err_text = String::from_utf8_lossy(&output.stderr);
                    return Err(format!("Помилка вилучення аудіо: {}", err_text.chars().take(400).collect::<String>()));
                }

                ("FFmpeg Audio Extractor".to_string(), "Аудіодоріжку успішно вилучено з відео".to_string())
            }

            ConversionCategory::VideoToVideo | ConversionCategory::AudioToAudio => {
                if !is_command_available("ffmpeg") {
                    return Err("Для конвертації медіа потрібен FFmpeg (`sudo apt install ffmpeg`)".to_string());
                }

                let output = Command::new("ffmpeg")
                    .args([
                        "-y",
                        "-i",
                        source.to_str().unwrap_or_default(),
                        dest_path.to_str().unwrap_or_default(),
                    ])
                    .output()
                    .map_err(|e| format!("Помилка запуску FFmpeg: {}", e))?;

                if !output.status.success() {
                    let err_text = String::from_utf8_lossy(&output.stderr);
                    return Err(format!("Помилка конвертації медіа: {}", err_text.chars().take(400).collect::<String>()));
                }

                ("FFmpeg Transcoder".to_string(), format!("Медіафайл успішно перекодовано у формат .{}", target_ext_clean))
            }

            ConversionCategory::ImageToImage => {
                if is_command_available("ffmpeg") {
                    let output = Command::new("ffmpeg")
                        .args([
                            "-y",
                            "-i",
                            source.to_str().unwrap_or_default(),
                            dest_path.to_str().unwrap_or_default(),
                        ])
                        .output()
                        .map_err(|e| format!("Помилка запуску FFmpeg: {}", e))?;

                    if !output.status.success() {
                        let err_text = String::from_utf8_lossy(&output.stderr);
                        return Err(format!("Помилка конвертації зображення: {}", err_text.chars().take(400).collect::<String>()));
                    }

                    ("FFmpeg Image Engine".to_string(), format!("Зображення успішно перетворено у .{}", target_ext_clean))
                } else {
                    return Err("Для конвертації зображень потрібен FFmpeg (`sudo apt install ffmpeg`)".to_string());
                }
            }

            ConversionCategory::ImageToPdf => {
                // Спроба використати img2pdf, pandoc, libreoffice або typst
                if is_command_available("img2pdf") {
                    let output = Command::new("img2pdf")
                        .args([
                            source.to_str().unwrap_or_default(),
                            "-o",
                            dest_path.to_str().unwrap_or_default(),
                        ])
                        .output()
                        .map_err(|e| format!("Помилка img2pdf: {}", e))?;

                    if !output.status.success() {
                        return Err("Помилка генерації PDF з зображення".to_string());
                    }
                    ("img2pdf Engine".to_string(), "Зображення перетворено на PDF".to_string())
                } else if is_command_available("pandoc") {
                    let output = Command::new("pandoc")
                        .args([
                            source.to_str().unwrap_or_default(),
                            "-o",
                            dest_path.to_str().unwrap_or_default(),
                        ])
                        .output()
                        .map_err(|e| format!("Помилка pandoc: {}", e))?;

                    if !output.status.success() {
                        return Err("Помилка Pandoc при створенні PDF".to_string());
                    }
                    ("Pandoc PDF Engine".to_string(), "Зображення перетворено на PDF через Pandoc".to_string())
                } else if is_command_available("python3") {
                    let py_code = r#"
import sys
from PIL import Image
try:
    src, dst = sys.argv[1], sys.argv[2]
    img = Image.open(src)
    if img.mode != 'RGB':
        img = img.convert('RGB')
    img.save(dst, 'PDF', resolution=100.0)
except Exception as e:
    sys.stderr.write(str(e))
    sys.exit(1)
"#;
                    let output = Command::new("python3")
                        .args([
                            "-c",
                            py_code,
                            source.to_str().unwrap_or_default(),
                            dest_path.to_str().unwrap_or_default(),
                        ])
                        .output()
                        .map_err(|e| format!("Помилка виклику python3: {}", e))?;

                    if !output.status.success() {
                        let err_text = String::from_utf8_lossy(&output.stderr);
                        return Err(format!("Помилка створення PDF: {}", err_text.chars().take(300).collect::<String>()));
                    }
                    ("Python Pillow Engine".to_string(), "Зображення успішно скомпільовано в PDF".to_string())
                } else {
                    return Err(
                        "Для конвертації зображення в PDF рекомендується встановити `img2pdf` або `python3-pil` (`sudo apt install img2pdf`)"
                            .to_string(),
                    );
                }
            }

            ConversionCategory::PdfToImage => {
                if is_command_available("pdftoppm") {
                    let fmt_flag = if target_ext_clean == "jpg" || target_ext_clean == "jpeg" {
                        "-jpeg"
                    } else {
                        "-png"
                    };

                    let prefix = dest_path.with_extension("");
                    let output = Command::new("pdftoppm")
                        .args([
                            fmt_flag,
                            "-r",
                            "150",
                            "-singlefile",
                            source.to_str().unwrap_or_default(),
                            prefix.to_str().unwrap_or_default(),
                        ])
                        .output()
                        .map_err(|e| format!("Помилка pdftoppm: {}", e))?;

                    if !output.status.success() {
                        let err_text = String::from_utf8_lossy(&output.stderr);
                        return Err(format!("Помилка рендерингу PDF: {}", err_text.chars().take(300).collect::<String>()));
                    }

                    ("pdftoppm Engine".to_string(), format!("PDF успішно перетворено в .{}", target_ext_clean))
                } else {
                    return Err("Для конвертації PDF у зображення потрібна утиліта pdftoppm (`sudo apt install poppler-utils`)".to_string());
                }
            }

            ConversionCategory::PdfToText => {
                if is_command_available("pdftotext") {
                    let output = Command::new("pdftotext")
                        .args([
                            "-layout",
                            source.to_str().unwrap_or_default(),
                            dest_path.to_str().unwrap_or_default(),
                        ])
                        .output()
                        .map_err(|e| format!("Помилка запуску pdftotext: {}", e))?;

                    if !output.status.success() {
                        let err_text = String::from_utf8_lossy(&output.stderr);
                        return Err(format!("Помилка вилучення тексту з PDF: {}", err_text.chars().take(300).collect::<String>()));
                    }

                    ("Poppler pdftotext Engine".to_string(), "PDF успішно перетворено в текстовий документ".to_string())
                } else if is_command_available("pandoc") {
                    let output = Command::new("pandoc")
                        .args([
                            source.to_str().unwrap_or_default(),
                            "-o",
                            dest_path.to_str().unwrap_or_default(),
                        ])
                        .output()
                        .map_err(|e| format!("Помилка pandoc: {}", e))?;

                    if !output.status.success() {
                        return Err("Помилка Pandoc при конвертації PDF у текст".to_string());
                    }

                    ("Pandoc PDF Engine".to_string(), "PDF успішно перетворено в текст через Pandoc".to_string())
                } else {
                    return Err("Для конвертації PDF у текст потрібна системна утиліта pdftotext (`sudo apt install poppler-utils`)".to_string());
                }
            }

            ConversionCategory::ToMarkdown => {
                if let Some((bin, mut prefix_args)) = get_markitdown_command() {
                    prefix_args.push(source.to_str().unwrap_or_default().to_string());
                    prefix_args.push("-o".to_string());
                    prefix_args.push(dest_path.to_str().unwrap_or_default().to_string());

                    let output = Command::new(&bin)
                        .args(&prefix_args)
                        .output()
                        .map_err(|e| format!("Помилка запуску Microsoft MarkItDown: {}", e))?;

                    if !output.status.success() {
                        let err_text = String::from_utf8_lossy(&output.stderr);
                        return Err(format!("Помилка MarkItDown: {}", err_text.chars().take(300).collect::<String>()));
                    }

                    ("Microsoft MarkItDown Engine".to_string(), "Файл успішно перетворено в структурований Markdown через Microsoft MarkItDown".to_string())
                } else if is_command_available("pandoc") {
                    let output = Command::new("pandoc")
                        .args([
                            source.to_str().unwrap_or_default(),
                            "-o",
                            dest_path.to_str().unwrap_or_default(),
                        ])
                        .output()
                        .map_err(|e| format!("Помилка pandoc: {}", e))?;

                    if !output.status.success() {
                        let err_text = String::from_utf8_lossy(&output.stderr);
                        return Err(format!("Помилка Pandoc: {}", err_text.chars().take(300).collect::<String>()));
                    }

                    ("Pandoc Markdown Engine".to_string(), "Файл конвертовано у Markdown через Pandoc".to_string())
                } else {
                    return Err(
                        "Для конвертації офісних документів (.docx, .xlsx, .pptx, .pdf) у Markdown рекомендується встановити Microsoft MarkItDown: `pip install 'markitdown[all]'`"
                            .to_string(),
                    );
                }
            }

            ConversionCategory::ArchiveFolderOrFiles => {
                let parent_dir = source.parent().unwrap_or(&source);
                let item_name = source.file_name().and_then(|n| n.to_str()).unwrap_or(".");

                if target_ext_clean == "zip" {
                    if !is_command_available("zip") {
                        return Err("Для створення ZIP потрібна утиліта zip (`sudo apt install zip`)".to_string());
                    }

                    let output = Command::new("zip")
                        .current_dir(parent_dir)
                        .args([
                            "-r",
                            dest_path.to_str().unwrap_or_default(),
                            item_name,
                        ])
                        .output()
                        .map_err(|e| format!("Помилка запуску zip: {}", e))?;

                    if !output.status.success() {
                        let err_text = String::from_utf8_lossy(&output.stderr);
                        return Err(format!("Помилка створення ZIP-архіву: {}", err_text.chars().take(400).collect::<String>()));
                    }

                    ("Zip Archiver".to_string(), "Успішно запаковано у ZIP-архів".to_string())
                } else {
                    // tar.gz або tar
                    if !is_command_available("tar") {
                        return Err("Для створення Tar потрібна утиліта tar".to_string());
                    }

                    let flag = if target_ext_clean.contains("gz") { "-czf" } else { "-cf" };
                    let output = Command::new("tar")
                        .current_dir(parent_dir)
                        .args([
                            flag,
                            dest_path.to_str().unwrap_or_default(),
                            item_name,
                        ])
                        .output()
                        .map_err(|e| format!("Помилка запуску tar: {}", e))?;

                    if !output.status.success() {
                        let err_text = String::from_utf8_lossy(&output.stderr);
                        return Err(format!("Помилка створення TAR-архіву: {}", err_text.chars().take(400).collect::<String>()));
                    }

                    ("Tar Archiver".to_string(), "Успішно запаковано у TAR-архів".to_string())
                }
            }

            ConversionCategory::ExtractArchive => {
                let src_lower = source.to_string_lossy().to_lowercase();
                fs::create_dir_all(&dest_path)
                    .map_err(|e| format!("Не вдалося створити директорію для розпакування: {}", e))?;

                if src_lower.ends_with(".zip") {
                    if is_command_available("unzip") {
                        let output = Command::new("unzip")
                            .args([
                                "-q",
                                "-o",
                                source.to_str().unwrap_or_default(),
                                "-d",
                                dest_path.to_str().unwrap_or_default(),
                            ])
                            .output()
                            .map_err(|e| format!("Помилка запуску unzip: {}", e))?;

                        if !output.status.success() {
                            let err_text = String::from_utf8_lossy(&output.stderr);
                            return Err(format!("Помилка розархівації ZIP: {}", err_text.chars().take(400).collect::<String>()));
                        }
                        ("Unzip Extractor".to_string(), "Архів ZIP успішно розархівовано".to_string())
                    } else if is_command_available("python3") {
                        let output = Command::new("python3")
                            .args([
                                "-c",
                                "import shutil, sys; shutil.unpack_archive(sys.argv[1], sys.argv[2], 'zip')",
                                source.to_str().unwrap_or_default(),
                                dest_path.to_str().unwrap_or_default(),
                            ])
                            .output()
                            .map_err(|e| format!("Помилка запуску Python unpack: {}", e))?;

                        if !output.status.success() {
                            let err_text = String::from_utf8_lossy(&output.stderr);
                            return Err(format!("Помилка розпакування ZIP через Python: {}", err_text.chars().take(400).collect::<String>()));
                        }
                        ("Python Zip Extractor".to_string(), "Архів ZIP успішно розархівовано".to_string())
                    } else {
                        return Err("Для розархівації ZIP потрібна утиліта unzip (`sudo apt install unzip`) або python3".to_string());
                    }
                } else if src_lower.ends_with(".tar.gz")
                    || src_lower.ends_with(".tgz")
                    || src_lower.ends_with(".tar.bz2")
                    || src_lower.ends_with(".tbz2")
                    || src_lower.ends_with(".tar.xz")
                    || src_lower.ends_with(".txz")
                    || src_lower.ends_with(".tar")
                {
                    if is_command_available("tar") {
                        let output = Command::new("tar")
                            .args([
                                "-xf",
                                source.to_str().unwrap_or_default(),
                                "-C",
                                dest_path.to_str().unwrap_or_default(),
                            ])
                            .output()
                            .map_err(|e| format!("Помилка запуску tar: {}", e))?;

                        if !output.status.success() {
                            let err_text = String::from_utf8_lossy(&output.stderr);
                            return Err(format!("Помилка розархівації TAR: {}", err_text.chars().take(400).collect::<String>()));
                        }
                        ("Tar Extractor".to_string(), "Архів успішно розархівовано".to_string())
                    } else if is_command_available("python3") {
                        let output = Command::new("python3")
                            .args([
                                "-c",
                                "import shutil, sys; shutil.unpack_archive(sys.argv[1], sys.argv[2])",
                                source.to_str().unwrap_or_default(),
                                dest_path.to_str().unwrap_or_default(),
                            ])
                            .output()
                            .map_err(|e| format!("Помилка розпакування TAR: {}", e))?;

                        if !output.status.success() {
                            let err_text = String::from_utf8_lossy(&output.stderr);
                            return Err(format!("Помилка розпакування TAR через Python: {}", err_text.chars().take(400).collect::<String>()));
                        }
                        ("Python Tar Extractor".to_string(), "Архів успішно розархівовано".to_string())
                    } else {
                        return Err("Для розархівації потрібна утиліта tar або python3".to_string());
                    }
                } else if src_lower.ends_with(".7z") {
                    if is_command_available("7z") {
                        let output = Command::new("7z")
                            .args([
                                "x",
                                "-y",
                                &format!("-o{}", dest_path.to_str().unwrap_or_default()),
                                source.to_str().unwrap_or_default(),
                            ])
                            .output()
                            .map_err(|e| format!("Помилка запуску 7z: {}", e))?;

                        if !output.status.success() {
                            let err_text = String::from_utf8_lossy(&output.stderr);
                            return Err(format!("Помилка розпакування 7z: {}", err_text.chars().take(400).collect::<String>()));
                        }
                        ("7-Zip Extractor".to_string(), "Архів .7z успішно розархівовано".to_string())
                    } else if is_command_available("7za") {
                        let output = Command::new("7za")
                            .args([
                                "x",
                                "-y",
                                &format!("-o{}", dest_path.to_str().unwrap_or_default()),
                                source.to_str().unwrap_or_default(),
                            ])
                            .output()
                            .map_err(|e| format!("Помилка запуску 7za: {}", e))?;

                        if !output.status.success() {
                            let err_text = String::from_utf8_lossy(&output.stderr);
                            return Err(format!("Помилка розпакування 7za: {}", err_text.chars().take(400).collect::<String>()));
                        }
                        ("7-Zip Extractor".to_string(), "Архів .7z успішно розархівовано".to_string())
                    } else {
                        return Err("Для розпакування .7z потрібна утиліта p7zip (`sudo apt install p7zip-full`)".to_string());
                    }
                } else if src_lower.ends_with(".rar") {
                    if is_command_available("unrar") {
                        let output = Command::new("unrar")
                            .args([
                                "x",
                                "-y",
                                source.to_str().unwrap_or_default(),
                                dest_path.to_str().unwrap_or_default(),
                            ])
                            .output()
                            .map_err(|e| format!("Помилка запуску unrar: {}", e))?;

                        if !output.status.success() {
                            let err_text = String::from_utf8_lossy(&output.stderr);
                            return Err(format!("Помилка unrar: {}", err_text.chars().take(400).collect::<String>()));
                        }
                        ("Unrar Extractor".to_string(), "Архів .rar успішно розархівовано".to_string())
                    } else if is_command_available("7z") {
                        let output = Command::new("7z")
                            .args([
                                "x",
                                "-y",
                                &format!("-o{}", dest_path.to_str().unwrap_or_default()),
                                source.to_str().unwrap_or_default(),
                            ])
                            .output()
                            .map_err(|e| format!("Помилка запуску 7z для RAR: {}", e))?;

                        if !output.status.success() {
                            let err_text = String::from_utf8_lossy(&output.stderr);
                            return Err(format!("Помилка 7z: {}", err_text.chars().take(400).collect::<String>()));
                        }
                        ("7-Zip RAR Extractor".to_string(), "Архів .rar успішно розархівовано".to_string())
                    } else {
                        return Err("Для розпакування .rar потрібна утиліта unrar (`sudo apt install unrar`) або 7z".to_string());
                    }
                } else {
                    // Універсальний фолбек через Python shutil
                    if is_command_available("python3") {
                        let output = Command::new("python3")
                            .args([
                                "-c",
                                "import shutil, sys; shutil.unpack_archive(sys.argv[1], sys.argv[2])",
                                source.to_str().unwrap_or_default(),
                                dest_path.to_str().unwrap_or_default(),
                            ])
                            .output()
                            .map_err(|e| format!("Помилка запуску Python unpack: {}", e))?;

                        if !output.status.success() {
                            let err_text = String::from_utf8_lossy(&output.stderr);
                            return Err(format!("Не вдалося розпакувати архів: {}", err_text.chars().take(400).collect::<String>()));
                        }
                        ("Universal Archive Extractor".to_string(), "Архів успішно розархівовано".to_string())
                    } else {
                        return Err(format!("Невідомий формат архіву для розпакування: {}", source.display()));
                    }
                }
            }

            ConversionCategory::TextNotesToPdf => {
                // Вбудований векторний рушій рендерингу нотаток/тексту у PDF через Python fpdf2 (з фолбеком на Pillow)
                if is_command_available("python3") {
                    let py_code = r#"
import os, sys

def render_notes_to_pdf(input_file, output_pdf):
    with open(input_file, 'r', encoding='utf-8', errors='replace') as f:
        content = f.read()

    font_paths = [
        '/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf',
        '/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf',
        '/usr/share/fonts/truetype/freefont/FreeSans.ttf',
        'C:\\Windows\\Fonts\\arial.ttf',
        '/System/Library/Fonts/Helvetica.ttc'
    ]
    font_bold_paths = [
        '/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf',
        '/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf',
        'C:\\Windows\\Fonts\\arialbd.ttf',
    ]
    font_path = next((f for f in font_paths if os.path.exists(f)), None)
    font_bold_path = next((f for f in font_bold_paths if os.path.exists(f)), font_path)

    # 1. Спроба створення справжнього векторного PDF з текстовим шаром (fpdf2)
    try:
        from fpdf import FPDF
        pdf = FPDF()
        pdf.set_margins(15, 15, 15)
        pdf.set_auto_page_break(auto=True, margin=15)
        pdf.add_page()
        if font_path:
            pdf.add_font('DocFont', '', font_path)
            pdf.add_font('DocFont', 'B', font_bold_path or font_path)
            pdf.set_font('DocFont', size=10)
        else:
            pdf.set_font('Helvetica', size=10)

        def write_cell(text, font_size=10, bold=False, before=0, after=0, indent=0):
            if before > 0:
                pdf.ln(before)
            family = 'DocFont' if font_path else 'Helvetica'
            style = 'B' if bold else ''
            pdf.set_font(family, style, font_size)
            x = pdf.l_margin + indent
            w = pdf.epw - indent
            pdf.set_x(x)
            pdf.multi_cell(w=w, h=max(4.5, font_size * 0.45), text=text, new_x='LMARGIN', new_y='NEXT')
            if after > 0:
                pdf.ln(after)

        for line in content.splitlines():
            s = line.rstrip()
            if not s:
                pdf.ln(2.5)
                continue
            clean_s = ''.join(c for c in s if ord(c) < 0x10000)
            stripped = clean_s.strip()
            if not stripped:
                pdf.ln(2.5)
                continue

            if stripped in ('---', '***', '___'):
                pdf.ln(2)
                y = pdf.get_y()
                pdf.set_draw_color(200, 200, 200)
                pdf.line(pdf.l_margin, y, pdf.l_margin + pdf.epw, y)
                pdf.ln(3)
                continue

            raw_indent = len(clean_s) - len(clean_s.lstrip())
            indent_level = min(20, (raw_indent // 2) * 5)

            if stripped.startswith('# '):
                write_cell(stripped[2:].strip(), font_size=16, bold=True, before=4, after=2)
            elif stripped.startswith('## '):
                write_cell(stripped[3:].strip(), font_size=13, bold=True, before=3, after=1.5)
            elif stripped.startswith('### '):
                write_cell(stripped[4:].strip(), font_size=11, bold=True, before=2, after=1)
            elif stripped.startswith(('- ', '* ')):
                write_cell('• ' + stripped[2:].strip(), font_size=10, bold=False, after=0.8, indent=indent_level + 3)
            else:
                write_cell(stripped, font_size=10, bold=False, after=0.5, indent=indent_level)

        pdf.output(output_pdf)
        return
    except Exception:
        pass

    # 2. Резервний растровий рендерер через Pillow
    from PIL import Image, ImageDraw, ImageFont

    dpi = 150
    width = int(8.27 * dpi)
    height = int(11.69 * dpi)
    margin_x = 80
    margin_y = 80
    max_w = width - (2 * margin_x)
    max_h = height - margin_y

    font_title = ImageFont.truetype(font_bold_path or font_path, 26) if font_path else ImageFont.load_default()
    font_h2 = ImageFont.truetype(font_bold_path or font_path, 20) if font_path else ImageFont.load_default()
    font_body = ImageFont.truetype(font_path, 15) if font_path else ImageFont.load_default()
    font_small = ImageFont.truetype(font_path, 11) if font_path else ImageFont.load_default()

    pages = []
    current_page = Image.new('RGB', (width, height), color=(255, 255, 255))
    draw = ImageDraw.Draw(current_page)
    y = margin_y

    def new_page():
        nonlocal current_page, draw, y
        pages.append(current_page)
        current_page = Image.new('RGB', (width, height), color=(255, 255, 255))
        draw = ImageDraw.Draw(current_page)
        y = margin_y

    def wrap_text(text, font, max_width):
        words = text.split(' ')
        lines = []
        cur_line = []
        for w in words:
            test = ' '.join(cur_line + [w])
            bbox = font.getbbox(test)
            w_px = bbox[2] - bbox[0]
            if w_px <= max_width:
                cur_line.append(w)
            else:
                if cur_line:
                    lines.append(' '.join(cur_line))
                    cur_line = [w]
                else:
                    lines.append(w)
                    cur_line = []
        if cur_line:
            lines.append(' '.join(cur_line))
        return lines or ['']

    for raw_line in content.splitlines():
        line = raw_line.rstrip()
        if not line:
            y += 12
            if y > max_h:
                new_page()
            continue

        stripped = line.strip()
        if stripped in ('---', '***', '___'):
            y += 8
            if y > max_h:
                new_page()
            draw.line([(margin_x, y), (width - margin_x, y)], fill=(203, 213, 225), width=1)
            y += 12
            continue

        if line.startswith('# '):
            font = font_title
            color = (30, 41, 59)
            text_to_wrap = line[2:].strip()
            line_spacing = 34
            before_spacing = 16
        elif line.startswith('## '):
            font = font_h2
            color = (51, 65, 85)
            text_to_wrap = line[3:].strip()
            line_spacing = 28
            before_spacing = 12
        elif line.startswith('### '):
            font = font_h2
            color = (71, 85, 105)
            text_to_wrap = line[4:].strip()
            line_spacing = 24
            before_spacing = 8
        elif line.startswith('- ') or line.startswith('* '):
            font = font_body
            color = (30, 41, 59)
            text_to_wrap = '• ' + line[2:].strip()
            line_spacing = 22
            before_spacing = 4
        else:
            font = font_body
            color = (30, 41, 59)
            text_to_wrap = line
            line_spacing = 22
            before_spacing = 0

        y += before_spacing
        wrapped = wrap_text(text_to_wrap, font, max_w)
        for subline in wrapped:
            if y + line_spacing > max_h:
                new_page()
            draw.text((margin_x, y), subline, font=font, fill=color)
            y += line_spacing

    pages.append(current_page)
    total = len(pages)
    for idx, p in enumerate(pages):
        d = ImageDraw.Draw(p)
        ft = f'Сторінка {idx + 1} з {total}'
        fb = font_small.getbbox(ft)
        fw = fb[2] - fb[0]
        d.text((width - margin_x - fw, height - 50), ft, font=font_small, fill=(148, 163, 184))

    pages[0].save(output_pdf, 'PDF', resolution=150.0, save_all=True, append_images=pages[1:])

try:
    render_notes_to_pdf(sys.argv[1], sys.argv[2])
except Exception as e:
    sys.stderr.write(str(e))
    sys.exit(1)
"#;
                    let output = Command::new("python3")
                        .args([
                            "-c",
                            py_code,
                            source.to_str().unwrap_or_default(),
                            dest_path.to_str().unwrap_or_default(),
                        ])
                        .output()
                        .map_err(|e| format!("Помилка запуску вбудованого генератора PDF: {}", e))?;

                    if !output.status.success() {
                        let err_text = String::from_utf8_lossy(&output.stderr);
                        return Err(format!("Помилка генерації PDF з нотаток: {}", err_text.chars().take(300).collect::<String>()));
                    }

                    ("Built-in Notes PDF Engine (fpdf2 / Pillow)".to_string(), "Нотатки успішно експортовано у векторний PDF документ".to_string())
                } else if is_command_available("pandoc") {
                    let output = Command::new("pandoc")
                        .args([
                            source.to_str().unwrap_or_default(),
                            "-o",
                            dest_path.to_str().unwrap_or_default(),
                        ])
                        .output()
                        .map_err(|e| format!("Помилка запуску pandoc: {}", e))?;

                    if !output.status.success() {
                        let err_text = String::from_utf8_lossy(&output.stderr);
                        return Err(format!("Помилка Pandoc: {}", err_text.chars().take(300).collect::<String>()));
                    }

                    ("Pandoc PDF Engine".to_string(), "Нотатки скомпільовано у PDF через Pandoc".to_string())
                } else {
                    return Err(
                        "Для експорту нотаток у PDF потрібен python3 з модулем Pillow або pandoc (`sudo apt install python3-pil` або `sudo apt install pandoc`)"
                            .to_string(),
                    );
                }
            }

            ConversionCategory::DocumentToPdf => {
                let parent_dir = dest_path.parent().unwrap_or(&dest_path);
                if is_command_available("libreoffice") || is_command_available("soffice") {
                    let cmd_name = if is_command_available("libreoffice") { "libreoffice" } else { "soffice" };
                    let output = Command::new(cmd_name)
                        .args([
                            "--headless",
                            "--convert-to",
                            "pdf",
                            source.to_str().unwrap_or_default(),
                            "--outdir",
                            parent_dir.to_str().unwrap_or_default(),
                        ])
                        .output()
                        .map_err(|e| format!("Помилка запуску LibreOffice: {}", e))?;

                    if !output.status.success() {
                        let err_text = String::from_utf8_lossy(&output.stderr);
                        return Err(format!("Помилка LibreOffice: {}", err_text.chars().take(400).collect::<String>()));
                    }

                    ("LibreOffice Headless Engine".to_string(), "Документ успішно перетворено на PDF".to_string())
                } else if is_command_available("pandoc") {
                    let output = Command::new("pandoc")
                        .args([
                            source.to_str().unwrap_or_default(),
                            "-o",
                            dest_path.to_str().unwrap_or_default(),
                        ])
                        .output()
                        .map_err(|e| format!("Помилка pandoc: {}", e))?;

                    if !output.status.success() {
                        let err_text = String::from_utf8_lossy(&output.stderr);
                        return Err(format!("Помилка Pandoc: {}", err_text.chars().take(400).collect::<String>()));
                    }

                    ("Pandoc Document Engine".to_string(), "Документ успішно скомпільовано у PDF".to_string())
                } else {
                    return Err(
                        "Для конвертації офісних документів (.docx, .xlsx, .pptx) у PDF зі збереженням розмітки та таблиць потрібен LibreOffice. Встановіть його однією командою: `sudo apt install libreoffice-writer` (або `sudo apt install libreoffice`)"
                            .to_string(),
                    );
                }
            }

            ConversionCategory::Unknown => {
                let media_exts = [
                    "mp4", "mkv", "avi", "mov", "webm", "flv", "wmv", "mp3", "wav", "flac", "ogg",
                    "aac", "m4a", "opus", "gif", "png", "jpg", "jpeg", "webp",
                ];
                let is_media_src = media_exts.contains(&src_ext.to_lowercase().as_str());
                let is_media_tgt = media_exts.contains(&target_ext_clean.as_str());

                // Універсальна спроба через FFmpeg ТІЛЬКИ для мультимедійних файлів
                if is_media_src && is_media_tgt && is_command_available("ffmpeg") {
                    let output = Command::new("ffmpeg")
                        .args([
                            "-y",
                            "-i",
                            source.to_str().unwrap_or_default(),
                            dest_path.to_str().unwrap_or_default(),
                        ])
                        .output();

                    if let Ok(out) = output {
                        if out.status.success() {
                            ("FFmpeg Universal Engine".to_string(), format!("Файл успішно сконвертовано у .{}", target_ext_clean))
                        } else {
                            return Err(format!(
                                "Не підтримується пряма конвертація з .{} у .{}",
                                src_ext, target_ext_clean
                            ));
                        }
                    } else {
                        return Err(format!("Невідомий формат конвертації: .{} -> .{}", src_ext, target_ext_clean));
                    }
                } else {
                    return Err(format!("Не знайдено рушія для конвертації з .{} у .{}", src_ext, target_ext_clean));
                }
            }
        };

        let (output_size, is_result_dir) = if dest_path.is_dir() {
            (get_dir_size_recursive(&dest_path), true)
        } else {
            (fs::metadata(&dest_path).map(|m| m.len()).unwrap_or(0), false)
        };

        if !is_result_dir && output_size == 0 {
            let _ = fs::remove_file(&dest_path);
            let is_pdf = src_ext.eq_ignore_ascii_case("pdf");
            if is_pdf {
                return Err(format!(
                    "Не вдалося вилучити текст з '{}': цей PDF-файл складається лише зі сканованих картинок без цифрового текстового шару. Для розпізнавання сканів встановіть OCR: sudo apt install tesseract-ocr tesseract-ocr-ukr",
                    source.file_name().and_then(|n| n.to_str()).unwrap_or("файл")
                ));
            }
            return Err(format!(
                "Помилка конвертації: отримано порожній файл (0 байтів) для .{} -> .{}",
                src_ext, target_ext_clean
            ));
        }

        let out_filename = dest_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();

        Ok(ConversionResult {
            success: true,
            input_path: source.to_string_lossy().to_string(),
            output_path: dest_path.to_string_lossy().to_string(),
            output_filename: out_filename,
            output_size_bytes: output_size,
            engine_used,
            message,
        })
    })
    .await
    .map_err(|e| format!("Помилка потоку конвертації: {}", e))?
}

/// Відкриває файл у програмі за замовчуванням (текстовий редактор, плеєр, переглядач зображень)
#[tauri::command(rename_all = "snake_case")]
pub async fn open_file(target_path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let clean = expand_tilde(&target_path)?;
        if !clean.exists() {
            return Err(format!("Файл не існує: {}", clean.display()));
        }

        #[cfg(target_os = "windows")]
        {
            Command::new("cmd")
                .args(["/C", "start", "", &clean.to_string_lossy()])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("Помилка відкриття файлу: {}", e))?;
        }

        #[cfg(target_os = "macos")]
        {
            Command::new("open")
                .arg(&clean)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("Помилка відкриття файлу: {}", e))?;
        }

        #[cfg(target_os = "linux")]
        {
            let res = if is_command_available("gio") {
                Command::new("gio")
                    .args(["open", &clean.to_string_lossy()])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
            } else {
                Command::new("xdg-open")
                    .arg(&clean)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
            };

            if let Err(e) = res {
                Command::new("xdg-open")
                    .arg(&clean)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .map_err(|e2| format!("Помилка відкриття файлу через xdg-open: {} / {}", e, e2))?;
            }
        }

        Ok(format!("Відкрито: {}", clean.display()))
    })
    .await
    .map_err(|e| format!("Помилка потоку: {}", e))?
}

/// Відкриває батьківську папку та підсвічує файл у системному файловому менеджері
#[tauri::command(rename_all = "snake_case")]
pub async fn reveal_in_file_manager(target_path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let clean = expand_tilde(&target_path)?;
        if !clean.exists() {
            return Err(format!("Шлях не існує: {}", clean.display()));
        }

        #[cfg(target_os = "windows")]
        {
            Command::new("explorer")
                .args(["/select,", &clean.to_string_lossy()])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("Помилка відкриття провідника: {}", e))?;
        }

        #[cfg(target_os = "macos")]
        {
            Command::new("open")
                .args(["-R", &clean.to_string_lossy()])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("Помилка відкриття Finder: {}", e))?;
        }

        #[cfg(target_os = "linux")]
        {
            if is_command_available("nautilus") && clean.is_file() {
                let res = Command::new("nautilus")
                    .args(["--select", &clean.to_string_lossy()])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();

                if res.is_err() {
                    let parent_dir = clean.parent().unwrap_or(&clean);
                    let _ = Command::new("xdg-open")
                        .arg(parent_dir)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn();
                }
            } else {
                let parent_dir = if clean.is_dir() {
                    clean.as_path()
                } else {
                    clean.parent().unwrap_or(&clean)
                };

                let res = if is_command_available("gio") {
                    Command::new("gio")
                        .args(["open", &parent_dir.to_string_lossy()])
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                } else {
                    Command::new("xdg-open")
                        .arg(parent_dir)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                };

                if let Err(_e) = res {
                    let _ = Command::new("xdg-open")
                        .arg(parent_dir)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn();
                }
            }
        }

        Ok(format!("Відкрито у файловому менеджері: {}", clean.display()))
    })
    .await
    .map_err(|e| format!("Помилка потоку: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_detect_conversion_category_video_to_gif() {
        assert_eq!(
            detect_conversion_category("mp4", "gif", false),
            ConversionCategory::VideoToGif
        );
        assert_eq!(
            detect_conversion_category(".mov", ".GIF", false),
            ConversionCategory::VideoToGif
        );
    }

    #[test]
    fn test_detect_conversion_category_video_to_audio() {
        assert_eq!(
            detect_conversion_category("mp4", "mp3", false),
            ConversionCategory::VideoToAudio
        );
        assert_eq!(
            detect_conversion_category("mkv", "wav", false),
            ConversionCategory::VideoToAudio
        );
    }

    #[test]
    fn test_detect_conversion_category_images() {
        assert_eq!(
            detect_conversion_category("png", "webp", false),
            ConversionCategory::ImageToImage
        );
        assert_eq!(
            detect_conversion_category("jpg", "pdf", false),
            ConversionCategory::ImageToPdf
        );
        assert_eq!(
            detect_conversion_category("pdf", "png", false),
            ConversionCategory::PdfToImage
        );
    }

    #[test]
    fn test_detect_conversion_category_archives() {
        assert_eq!(
            detect_conversion_category("", "zip", true),
            ConversionCategory::ArchiveFolderOrFiles
        );
        assert_eq!(
            detect_conversion_category("txt", "zip", false),
            ConversionCategory::ArchiveFolderOrFiles
        );
    }

    #[test]
    fn test_detect_conversion_category_extract_archive() {
        assert_eq!(
            detect_conversion_category("zip", "unzip", false),
            ConversionCategory::ExtractArchive
        );
        assert_eq!(
            detect_conversion_category("tar.gz", "extract", false),
            ConversionCategory::ExtractArchive
        );
        assert_eq!(
            detect_conversion_category("7z", "folder", false),
            ConversionCategory::ExtractArchive
        );
        assert_eq!(
            detect_conversion_category("rar", "unpack", false),
            ConversionCategory::ExtractArchive
        );
    }

    #[test]
    fn test_clean_archive_stem() {
        assert_eq!(clean_archive_stem("my_photos.zip"), "my_photos");
        assert_eq!(clean_archive_stem("project_backup.tar.gz"), "project_backup");
        assert_eq!(clean_archive_stem("docs.7z"), "docs");
        assert_eq!(clean_archive_stem("plain_folder"), "plain_folder");
    }

    #[test]
    fn test_generate_unique_target_path_basic() {
        let dummy = Path::new("/tmp/test_converter_sample_file_12345.mp4");
        let result = generate_unique_target_path(dummy, "gif", None).unwrap();
        assert_eq!(result.extension().unwrap(), "gif");
        assert!(result.to_str().unwrap().contains("test_converter_sample_file_12345"));
    }

    #[test]
    fn test_detect_conversion_category_notes_to_pdf() {
        assert_eq!(
            detect_conversion_category("md", "pdf", false),
            ConversionCategory::TextNotesToPdf
        );
        assert_eq!(
            detect_conversion_category("txt", "pdf", false),
            ConversionCategory::TextNotesToPdf
        );
        assert_eq!(
            detect_conversion_category("csv", "pdf", false),
            ConversionCategory::TextNotesToPdf
        );
        assert_eq!(
            detect_conversion_category("docx", "pdf", false),
            ConversionCategory::DocumentToPdf
        );
        assert_eq!(
            detect_conversion_category("pdf", "txt", false),
            ConversionCategory::PdfToText
        );
        assert_eq!(
            detect_conversion_category("pdf", "text", false),
            ConversionCategory::PdfToText
        );
        assert_eq!(
            detect_conversion_category("docx", "md", false),
            ConversionCategory::ToMarkdown
        );
        assert_eq!(
            detect_conversion_category("xlsx", "markdown", false),
            ConversionCategory::ToMarkdown
        );
        assert_eq!(
            detect_conversion_category("pptx", "md", false),
            ConversionCategory::ToMarkdown
        );
        assert_eq!(
            detect_conversion_category("md", "md", false),
            ConversionCategory::ToMarkdown
        );
        assert_eq!(
            detect_conversion_category("markdown", "md", false),
            ConversionCategory::ToMarkdown
        );
    }

    #[test]
    fn test_convert_to_markdown_markitdown() {
        let temp_dir = std::env::temp_dir().join(format!("test_conv_md_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let sample_csv = temp_dir.join("sample.csv");
        let _ = fs::write(&sample_csv, "Name,Age\nIvan,18\nOksana,25");

        let res = tauri::async_runtime::block_on(convert_file(
            sample_csv.to_string_lossy().to_string(),
            "md".to_string(),
            None,
        ));

        if get_markitdown_command().is_some() {
            assert!(res.is_ok(), "Expected CSV to markdown conversion: {:?}", res);
            let conv = res.unwrap();
            assert!(conv.success);
            assert!(conv.output_size_bytes > 0);
            assert!(Path::new(&conv.output_path).exists());
            let content = fs::read_to_string(&conv.output_path).unwrap_or_default();
            assert!(content.contains("Ivan") || content.contains("Age"));
            let _ = fs::remove_file(&conv.output_path);
        }

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_convert_text_notes_to_pdf() {
        let temp_dir = std::env::temp_dir().join(format!("test_conv_notes_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let sample_md = temp_dir.join("sample_note.md");
        let _ = fs::write(
            &sample_md,
            "# Тестовий документ\n\n- Пункт 1\n- Пункт 2 з українським текстом\n\nПривіт світ!",
        );

        let res = tauri::async_runtime::block_on(convert_file(
            sample_md.to_string_lossy().to_string(),
            "pdf".to_string(),
            None,
        ));

        if is_command_available("python3") {
            assert!(res.is_ok(), "Expected notes to PDF conversion to succeed: {:?}", res);
            let conv = res.unwrap();
            assert!(conv.success);
            assert!(Path::new(&conv.output_path).exists());
            let _ = fs::remove_file(&conv.output_path);
        }

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_extract_zip_archive() {
        let temp_dir = std::env::temp_dir().join(format!("test_extract_zip_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let sample_file = temp_dir.join("inner.txt");
        let _ = fs::write(&sample_file, "Inner content to extract");

        let zip_res = tauri::async_runtime::block_on(convert_file(
            temp_dir.to_string_lossy().to_string(),
            "zip".to_string(),
            None,
        ));

        if is_command_available("zip") && (is_command_available("unzip") || is_command_available("python3")) {
            assert!(zip_res.is_ok(), "Expected zip to succeed: {:?}", zip_res);
            let zip_conv = zip_res.unwrap();
            let zip_path = zip_conv.output_path;

            let extract_res = tauri::async_runtime::block_on(convert_file(
                zip_path.clone(),
                "unzip".to_string(),
                None,
            ));

            assert!(extract_res.is_ok(), "Expected extract to succeed: {:?}", extract_res);
            let extract_conv = extract_res.unwrap();
            assert!(extract_conv.success);
            let extracted_dir = Path::new(&extract_conv.output_path);
            assert!(extracted_dir.is_dir());

            let _ = fs::remove_file(&zip_path);
            let _ = fs::remove_dir_all(&extracted_dir);
        }

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_convert_file_zip_archive() {
        let temp_dir = std::env::temp_dir().join(format!("test_conv_zip_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let sample_file = temp_dir.join("sample.txt");
        let _ = fs::write(&sample_file, "Hello, converter!");

        let res = tauri::async_runtime::block_on(convert_file(
            temp_dir.to_string_lossy().to_string(),
            "zip".to_string(),
            None,
        ));

        if is_command_available("zip") {
            assert!(res.is_ok(), "Expected zip conversion to succeed: {:?}", res);
            let conv = res.unwrap();
            assert!(conv.success);
            assert!(Path::new(&conv.output_path).exists());
            let _ = fs::remove_file(&conv.output_path);
        }

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_open_file_and_reveal_checks() {
        let temp_dir = std::env::temp_dir().join(format!("test_open_reveal_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let sample_file = temp_dir.join("test_open.txt");
        let _ = fs::write(&sample_file, "Hello, open_file!");

        // 1. Неіснуючий файл має повертати чітку помилку
        let non_existent = temp_dir.join("non_existent_file.xyz");
        let res_err = tauri::async_runtime::block_on(open_file(non_existent.to_string_lossy().to_string()));
        assert!(res_err.is_err());
        assert!(res_err.unwrap_err().contains("не існує"));

        // 2. Перевірка неіснуючого шляху для reveal_in_file_manager
        let res_reveal_err = tauri::async_runtime::block_on(reveal_in_file_manager(non_existent.to_string_lossy().to_string()));
        assert!(res_reveal_err.is_err());
        assert!(res_reveal_err.unwrap_err().contains("не існує"));

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
