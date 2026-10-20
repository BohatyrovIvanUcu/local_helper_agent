### 🤖 Local Helper Mascot

Десктопний анімований AI-маскот та локальний помічник на базі **Tauri v2 + Gemini 3.5 Flash-Lite**.

#### 🚀 Швидке встановлення (.deb):
```bash
sudo dpkg -i local-helper-ai_0.1.0_amd64.deb
sudo apt-get install -f -y  # за потреби доставити залежності
```

#### 🔑 Активація ключа Gemini API:
Отримайте безкоштовний ключ на [Google AI Studio](https://aistudio.google.com/) та виконайте в терміналі:
```bash
addkey <ВАШ_GEMINI_API_KEY>
```
*(або просто `addkey` для інтерактивного введення)*

#### 🛠️ Основні можливості:
- 📌 **Always on Top**: маскот завжди поруч поверх усіх програм.
- 🎙️ **Голосове введення**: надиктуйте задачу або запит голосом через мікрофон.
- 📸 **Робота з фото**: швидка вставка скриншотів через `Ctrl+V` або drag-and-drop.
- 📋 **Obsidian Kanban**: запис задач і дедлайнів у `TODO.md` (`@{YYYY-MM-DD}`).
- ⚙️ **CLI керування**: `addkey`, `local-helper-ai config`, `local-helper-ai set-model`, `local-helper-ai set-vault`.

