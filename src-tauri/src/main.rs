// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::{self, Write};

fn print_help() {
    println!(
        r#"🤖 Local Helper Mascot — Десктопний AI-асистент

ВИКОРИСТАННЯ:
  local-helper-ai [КОМАНДА] [АРГУМЕНТИ]
  addkey [КЛЮЧ]

КОМАНДИ НАЛАШТУВАННЯ:
  addkey [КЛЮЧ]              Встановити ключ Gemini API (або ввести інтерактивно)
  set-model [МОДЕЛЬ]         Встановити модель Gemini (наприклад: gemini-3.5-flash-lite)
  set-vault [ШЛЯХ]           Встановити шлях до Obsidian Vault (наприклад: ~/Documents/Notes)
  set <ПАРАМЕТР> <ЗНАЧЕННЯ>  Встановити будь-який параметр у файлі конфігурації
  config                     Переглянути поточні налаштування та шлях до файлу конфігурації
  --help, -h                 Показати цю довідку
  --version, -v              Показати версію програми

ПРИКЛАДИ:
  addkey AIzaSyDxxxxxxxxxxxxxxxxxxxxxxxx
  local-helper-ai addkey AIzaSyDxxxxxxxxxxxxxxxxxxxxxxxx
  local-helper-ai set-model gemini-3.5-flash-lite
  local-helper-ai set-vault ~/Documents/Obsidian
  local-helper-ai config

Запуск без аргументів (або клік по іконці в системному меню) відкриває вікно маскота."#
    );
}

fn handle_cli_addkey(maybe_key: Option<String>) {
    if let Some(ref k) = maybe_key {
        if k == "--help" || k == "-h" || k == "help" {
            println!(
                r#"🔑 Використання команди addkey:
  addkey <ВАШ_GEMINI_API_KEY>   Встановити ключ Gemini API напряму
  addkey                        Ввести ключ інтерактивно в терміналі

Отримати ключ безкоштовно: https://aistudio.google.com/
Конфігурація зберігається у: ~/.config/local-helper-ai/config.env"#
            );
            return;
        }
    }

    let key = match maybe_key {
        Some(k) if !k.trim().is_empty() => k.trim().to_string(),
        _ => {
            println!("🔑 Введіть ваш Gemini API Key:");
            print!("> ");
            let _ = io::stdout().flush();
            let mut input = String::new();
            if io::stdin().read_line(&mut input).is_err() || input.trim().is_empty() {
                eprintln!("❌ Помилка: ключ не може бути порожнім.");
                std::process::exit(1);
            }
            input.trim().to_string()
        }
    };

    match local_helper_ai_lib::update_config_value("GEMINI_API_KEY", &key) {
        Ok(path) => {
            let masked = if key.len() > 10 {
                format!("{}...{}", &key[..6], &key[key.len() - 4..])
            } else {
                "********".to_string()
            };
            println!("✅ Ключ Gemini API ({}) успішно збережено у:", masked);
            println!("   {}", path.display());
            println!("💡 Тепер ви можете запустити Local Helper Mascot з меню програм або терміналу.");
        }
        Err(err) => {
            eprintln!("❌ Помилка збереження ключа: {}", err);
            std::process::exit(1);
        }
    }
}

fn handle_cli_set_model(maybe_model: Option<String>) {
    if let Some(ref m) = maybe_model {
        if m == "--help" || m == "-h" || m == "help" {
            println!(
                r#"🧠 Використання команди set-model:
  local-helper-ai set-model <МОДЕЛЬ>   Встановити модель Gemini

Рекомендовані моделі:
  - gemini-3.5-flash-lite (за замовчуванням: надшвидка та легка)
  - gemini-2.5-flash      (швидка мультимодальна)
  - gemini-2.5-pro        (максимальна якість міркувань)"#
            );
            return;
        }
    }

    let model = match maybe_model {
        Some(m) if !m.trim().is_empty() => m.trim().to_string(),
        _ => {
            println!("🧠 Введіть назву моделі Gemini (наприклад: gemini-3.5-flash-lite або gemini-2.5-flash):");
            print!("> ");
            let _ = io::stdout().flush();
            let mut input = String::new();
            if io::stdin().read_line(&mut input).is_err() || input.trim().is_empty() {
                eprintln!("❌ Помилка: назва моделі не може бути порожньою.");
                std::process::exit(1);
            }
            input.trim().to_string()
        }
    };

    match local_helper_ai_lib::update_config_value("GEMINI_MODEL", &model) {
        Ok(path) => {
            println!("✅ Модель Gemini змінено на '{}'.", model);
            println!("   Збережено у: {}", path.display());
        }
        Err(err) => {
            eprintln!("❌ Помилка збереження моделі: {}", err);
            std::process::exit(1);
        }
    }
}

fn handle_cli_set_vault(maybe_vault: Option<String>) {
    if let Some(ref v) = maybe_vault {
        if v == "--help" || v == "-h" || v == "help" {
            println!(
                r#"📝 Використання команди set-vault:
  local-helper-ai set-vault <ШЛЯХ>   Встановити шлях до папки Obsidian Vault

Приклад:
  local-helper-ai set-vault ~/Documents/Obsidian
  local-helper-ai set-vault /home/ivan/Obsidian/Ivan"#
            );
            return;
        }
    }

    let vault = match maybe_vault {
        Some(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => {
            println!("📝 Введіть шлях до Obsidian Vault (наприклад, ~/Documents/Notes або /home/user/Obsidian):");
            print!("> ");
            let _ = io::stdout().flush();
            let mut input = String::new();
            if io::stdin().read_line(&mut input).is_err() || input.trim().is_empty() {
                eprintln!("❌ Помилка: шлях не може бути порожнім.");
                std::process::exit(1);
            }
            input.trim().to_string()
        }
    };

    match local_helper_ai_lib::update_config_value("OBSIDIAN_VAULT_PATH", &vault) {
        Ok(path) => {
            println!("✅ Шлях до Obsidian Vault змінено на '{}'.", vault);
            println!("   Збережено у: {}", path.display());
        }
        Err(err) => {
            eprintln!("❌ Помилка збереження шляху до Vault: {}", err);
            std::process::exit(1);
        }
    }
}

fn handle_cli_set(maybe_key: Option<String>, maybe_val: Option<String>) {
    let (key, val) = match (maybe_key, maybe_val) {
        (Some(k), Some(v)) if !k.trim().is_empty() && !v.trim().is_empty() => {
            (k.trim().to_string(), v.trim().to_string())
        }
        _ => {
            eprintln!("❌ Використання: local-helper-ai set <ПАРАМЕТР> <ЗНАЧЕННЯ>");
            std::process::exit(1);
        }
    };

    match local_helper_ai_lib::update_config_value(&key, &val) {
        Ok(path) => {
            println!("✅ Параметр '{}' успішно оновлено.", key);
            println!("   Збережено у: {}", path.display());
        }
        Err(err) => {
            eprintln!("❌ Помилка збереження параметра: {}", err);
            std::process::exit(1);
        }
    }
}

fn handle_cli_show_config() {
    let config = local_helper_ai_lib::read_app_config();
    let masked_key = if config.gemini_api_key.is_empty() {
        "❌ Не встановлено (виконайте: addkey <ВАШ_КЛЮЧ>)".to_string()
    } else if config.gemini_api_key.len() > 10 {
        format!(
            "{}...{} (Встановлено ✅)",
            &config.gemini_api_key[..6],
            &config.gemini_api_key[config.gemini_api_key.len() - 4..]
        )
    } else {
        "******** (Встановлено ✅)".to_string()
    };

    let vault_status = if config.obsidian_vault_path.is_empty() {
        "⚠️  Не встановлено (виконайте: local-helper-ai set-vault <ШЛЯХ>)".to_string()
    } else {
        format!("{} ✅", config.obsidian_vault_path)
    };

    println!("==================================================");
    println!("🤖 Local Helper Mascot — Поточна конфігурація");
    println!("==================================================");
    println!("📁 Файл конфігурації : {}", config.config_file_path);
    println!("🔑 Gemini API Key    : {}", masked_key);
    println!("🧠 Модель Gemini     : {}", config.gemini_model);
    println!("📝 Obsidian Vault    : {}", vault_status);
    println!("==================================================");
    println!("Корисні команди:");
    println!("  addkey <КЛЮЧ>                       Встановити/змінити API ключ");
    println!("  local-helper-ai set-model <МОДЕЛЬ>  Змінити модель Gemini");
    println!("  local-helper-ai set-vault <ШЛЯХ>    Змінити шлях до нотаток Obsidian");
    println!("  local-helper-ai config              Показати конфігурацію");
    println!("==================================================");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let bin_name = args.first().map(|s| s.as_str()).unwrap_or("");

    // Якщо бінарник або скрипт викликано як `addkey`
    if bin_name.ends_with("addkey") {
        handle_cli_addkey(args.get(1).cloned());
        return;
    }

    if args.len() > 1 {
        match args[1].as_str() {
            "addkey" | "set-key" => {
                handle_cli_addkey(args.get(2).cloned());
                return;
            }
            "set-model" => {
                handle_cli_set_model(args.get(2).cloned());
                return;
            }
            "set-vault" => {
                handle_cli_set_vault(args.get(2).cloned());
                return;
            }
            "set" => {
                handle_cli_set(args.get(2).cloned(), args.get(3).cloned());
                return;
            }
            "config" | "show-config" | "status" => {
                handle_cli_show_config();
                return;
            }
            "--help" | "-h" | "help" => {
                print_help();
                return;
            }
            "--version" | "-v" | "version" => {
                println!("Local Helper Mascot v{}", env!("CARGO_PKG_VERSION"));
                return;
            }
            other => {
                if other.starts_with("AIzaSy") {
                    println!("💡 Схоже, ви передали API ключ безпосередньо.");
                    handle_cli_addkey(Some(other.to_string()));
                    return;
                }
                eprintln!("❌ Невідома команда: {}\n", other);
                print_help();
                std::process::exit(1);
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        // У GNOME на Wayland стандартний протокол xdg-shell блокує always-on-top для клієнтських вікон.
        // GDK_BACKEND=x11 перемикає вікно на XWayland, де _NET_WM_STATE_ABOVE гарантовано тримає
        // маскота поверх навіть повноекранних/максимізованих вікон (VS Code, браузерів тощо).
        if std::env::var("GDK_BACKEND").is_err() {
            std::env::set_var("GDK_BACKEND", "x11");
        }
    }

    local_helper_ai_lib::run();
}
