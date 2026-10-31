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

export interface ManageObsidianToolArgs {
  action:
    | "read"
    | "create"
    | "append"
    | "search"
    | "list"
    | "daily"
    | "complete_todo";
  filename?: string;
  subfolder?: string | null;
  content?: string;
  query?: string;
  section?: string | null;
  as_todo?: boolean;
  due_date?: string | null;
  reminder_at?: string | null;
  overwrite?: boolean;
  date?: string | null;
  search_content?: boolean;
  recursive?: boolean;
}

interface SearchResultItem {
  filename: string;
  relative_path: string;
  matches: string[];
}

interface ListEntryItem {
  name: string;
  relative_path: string;
  is_dir: boolean;
  size_bytes: number;
  modified_at: string;
}

interface DailyNoteResultItem {
  filename: string;
  relative_path: string;
  content: string;
  created_new: boolean;
}

function cleanFilename(rawFilename?: string | null, defaultName = "TODO.md"): string {
  let name = typeof rawFilename === "string" && rawFilename.trim() ? rawFilename.trim() : defaultName;
  if (name.includes("/")) {
    name = name.split("/").pop() || defaultName;
  }
  if (!name.toLowerCase().endsWith(".md")) {
    name = `${name}.md`;
  }
  return name;
}

function cleanSubfolder(rawSub?: string | null): string | null {
  const trimmed = typeof rawSub === "string" ? rawSub.trim() : "";
  if (!trimmed || ["null", "none", "undefined", "nil", ".", "/", "~"].includes(trimmed.toLowerCase())) {
    return null;
  }
  return trimmed;
}

/**
 * 1. Спеціалізований інструмент append_to_obsidian для зворотної сумісності (TODO, Kanban-колонки, дедлайни).
 */
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
      required: ["content"],
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
    const asTodo =
      raw.as_todo !== undefined
        ? Boolean(raw.as_todo)
        : raw.is_todo !== undefined
        ? Boolean(raw.is_todo)
        : true;

    const filename = cleanFilename(raw.filename as string | undefined, "TODO.md");
    const subfolder = cleanSubfolder(raw.subfolder as string | undefined);

    const rawSec = typeof raw.section === "string" ? raw.section.trim() : "";
    const section = rawSec ? rawSec.replace(/^#+\s*/, "").trim() : null;

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
      finalContent = finalContent.replace(
        /@\{?(\d{4}-\d{2}-\d{2})\}?/g,
        "@{$1}"
      );

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

/**
 * 2. Уніфікований повнофункціональний інструмент manage_obsidian:
 * Читання, створення нотаток, повнотекстовий пошук, Daily Notes, перегляд папок та закриття тасків.
 */
export const manageObsidianTool: ToolSpec<ManageObsidianToolArgs> = {
  name: "manage_obsidian",
  uiLabel: "Obsidian Vault",
  risk: "write",
  idempotent: false,
  timeoutMs: 15000,
  declaration: {
    name: "manage_obsidian",
    description:
      "Універсальний інструмент для роботи зі сховищем Obsidian: читання та створення нотаток, повнотекстовий пошук, щоденні нотатки (Daily Notes), перегляд структури папок, відмітка виконання тасків у TODO/Kanban.",
    parameters: {
      type: Type.OBJECT,
      properties: {
        action: {
          type: Type.STRING,
          description:
            "Дія: 'read' (прочитати вміст нотатки), 'create' (створити нову або перезаписати нотатку), 'append' (додати запис/таск у нотатку чи секцію), 'search' (пошук за назвою або вмістом нотаток), 'list' (список файлів та папок у Vault), 'daily' (відкрити або створити щоденну нотатку), 'complete_todo' (позначити задачу виконаною [x] у TODO.md).",
        },
        filename: {
          type: Type.STRING,
          description:
            "Назва файлу нотатки (наприклад 'TODO.md', 'Конспект', 'lab ASM'). Для append і complete_todo за замовчуванням 'TODO.md'.",
        },
        subfolder: {
          type: Type.STRING,
          description: "Необов'язкова підпапка всередині Vault (наприклад 'Work', 'Projects').",
        },
        content: {
          type: Type.STRING,
          description: "Markdown-вміст або текст для дій 'create' чи 'append'.",
        },
        query: {
          type: Type.STRING,
          description:
            "Пошуковий запит для дії 'search' або ключові слова/назва задачі для дії 'complete_todo'.",
        },
        section: {
          type: Type.STRING,
          description: "Секція/колонка для дії 'append' (наприклад 'TO DO', 'Ideas', 'IN PROGRESS').",
        },
        as_todo: {
          type: Type.BOOLEAN,
          description: "Для 'append': якщо true — додає як чекбокс '- [ ] ...'.",
        },
        due_date: {
          type: Type.STRING,
          description: "Для 'append': необов'язковий дедлайн (YYYY-MM-DD або 'завтра', 'в п'ятницю').",
        },
        overwrite: {
          type: Type.BOOLEAN,
          description: "Для 'create': якщо true — перезаписує існуючий файл з бекапом .bak (за замовчуванням false).",
        },
        date: {
          type: Type.STRING,
          description: "Для 'daily': дата щоденника (YYYY-MM-DD або відносна, наприклад 'сьогодні', 'вчора').",
        },
        search_content: {
          type: Type.BOOLEAN,
          description: "Для 'search': якщо true — шукає також по тексту всередині нотаток (за замовчуванням true).",
        },
        recursive: {
          type: Type.BOOLEAN,
          description: "Для 'list': чи переглядати підпапки рекурсивно (за замовчуванням false).",
        },
      },
      required: ["action"],
    },
  },
  promptRule:
    "manage_obsidian: універсальне керування базою знань Obsidian (read, create, append, search, list, daily, complete_todo).",
  validateArgs: (raw) => {
    const rawAction = String(raw.action ?? "").trim().toLowerCase();
    const validActions: ManageObsidianToolArgs["action"][] = [
      "read",
      "create",
      "append",
      "search",
      "list",
      "daily",
      "complete_todo",
    ];

    let action: ManageObsidianToolArgs["action"] = "read";
    if (validActions.includes(rawAction as ManageObsidianToolArgs["action"])) {
      action = rawAction as ManageObsidianToolArgs["action"];
    } else if (rawAction.includes("search") || rawAction.includes("find") || rawAction.includes("шук")) {
      action = "search";
    } else if (rawAction.includes("creat") || rawAction.includes("writ") || rawAction.includes("створ") || rawAction.includes("запис")) {
      action = "create";
    } else if (rawAction.includes("append") || rawAction.includes("дода")) {
      action = "append";
    } else if (rawAction.includes("list") || rawAction.includes("список") || rawAction.includes("файли")) {
      action = "list";
    } else if (rawAction.includes("daily") || rawAction.includes("день") || rawAction.includes("щоден")) {
      action = "daily";
    } else if (rawAction.includes("done") || rawAction.includes("complete") || rawAction.includes("викон") || rawAction.includes("закр")) {
      action = "complete_todo";
    }

    const subfolder = cleanSubfolder(raw.subfolder as string | undefined);
    const filename = raw.filename
      ? cleanFilename(raw.filename as string, action === "complete_todo" || action === "append" ? "TODO.md" : "Note.md")
      : action === "complete_todo" || action === "append"
      ? "TODO.md"
      : undefined;

    const rawContent = raw.content ?? raw.text ?? raw.markdown;
    const content = typeof rawContent === "string" ? rawContent : undefined;

    const rawQuery = raw.query ?? raw.task_query ?? raw.q ?? raw.title ?? raw.search;
    const query = typeof rawQuery === "string" ? rawQuery.trim() : undefined;

    const rawSec = typeof raw.section === "string" ? raw.section.trim() : "";
    const section = rawSec ? rawSec.replace(/^#+\s*/, "").trim() : null;

    const asTodo =
      raw.as_todo !== undefined
        ? Boolean(raw.as_todo)
        : raw.is_todo !== undefined
        ? Boolean(raw.is_todo)
        : true;

    const dueDate =
      typeof raw.due_date === "string" && raw.due_date.trim()
        ? raw.due_date.trim()
        : null;

    const overwrite = Boolean(raw.overwrite);
    const searchContent = raw.search_content !== undefined ? Boolean(raw.search_content) : true;
    const recursive = Boolean(raw.recursive);

    const rawDate = raw.date ?? raw.day;
    const date = typeof rawDate === "string" && rawDate.trim() ? rawDate.trim() : null;

    if ((action === "create" || action === "append") && (!content || !content.trim())) {
      return { ok: false, error: `Для дії '${action}' обов'язково вкажіть 'content' (текст нотатки).` };
    }

    if (action === "read" && !filename) {
      return { ok: false, error: "Для читання нотатки вкажіть 'filename' (назву файлу)." };
    }

    if ((action === "search" || action === "complete_todo") && (!query || !query.trim())) {
      return {
        ok: false,
        error: action === "search"
          ? "Для пошуку по Obsidian обов'язково вкажіть 'query' (пошуковий запит)."
          : "Для відмітки задачі вкажіть 'query' (текст або назву задачі для пошуку в TODO).",
      };
    }

    return {
      ok: true,
      args: {
        action,
        filename,
        subfolder,
        content,
        query,
        section,
        as_todo: asTodo,
        due_date: dueDate,
        overwrite,
        date,
        search_content: searchContent,
        recursive,
      },
    };
  },
  execute: async (args, ctx): Promise<ToolOutcome> => {
    try {
      switch (args.action) {
        case "read": {
          const filename = args.filename || "TODO.md";
          const res = await ctx.invoke<string>("read_obsidian_note", {
            subfolder: args.subfolder,
            filename,
          });
          return {
            ok: true,
            summary: `Прочитано нотатку '${filename}' (${res.length} символів).`,
            data: { filename, content: res },
          };
        }

        case "create": {
          const filename = args.filename || "Note.md";
          const content = args.content ?? "";
          const res = await ctx.invoke<string>("create_or_update_obsidian_note", {
            subfolder: args.subfolder,
            filename,
            content,
            overwrite: Boolean(args.overwrite),
          });
          return {
            ok: true,
            summary: res,
            data: { result: res, filename },
          };
        }

        case "append": {
          const filename = args.filename || "TODO.md";
          const normalizedDueDate = args.due_date
            ? resolveRelativeDate(args.due_date, ctx.now)
            : null;

          let finalContent = (args.content ?? "").trim();
          finalContent = finalContent.replace(
            /@\{?(\d{4}-\d{2}-\d{2})\}?/g,
            "@{$1}"
          );
          if (normalizedDueDate && !finalContent.includes(normalizedDueDate)) {
            finalContent = `${finalContent} @{${normalizedDueDate}}`;
          }

          const res = await ctx.invoke<string>("append_to_obsidian", {
            subfolder: args.subfolder,
            filename,
            content: finalContent,
            as_todo: args.as_todo ?? true,
            section: args.section,
            due_date: normalizedDueDate,
          });
          return {
            ok: true,
            summary: res,
            data: { result: res, filename },
          };
        }

        case "search": {
          const query = args.query ?? "";
          const results = await ctx.invoke<SearchResultItem[]>("search_obsidian_notes", {
            query,
            search_content: args.search_content ?? true,
            max_results: 20,
          });

          if (results.length === 0) {
            return {
              ok: true,
              summary: `У Obsidian Vault не знайдено нотаток за запитом '${query}'.`,
              data: { query, results: [] },
            };
          }

          const formatted = results
            .map((item) => {
              const snippets =
                item.matches.length > 0
                  ? `\n  - Збіги: "${item.matches.join('", "')}"`
                  : "";
              return `📄 **${item.filename}** (${item.relative_path})${snippets}`;
            })
            .join("\n");

          return {
            ok: true,
            summary: `Знайдено ${results.length} нотаток у Obsidian:\n${formatted}`,
            data: { query, results },
          };
        }

        case "list": {
          const entries = await ctx.invoke<ListEntryItem[]>("list_obsidian_notes", {
            subfolder: args.subfolder,
            recursive: Boolean(args.recursive),
          });

          if (entries.length === 0) {
            return {
              ok: true,
              summary: "У вказаній папці Obsidian немає файлів чи підпапок.",
              data: { entries: [] },
            };
          }

          const formatted = entries
            .slice(0, 30)
            .map((e) => `${e.is_dir ? "📁" : "📝"} ${e.relative_path}`)
            .join("\n");

          const countNote = entries.length > 30 ? `\n... та ще ${entries.length - 30} елементів.` : "";

          return {
            ok: true,
            summary: `Файли у сховищі Obsidian (${entries.length}):\n${formatted}${countNote}`,
            data: { entries },
          };
        }

        case "daily": {
          const normalizedDate = args.date
            ? resolveRelativeDate(args.date, ctx.now)
            : null;

          const res = await ctx.invoke<DailyNoteResultItem>("get_or_create_daily_note", {
            date: normalizedDate,
            subfolder: args.subfolder,
          });

          const statusNotice = res.created_new
            ? `Створено нову щоденну нотатку '${res.filename}'`
            : `Відкрито існуючу щоденну нотатку '${res.filename}'`;

          return {
            ok: true,
            summary: `${statusNotice} (${res.relative_path}).`,
            data: { daily_note: res },
          };
        }

        case "complete_todo": {
          const filename = args.filename || "TODO.md";
          const query = args.query ?? "";

          const res = await ctx.invoke<string>("complete_obsidian_todo", {
            subfolder: args.subfolder,
            filename,
            task_query: query,
          });

          return {
            ok: true,
            summary: res,
            data: { result: res, filename, query },
          };
        }

        default:
          return {
            ok: false,
            code: "validation_error",
            summary: `Невідома дія: ${(args as { action?: string }).action}`,
          };
      }
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      return {
        ok: false,
        code: "execution_error",
        summary: `Помилка взаємодії з Obsidian: ${msg}`,
        data: { error: msg },
      };
    }
  },
};
