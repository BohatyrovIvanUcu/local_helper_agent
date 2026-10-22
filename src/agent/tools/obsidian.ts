import { Type } from "@google/genai";
import type { ToolOutcome, ToolSpec } from "../types.ts";
import { resolveRelativeDate, resolveReminderTime } from "../datetime.ts";

export interface ObsidianToolArgs {
  subfolder?: string | null;
  filename?: string;
  section?: string | null;
  content: string;
  as_todo: boolean;
  due_date?: string | null;
  reminder_at?: string | null;
}

export const obsidianTool: ToolSpec<ObsidianToolArgs> = {
  name: "append_to_obsidian",
  uiLabel: "Obsidian TODO",
  risk: "write",
  idempotent: false,
  timeoutMs: 10000,
  declaration: {
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
          description: "Назва файлу нотатки. За замовчуванням 'TODO.md'.",
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
        due_date: {
          type: Type.STRING,
          description:
            "Необов'язкова дата дедлайну (YYYY-MM-DD або відносна, наприклад 'завтра', 'в середу').",
        },
        reminder_at: {
          type: Type.STRING,
          description:
            "Необов'язковий точний час нагадування (ISO або відносний, наприклад '18:00', 'завтра 09:00').",
        },
      },
      required: ["filename", "content", "as_todo"],
    },
  },
  promptRule:
    "append_to_obsidian: додавання нотаток, ідей та завдань у TODO.md (секції 'TO DO', 'IN PROGRESS', 'DONE', 'Ideas', дедлайн @{YYYY-MM-DD}).",
  validateArgs: (raw) => {
    const rawContent = raw.content ?? raw.text ?? raw.note;
    const content = typeof rawContent === "string" ? rawContent.trim() : "";
    if (!content) {
      return { ok: false, error: "Поле 'content' не може бути порожнім." };
    }
    const asTodo = Boolean(raw.as_todo ?? raw.is_todo ?? false);

    let filename =
      typeof raw.filename === "string" && raw.filename.trim()
        ? raw.filename.trim()
        : "TODO.md";
    if (filename.includes("/")) {
      filename = filename.split("/").pop() || "TODO.md";
    }
    if (!filename.toLowerCase().endsWith(".md")) {
      filename = `${filename}.md`;
    }

    const rawSub =
      typeof raw.subfolder === "string" ? raw.subfolder.trim() : "";
    const subfolder =
      rawSub &&
      !["null", "none", "undefined", "nil", ".", "/", "~"].includes(
        rawSub.toLowerCase()
      )
        ? rawSub
        : null;

    const rawSec =
      typeof raw.section === "string" ? raw.section.trim() : "";
    const section =
      rawSec
        ? rawSec.replace(/^#+\s*/, "").trim()
        : null;

    const dueDate =
      typeof raw.due_date === "string" && raw.due_date.trim()
        ? raw.due_date.trim()
        : null;
    const reminderAt =
      typeof raw.reminder_at === "string" && raw.reminder_at.trim()
        ? raw.reminder_at.trim()
        : null;

    return {
      ok: true,
      args: {
        content,
        as_todo: asTodo,
        filename,
        subfolder,
        section,
        due_date: dueDate,
        reminder_at: reminderAt,
      },
    };
  },
  execute: async (args, ctx): Promise<ToolOutcome> => {
    try {
      const normalizedDueDate = args.due_date
        ? resolveRelativeDate(args.due_date, ctx.now)
        : null;
      const normalizedReminderAt = args.reminder_at
        ? resolveReminderTime({ remindAt: args.reminder_at }, ctx.now).isoLocal
        : null;

      let finalContent = args.content.trim();

      // Нормалізуємо голі дати з собачкою або фігурними дужками до стандарту Obsidian Kanban: @{YYYY-MM-DD}
      finalContent = finalContent.replace(
        /@\{?(\d{4}-\d{2}-\d{2})\}?/g,
        "@{$1}"
      );

      // Якщо вказано дедлайн і його ще немає у тексті завдання — додаємо в кінці
      if (normalizedDueDate && !finalContent.includes(normalizedDueDate)) {
        finalContent = `${finalContent} @{${normalizedDueDate}}`;
      }

      const res = await ctx.invoke<string>("append_to_obsidian", {
        subfolder: args.subfolder,
        filename: args.filename ?? "TODO.md",
        content: finalContent,
        as_todo: args.as_todo,
        section: args.section,
        due_date: normalizedDueDate,
        reminder_at: normalizedReminderAt,
      });
      return {
        ok: true,
        summary: res,
        data: { result: res },
      };
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      return {
        ok: false,
        code: "execution_error",
        summary: `Помилка запису в Obsidian: ${msg}`,
        data: { error: msg },
      };
    }
  },
};
