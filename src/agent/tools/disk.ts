import { Type } from "@google/genai";
import type { ToolSpec } from "../types.ts";

export interface OpenDiskAnalyzerArgs {
  path?: string;
}

export const openDiskAnalyzerTool: ToolSpec<OpenDiskAnalyzerArgs> = {
  name: "open_disk_analyzer",
  uiLabel: "Аналізатор диска",
  risk: "read",
  idempotent: true,
  timeoutMs: 15000,
  declaration: {
    name: "open_disk_analyzer",
    description:
      "Відкриває повнорозмірне вікно візуалізатора дискового простору у стилі WizTree з інтерактивною картою Treemap, деревом папок, статистикою розширень та можливістю очищення важких папок розробника (node_modules, target, кеш).",
    parameters: {
      type: Type.OBJECT,
      properties: {
        path: {
          type: Type.STRING,
          description:
            "Шлях до директорії для сканування (наприклад, '~', '~/Downloads', '/' або шлях до проєкту). Якщо не вказано, відкривається домашня папка '~'.",
        },
      },
    },
  },
  promptRule:
    "Використовуй `open_disk_analyzer`, коли користувач просить показати зайнятий дисковий простір, відкрити візуалізатор дисків / WizTree, або питає «що забило диск?».",
  validateArgs: (raw) => {
    const p = raw.path;
    if (p !== undefined && p !== null && typeof p !== "string") {
      return { ok: false, error: "Параметр 'path' має бути рядком" };
    }
    return {
      ok: true,
      args: {
        path: typeof p === "string" ? p.trim() : undefined,
      },
    };
  },
  execute: async (args, ctx) => {
    try {
      await ctx.invoke("open_disk_analyzer_window", {
        path: args.path || "~",
      });
      const summary = `Вікно WizTree успішно відкрито для директорії ${args.path || "~"}.`;
      return {
        ok: true,
        summary,
        data: { path: args.path || "~" },
      };
    } catch (err) {
      const summary = `Помилка відкриття аналізатора диска: ${String(err)}`;
      return {
        ok: false,
        summary,
        code: "execution_error",
      };
    }
  },
};
