### 🤖 Local Helper Mascot

Десктопний локальний AI-асистент та анімований маскот на базі **Tauri v2 + Gemini 3.5 Flash-Lite**.

---

### 📦 Завантаження та встановлення

#### 🐧 Linux (Ubuntu / Debian / Mint):
1. Завантажте файл `.deb` нижче в розділі **Assets**.
2. Встановіть через термінал:
```bash
sudo dpkg -i Local*.deb
sudo apt-get install -f -y  # за потреби доставити залежності
```

#### 🪟 Windows (10 / 11):
1. Завантажте інсталятор `.exe` (`Local Helper Mascot_*_x64-setup.exe`).
2. Запустіть файл та завершіть встановлення у 2 кліки.

#### 🍏 macOS (Intel / Apple Silicon):
1. Завантажте образ диска `.dmg` (`Local Helper Mascot_*.dmg`).
2. Відкрийте `.dmg` та перетягніть **Local Helper Mascot** у папку **Applications** (Програми).

---

### 🔑 Активація ключа Gemini API:
Отримайте безкоштовний ключ на [Google AI Studio](https://aistudio.google.com/) та активуйте його:
- **Linux:** виконайте у терміналі `addkey <ВАШ_GEMINI_API_KEY>` (або просто `addkey`).
- **Windows:** виконайте у PowerShell / CMD `local-helper-ai addkey <ВАШ_GEMINI_API_KEY>`.
- **macOS:** виконайте у Terminal `local-helper-ai addkey <ВАШ_GEMINI_API_KEY>`.

---

### 🛠️ Основні можливості:
- 📌 **Always on Top**: маскот завжди поруч поверх усіх відкритих вікон.
- 🎙️ **Голосове введення**: диктування задач або запитів через мікрофон.
- 📸 **Робота з фото**: вставка скриншотів через `Ctrl+V` або перетягування (drag-and-drop).
- 📋 **Obsidian Kanban**: автоматичний запис задач та дедлайнів у `TODO.md` (`@{YYYY-MM-DD}`).
- ⚙️ **CLI керування**: `local-helper-ai config`, `local-helper-ai set-model`, `local-helper-ai set-vault`.
