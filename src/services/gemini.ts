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
  | "run_cli_command"
  | "control_media"
  | "manage_memory"
  | "manage_reminders";

export interface MemoryMessage {
  role: string;
  text: string;
  timestamp: string;
}

export interface AgentMemory {
  facts: string[];
  recent_messages: MemoryMessage[];
}

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
        import.meta.env.VITE_GEMINI_MODEL?.trim() || "gemini-3.5-flash-lite",
      obsidian_vault_path: "",
      config_file_path: "",
    };
  }
}

export async function getAgentMemory(): Promise<AgentMemory> {
  try {
    return await invoke<AgentMemory>("get_agent_memory");
  } catch {
    return { facts: [], recent_messages: [] };
  }
}

function getSystemInstruction(memoryFacts: string[] = []): string {
  const now = new Date();
  const isoDate = now.toLocaleDateString("en-CA");
  const currentTime = now.toLocaleTimeString("uk-UA", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  });
  const weekday = now.toLocaleDateString("uk-UA", { weekday: "long" });

  const factsSection =
    memoryFacts.length > 0
      ? `\nДовгострокова пам'ять про користувача та його контекст (враховуй ці факти):\n${memoryFacts
          .map((f) => `- ${f}`)
          .join("\n")}\n`
      : "";

  return `Ти — кмітливий десктопний маскот-будівельник у жовтій касці, локальний AI-помічник користувача.
Твій стиль: дружній, лаконічний, конкретний тон майстра-помічника без зайвої води (1–3 короткі речення).
Мова спілкування: українська.

Поточний точний час та дата: ${isoDate} ${currentTime} (сьогодні: ${weekday}).
Головний файл для користувача: 'TODO.md' в Obsidian (дошка Kanban).
${factsSection}
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
5. control_media — керуй відтворенням музики (play, pause, next, prev, liked_music, my_mix).
6. manage_memory — зберігай або видаляй важливі факти у своїй довгостроковій пам'яті між перезапусками.
7. manage_reminders — встановлюй таймери та плануй нагадування на певний час, показуй список або скасовуй їх.

Нагадування та таймери (manage_reminders):
Коли користувач каже: "нагадай через 10 хвилин...", "постав таймер на 15 хв...", "нагадай завтра о 10:00 про...", "які в мене нагадування?", "скасуй нагадування...":
- ЗАВЖДИ викликай інструмент 'manage_reminders'!
- Якщо вказано відносний час ("через 15 хвилин", "через годину", "таймер на 5 хвилин"):
  обчисли кількість секунд у delay_seconds (наприклад, 5 хв = 300, 15 хв = 900, 1 год = 3600) і виклич manage_reminders(action: "create", text: "про що нагадати", delay_seconds: кількість_секунд).
- Якщо вказано точний час або день ("завтра о 10:00", "сьогодні о 19:30", "в п'ятницю о 14:00"):
  точно обчисли дату й час відносно поточної дати (${isoDate}, ${weekday}) у форматі YYYY-MM-DDTHH:MM:SS і виклич manage_reminders(action: "create", text: "про що нагадати", target_time: "YYYY-MM-DDTHH:MM:SS").
- Якщо користувач питає "які в мене є нагадування?", "покажи таймери":
  виклич manage_reminders(action: "list").
- Якщо користувач просить скасувати нагадування ("скасуй нагадування про чайник"):
  виклич manage_reminders(action: "cancel", text: "ключове слово або all").

Робота з пам'яттю (manage_memory):
- Якщо користувач ділиться важливим фактом про себе, свої проєкти, шляхи до робочих папок, університет, улюблені сервіси/браузер або прямо каже "запам'ятай..." — виклич manage_memory(action: "remember", fact: "короткий факт 1 реченням").
- Якщо користувач просить забути щось конкретне — виклич manage_memory(action: "forget", fact: "ключові слова").
- Якщо користувач просить повністю очистити пам'ять або історію діалогу — виклич manage_memory(action: "clear_all").

Керування музикою та медіа:
Коли користувач каже: "включи музику", "грай", "постав пісню", "включи мої лайки", "включи мій плейлист", "улюблена музика", "рекомендації", "пауза", "зупини музику", "продовж", "наступний трек", "попередня пісня":
- ЗАВЖДИ викликай інструмент 'control_media'!
- КАТЕГОРИЧНО НЕ використовуй run_cli_command з bare посиланням на головну сторінку на кшталт 'xdg-open https://music.youtube.com', бо це лише відкриває порожню головну сторінку без відтворення!
- Якщо користувач просить увімкнути музику без назви ("включи музику", "включи мою музику", "мої лайки", "улюблені треки", "мій плейлист"):
  викликай control_media(action: "play", query: "liked_music"). Цей інструмент або продовжить відтворення вже відкритого у браузері треку, або одразу увімкне особистий плейлист уподобань користувача (Liked Music).
- Якщо користувач просить персональні рекомендації чи мікс ("включи рекомендації", "новинки", "мій мікс"):
  викликай control_media(action: "play", query: "my_mix").
- Якщо користувач вказує назву пісні, виконавця чи жанр (наприклад, "включи Did It First", "включи Скрябіна", "включи рок"):
  викликай control_media(action: "play", query: "назва пісні чи виконавець"). Інструмент знайде прямий трек і одразу запустить його відтворення.
- Для паузи ("зупини", "пауза"): викликай control_media(action: "pause").
- Для наступного треку: викликай control_media(action: "next").
- Для попереднього треку: викликай control_media(action: "prev").

Завжди виконуй дію через відповідний інструмент, якщо запит передбачає роботу з нотатками, файлами, терміналом, пам'яттю або музикою, і коротко звітуй про результат.`;
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
  {
    name: "control_media",
    description:
      "Керує відтворенням музики та мультимедіа на комп'ютері. Може натискати Play/Pause на активному плеєрі (включно з YouTube Music у браузері чи Spotify) або знаходити та вмикати конкретний трек чи жанр одразу з відтворенням.",
    parameters: {
      type: Type.OBJECT,
      properties: {
        action: {
          type: Type.STRING,
          description:
            "Дія з відтворенням: 'play' (увімкнути або продовжити), 'pause' (поставити на паузу), 'next' (наступний трек), 'prev' (попередній трек).",
        },
        query: {
          type: Type.STRING,
          description:
            "Необов'язкова назва пісні, виконавця чи жанру (наприклад: 'Did It First', 'Скрябін', 'lofi beats'). Залиш null, якщо користувач просто сказав 'включи музику'.",
        },
      },
      required: ["action"],
    },
  },
  {
    name: "manage_memory",
    description:
      "Керує довгостроковою пам'яттю маскота про користувача, його вподобання, проєкти та контекст між перезапусками.",
    parameters: {
      type: Type.OBJECT,
      properties: {
        action: {
          type: Type.STRING,
          description:
            "Дія: 'remember' (зберегти новий факт у пам'ять), 'forget' (видалити факт за ключовим словом), 'clear_all' (повністю очистити пам'ять та історію розмов), 'list' (показати збережені факти).",
        },
        fact: {
          type: Type.STRING,
          description:
            "Короткий факт (1 речення) для запам'ятовування (при action='remember') або ключове слово для видалення (при action='forget').",
        },
      },
      required: ["action"],
    },
  },
  {
    name: "manage_reminders",
    description:
      "Встановлює, показує або скасовує нагадування та таймери. Працює локально та зберігається між перезапусками.",
    parameters: {
      type: Type.OBJECT,
      properties: {
        action: {
          type: Type.STRING,
          description:
            "Дія: 'create' (створити нове нагадування/таймер), 'list' (показати активні), 'cancel' (скасувати).",
        },
        text: {
          type: Type.STRING,
          description:
            "Текст нагадування (про що саме нагадати) або ключове слово для скасування.",
        },
        delay_seconds: {
          type: Type.INTEGER,
          description:
            "Для відносних таймерів: кількість секунд від поточного моменту (наприклад: 300 для 5 хв, 900 для 15 хв, 3600 для 1 год).",
        },
        target_time: {
          type: Type.STRING,
          description:
            "Для точного часу: дата й час у форматі ISO 8601 YYYY-MM-DDTHH:MM:SS (наприклад, 2026-10-22T10:00:00).",
        },
      },
      required: ["action"],
    },
  },
];

async function executeLocalRustToolWithTimeout(
  name: string,
  args: Record<string, unknown>,
  timeoutMs = 10000
): Promise<{ ok: boolean; output: string }> {
  let timer: number | undefined;
  const timeoutPromise = new Promise<{ ok: boolean; output: string }>((resolve) => {
    timer = window.setTimeout(() => {
      resolve({
        ok: false,
        output: `Час очікування інструменту ${name} вичерпано (${timeoutMs / 1000} с).`,
      });
    }, timeoutMs);
  });

  try {
    return await Promise.race([
      executeLocalRustTool(name, args),
      timeoutPromise,
    ]);
  } finally {
    if (timer) window.clearTimeout(timer);
  }
}

async function executeLocalRustTool(
  name: string,
  args: Record<string, unknown>
): Promise<{ ok: boolean; output: string }> {
  try {
    switch (name) {
      case "manage_reminders": {
        const rawDelay = args.delay_seconds ?? args.delaySeconds ?? args.seconds;
        const rawTargetTime = args.target_time ?? args.targetTime ?? args.time;
        const delaySeconds =
          typeof rawDelay === "number"
            ? Math.round(rawDelay)
            : typeof rawDelay === "string" && !isNaN(Number(rawDelay))
            ? Math.round(Number(rawDelay))
            : null;
        const targetTime =
          typeof rawTargetTime === "string" ? rawTargetTime.trim() : null;

        const res = await invoke<string>("manage_reminders", {
          action: String(args.action || "create"),
          text: typeof args.text === "string" ? args.text.trim() : null,
          delay_seconds: delaySeconds,
          target_time: targetTime,
          delaySeconds: delaySeconds,
          targetTime: targetTime,
        });
        return { ok: true, output: res };
      }
      case "manage_memory": {
        const res = await invoke<string>("manage_memory", {
          action: String(args.action || "remember"),
          fact:
            typeof args.fact === "string" && args.fact.trim() !== ""
              ? args.fact.trim()
              : null,
        });
        return { ok: true, output: res };
      }
      case "control_media": {
        const res = await invoke<string>("control_media", {
          action: String(args.action || "play"),
          query:
            typeof args.query === "string" && args.query.trim() !== ""
              ? args.query.trim()
              : null,
        });
        return { ok: true, output: res };
      }
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

export async function askMascot(
  userPrompt: string,
  callbacks: AskMascotCallbacks,
  attachments: MediaAttachment[] = []
): Promise<string> {
  const [config, memory] = await Promise.all([
    getAppConfig(),
    getAgentMemory(),
  ]);

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
    "gemini-3.5-flash-lite";

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

  const persistedHistory: Content[] = memory.recent_messages.map((msg) => ({
    role: msg.role === "model" ? "model" : "user",
    parts: [{ text: msg.text }],
  }));

  const contents: Content[] = [
    ...persistedHistory,
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

  let currentFacts = [...memory.facts];
  const executedToolsLog: string[] = [];
  let memoryWasCleared = false;

  const MAX_TOOL_TURNS = 5;
  let turn = 0;

  while (turn < MAX_TOOL_TURNS) {
    turn += 1;

    const response = await ai.models.generateContent({
      model: effectiveModel,
      contents,
      config: {
        systemInstruction: getSystemInstruction(currentFacts),
        tools: [{ functionDeclarations: toolDeclarations }],
        temperature: 0.3,
      },
    });

    const functionCalls = response.functionCalls;

    if (!functionCalls || functionCalls.length === 0) {
      const replyText =
        response.text?.trim() || "Готово! Завдання виконано.";

      if (!memoryWasCleared) {
        const toolPrefix =
          executedToolsLog.length > 0
            ? `[Виконано: ${executedToolsLog.join(", ")}] `
            : "";
        try {
          await invoke("append_conversation_turn", {
            userText: historySummaryText || effectiveText,
            modelText: `${toolPrefix}${replyText}`,
          });
        } catch {
          // Ігноруємо помилку запису у фоновому режимі
        }
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
      const result = await executeLocalRustToolWithTimeout(toolName, args);

      executedToolsLog.push(toolName);
      if (toolName === "manage_memory") {
        const act = String(args.action || "").toLowerCase();
        if (act === "clear_all" || act === "clear") {
          memoryWasCleared = true;
          currentFacts = [];
        } else {
          const updatedMem = await getAgentMemory();
          currentFacts = updatedMem.facts;
        }
      }

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

  const fallbackReply = "Виконав серію дій! Перевір результат.";
  if (!memoryWasCleared) {
    try {
      await invoke("append_conversation_turn", {
        userText: historySummaryText || effectiveText,
        modelText: fallbackReply,
      });
    } catch {
      // Ігноруємо помилку запису
    }
  }

  return fallbackReply;
}

