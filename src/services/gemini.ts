import {
  GoogleGenAI,
  Type,
  type Content,
  type FunctionDeclaration,
  type Part,
} from "@google/genai";
import { invoke } from "@tauri-apps/api/core";

export type MascotState = "IDLE" | "THINKING" | "WORKING" | "SUCCESS" | "ERROR";
export type WorkingToolName =
  | "append_to_obsidian"
  | "create_or_update_file"
  | "read_file_or_dir"
  | "run_cli_command";

export interface AppConfig {
  gemini_api_key: string;
  gemini_model: string;
  obsidian_vault_path: string;
  config_file_path: string;
}

export async function getAppConfig(): Promise<AppConfig> {
  try {
    return await invoke<AppConfig>("get_app_config");
  } catch {
    return {
      gemini_api_key: import.meta.env.VITE_GEMINI_API_KEY?.trim() || "",
      gemini_model:
        import.meta.env.VITE_GEMINI_MODEL?.trim() || "gemini-2.5-flash",
      obsidian_vault_path: "",
      config_file_path: "",
    };
  }
}

function getSystemInstruction(): string {
  const now = new Date();
  const isoDate = now.toISOString().slice(0, 10);
  const weekday = now.toLocaleDateString("uk-UA", { weekday: "long" });

  return `Ти — кмітливий десктопний маскот-будівельник у жовтій касці, локальний AI-помічник користувача.
Твій стиль: дружній, лаконічний, конкретний тон майстра-помічника без зайвої води (1–3 короткі речення).
Мова спілкування: українська.

Поточний час та дата: ${isoDate} (сьогодні: ${weekday}).
Головний файл для користувача: 'TODO.md' в Obsidian (дошка Kanban).

Колонки у файлі TODO.md:
1. "TO DO" — для поточних задач, справ, дій, дедлайнів (section: "TO DO", as_todo: true).
2. "Ideas" — для нових ідей, креативних думок, пропозицій, інсайтів на майбутнє (section: "Ideas", as_todo: true).

Коли користувач каже: "запиши ідею...", "в мене є ідея...", "додай в ідеї..." або пропонує концепт:
- ЗАВЖДИ викликай append_to_obsidian з filename: "TODO.md", section: "Ideas", as_todo: true!
- У content записуй тільки чистий текст самої ідеї БЕЗ дати, часу та часових міток (наприклад: "Зробити темну тему для інтерфейсу" або "Більше інтеракцій, анімацій та голосове введення").

Форматування дедлайнів для задач у "TO DO":
У плагіні Obsidian Kanban дедлайн позначається синтаксисом: @{YYYY-MM-DD} в кінці рядка таску.
Коли користувач вказує дату або день тижня (наприклад: "на вівторок", "на завтра", "до п'ятниці", "наступного тижня в середу"):
1. Точно обчисли дату у форматі YYYY-MM-DD відносно поточної дати (${isoDate}, ${weekday}).
2. Додай її в кінець опису таску у форматі: @{YYYY-MM-DD} (через пробіл).
3. Прибери з тексту задачі зайві слова про термін (наприклад, "Купити сухарики на вівторок" -> "Купити сухарики @{2026-10-20}").

Приклади:
- "Купити сухарики на вівторок" -> append_to_obsidian(filename: "TODO.md", section: "TO DO", content: "Купити сухарики @{2026-10-20}", as_todo: true)
- "Запиши ідею зробити темну тему" -> append_to_obsidian(filename: "TODO.md", section: "Ideas", content: "Зробити темну тему для інтерфейсу", as_todo: true)

Твої можливості (інструменти):
1. append_to_obsidian — записуй задачі (section: "TO DO") або ідеї (section: "Ideas") у сховище Obsidian.
2. create_or_update_file — створюй або оновлюй файли проєктів, скрипти, шаблони за вказаним шляхом (підтримується ~).
3. read_file_or_dir — перевіряй вміст файлів (включно з TODO.md) або список директорій перед змінами чи плануванням.
4. run_cli_command — виконуй термінальні команди, коли потрібно перевірити статус git, список процесів або запустити скрипт.

Завжди виконуй дію через відповідний інструмент, якщо запит передбачає роботу з нотатками, файлами чи терміналом, і коротко звітуй про результат.`;
}

export const toolDeclarations: FunctionDeclaration[] = [
  {
    name: "append_to_obsidian",
    description:
      "Додає нову задачу, запис або ідею у відповідну колонку файлу TODO.md в Obsidian Vault.",
    parameters: {
      type: Type.OBJECT,
      properties: {
        subfolder: {
          type: Type.STRING,
          description:
            "Необов'язкова підпапка всередині Obsidian Vault. Для основного TODO залишай null.",
        },
        filename: {
          type: Type.STRING,
          description:
            "Назва файлу нотатки. За замовчуванням 'TODO.md'.",
        },
        section: {
          type: Type.STRING,
          description:
            "Колонка/секція у Kanban: 'TO DO' для задач або 'Ideas' для ідей.",
        },
        content: {
          type: Type.STRING,
          description:
            "Текст запису, таску або ідеї. Для тасків з датою вказуй у кінці @{YYYY-MM-DD}.",
        },
        as_todo: {
          type: Type.BOOLEAN,
          description:
            "Якщо true — форматує рядок як чекбокс '- [ ] {content}'.",
        },
      },
      required: ["filename", "content", "as_todo"],
    },
  },
  {
    name: "create_or_update_file",
    description:
      "Створює новий файл або перезаписує існуючий файл на диску. Підтримує '~' для домашньої директорії та автоматично створює батьківські папки.",
    parameters: {
      type: Type.OBJECT,
      properties: {
        filepath: {
          type: Type.STRING,
          description:
            "Шлях до файлу (наприклад, '~/projects/demo/README.md' або '/tmp/script.sh').",
        },
        content: {
          type: Type.STRING,
          description: "Повний текстовий вміст файлу для запису.",
        },
      },
      required: ["filepath", "content"],
    },
  },
  {
    name: "read_file_or_dir",
    description:
      "Читає вміст текстового файлу або повертає список файлів і папок у директорії. Підтримує '~' для домашньої директорії.",
    parameters: {
      type: Type.OBJECT,
      properties: {
        target_path: {
          type: Type.STRING,
          description:
            "Шлях до файлу або директорії (наприклад, '~', '~/Documents', './src').",
        },
      },
      required: ["target_path"],
    },
  },
  {
    name: "run_cli_command",
    description:
      "Виконує shell-команду на комп'ютері користувача та повертає stdout, stderr і код завершення.",
    parameters: {
      type: Type.OBJECT,
      properties: {
        command: {
          type: Type.STRING,
          description: "Рядок shell-команди для виконання (наприклад, 'git status -s').",
        },
        working_dir: {
          type: Type.STRING,
          description:
            "Необов'язкова робоча директорія для виконання команди (підтримує '~').",
        },
      },
      required: ["command"],
    },
  },
];

async function executeLocalRustTool(
  name: string,
  args: Record<string, unknown>
): Promise<{ ok: boolean; output: string }> {
  try {
    switch (name) {
      case "append_to_obsidian": {
        const res = await invoke<string>("append_to_obsidian", {
          subfolder:
            typeof args.subfolder === "string" && args.subfolder.trim() !== ""
              ? args.subfolder
              : null,
          filename: String(args.filename ?? "TODO.md"),
          content: String(args.content ?? ""),
          as_todo: Boolean(args.as_todo),
          section:
            typeof args.section === "string" && args.section.trim() !== ""
              ? args.section.trim()
              : null,
        });
        return { ok: true, output: res };
      }

      case "create_or_update_file": {
        const res = await invoke<string>("create_or_update_file", {
          filepath: String(args.filepath ?? ""),
          content: String(args.content ?? ""),
        });
        return { ok: true, output: res };
      }

      case "read_file_or_dir": {
        const res = await invoke<string>("read_file_or_dir", {
          target_path: String(args.target_path ?? "."),
        });
        return { ok: true, output: res };
      }

      case "run_cli_command": {
        const res = await invoke<string>("run_cli_command", {
          command: String(args.command ?? ""),
          working_dir:
            typeof args.working_dir === "string" &&
            args.working_dir.trim() !== ""
              ? args.working_dir
              : null,
        });
        return { ok: true, output: res };
      }

      default:
        return {
          ok: false,
          output: `Невідомий інструмент: ${name}`,
        };
    }
  } catch (err) {
    const errorMessage = err instanceof Error ? err.message : String(err);
    return {
      ok: false,
      output: `Помилка виконання ${name}: ${errorMessage}`,
    };
  }
}

export interface MediaAttachment {
  mimeType: string;
  base64Data: string;
  previewUrl?: string;
  name?: string;
}

export interface AskMascotCallbacks {
  onStateChange: (state: MascotState, activeTool?: WorkingToolName) => void;
}

const chatHistory: Content[] = [];
const MAX_HISTORY_ITEMS = 12;

export async function askMascot(
  userPrompt: string,
  callbacks: AskMascotCallbacks,
  attachments: MediaAttachment[] = []
): Promise<string> {
  const config = await getAppConfig();
  const apiKey =
    config.gemini_api_key?.trim() ||
    import.meta.env.VITE_GEMINI_API_KEY?.trim();

  if (!apiKey) {
    throw new Error(
      "🔑 Не знайдено Gemini API ключ!\nБудь ласка, введіть у терміналі:\naddkey <ВАШ_КЛЮЧ>\nабо: local-helper-ai addkey <ВАШ_КЛЮЧ>"
    );
  }

  const effectiveModel =
    config.gemini_model?.trim() ||
    import.meta.env.VITE_GEMINI_MODEL?.trim() ||
    "gemini-2.5-flash";

  const ai = new GoogleGenAI({ apiKey });

  callbacks.onStateChange("THINKING");

  const userParts: Part[] = [];

  for (const item of attachments) {
    userParts.push({
      inlineData: {
        mimeType: item.mimeType,
        data: item.base64Data,
      },
    });
  }

  const trimmedPrompt = userPrompt.trim();
  const hasAudio = attachments.some((a) => a.mimeType.startsWith("audio/"));
  const hasImage = attachments.some((a) => a.mimeType.startsWith("image/"));

  const effectiveText =
    trimmedPrompt ||
    (hasAudio && hasImage
      ? "Прослухай голосову інструкцію, проаналізуй прикріплене зображення та виконай потрібну дію."
      : hasAudio
      ? "Уважно прослухай голосове повідомлення користувача українською мовою та виконай запитану дію (наприклад, запиши задачу чи ідею в Obsidian або дай лаконічну відповідь)."
      : hasImage
      ? "Проаналізуй це зображення. Якщо на ньому задачі, дедлайни чи ідеї — додай їх у відповідну секцію Obsidian, інакше коротко поясни суть."
      : "");

  if (effectiveText) {
    userParts.push({ text: effectiveText });
  }

  const contents: Content[] = [
    ...chatHistory,
    {
      role: "user",
      parts: userParts,
    },
  ];

  const historySummaryText = [
    hasImage ? "[Прикріплено зображення]" : "",
    hasAudio ? "[Голосове повідомлення]" : "",
    trimmedPrompt,
  ]
    .filter(Boolean)
    .join(" ");

  const MAX_TOOL_TURNS = 5;
  let turn = 0;

  while (turn < MAX_TOOL_TURNS) {
    turn += 1;

    const response = await ai.models.generateContent({
      model: effectiveModel,
      contents,
      config: {
        systemInstruction: getSystemInstruction(),
        tools: [{ functionDeclarations: toolDeclarations }],
        temperature: 0.3,
      },
    });

    const functionCalls = response.functionCalls;

    if (!functionCalls || functionCalls.length === 0) {
      const replyText =
        response.text?.trim() || "Готово! Завдання виконано.";

      chatHistory.push(
        { role: "user", parts: [{ text: historySummaryText || effectiveText }] },
        { role: "model", parts: [{ text: replyText }] }
      );
      if (chatHistory.length > MAX_HISTORY_ITEMS) {
        chatHistory.splice(0, chatHistory.length - MAX_HISTORY_ITEMS);
      }

      return replyText;
    }

    // Додаємо відповідь моделі з викликами функцій до поточного контексту
    const modelContent = response.candidates?.[0]?.content;
    if (modelContent) {
      contents.push(modelContent);
    } else {
      contents.push({
        role: "model",
        parts: functionCalls.map((fc) => ({ functionCall: fc })),
      });
    }

    const responseParts: Part[] = [];

    for (const call of functionCalls) {
      const toolName = (call.name ?? "append_to_obsidian") as WorkingToolName;
      callbacks.onStateChange("WORKING", toolName);

      const args = (call.args as Record<string, unknown>) ?? {};
      const result = await executeLocalRustTool(toolName, args);

      responseParts.push({
        functionResponse: {
          id: call.id,
          name: toolName,
          response: result.ok
            ? { result: result.output }
            : { error: result.output },
        },
      });
    }

    // Відправляємо результати Rust-інструментів назад у Gemini
    contents.push({
      role: "user",
      parts: responseParts,
    });

    callbacks.onStateChange("THINKING");
  }

  return "Виконав серію дій! Перевір результат.";
}

