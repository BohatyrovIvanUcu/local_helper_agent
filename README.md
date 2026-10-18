# Local Helper AI Mascot 🤖🔨

Десктопний AI-асистент маскот на базі **Tauri v2 + React (TypeScript) + Tailwind CSS + Framer Motion**, що використовує **Gemini API (`@google/genai`)** з підтримкою Tool Calling та виконує локальні дії через Rust-команди.

![Mascot](public/mascot/idle.png)

## Можливості
- **Компактне вікно поверх усіх програм (`Always on Top`)** без системних рамок і тіней (`320x400`).
- **5 інтерактивних станів маскота** (`IDLE`, `THINKING`, `WORKING`, `SUCCESS`, `ERROR`).
- **Підтримка Obsidian Kanban**:
  - Задачі автоматично додаються в колонку `## TO DO` із дедлайнами у форматі `@{YYYY-MM-DD}` (наприклад: *«купити сухарики на вівторок»*).
  - Ідеї та пропозиції автоматично додаються в колонку `## Ideas` з таймштампом.
- **Робота з файловою системою**: створення/редагування файлів проєктів (`create_or_update_file`), читання директорій та файлів (`read_file_or_dir`).
- **CLI команди**: виконання термінальних команд (`run_cli_command`).
- **Керування вікном**:
  - Клік по маскоту відкриває/ховає рядок вводу.
  - Перетягування вікна за маскота.
  - Іконка в системному треї.

## Швидкий старт

### 1. Системні залежності (Linux / Ubuntu / Debian)
```bash
# Встановлення Rust (якщо ще не встановлено)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

# Системні бібліотеки для Tauri v2
sudo apt update && sudo apt install -y build-essential curl wget file libssl-dev libgtk-3-dev libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev
```

### 2. Встановлення пакетів Node.js
```bash
npm install
```

### 3. Налаштування оточення
Створіть файл `.env` на основі `.env.example`:
```bash
cp .env.example .env
```

Заповніть змінні у `.env`:
```dotenv
VITE_GEMINI_API_KEY=ваш_ключ_gemini_api
VITE_GEMINI_MODEL=gemini-3.8-flash
OBSIDIAN_VAULT_PATH=/шлях/до/вашого/ObsidianVault
```

### 4. Запуск у режимі розробки
```bash
npm run tauri dev
```

