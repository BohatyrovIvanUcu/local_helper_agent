import { Type } from "@google/genai";
import type { ToolOutcome, ToolSpec } from "../types.ts";

export interface ManageSessionPresetsArgs {
  action: string;
  name?: string | null;
  urls?: string[] | null;
}

export const presetsTool: ToolSpec<ManageSessionPresetsArgs> = {
  name: "manage_session_presets",
  uiLabel: "Робочі стеки",
  risk: "write",
  idempotent: false,
  timeoutMs: 12000,
  declaration: {
    name: "manage_session_presets",
    description:
      "Керує робочими сесіями («пресетами вкладок та програм»): відкриття, збереження поточної сесії, м'яке закриття та перегляд списку стеків (наприклад: навчання з UCU LMS, Teams, VS Code; робота; дизайн тощо).",
    parameters: {
      type: Type.OBJECT,
      properties: {
        action: {
          type: Type.STRING,
          description:
            "Дія: 'open' (відкрити пресет у браузері та супутні програми), 'save' (зберегти поточні вкладки браузера або вказані посилання під назвою пресету), 'close' (перевірити ресурси пресету та запитати/здійснити м'яке закриття), 'list' (показати всі збережені пресети), 'delete' (видалити пресет).",
        },
        name: {
          type: Type.STRING,
          description:
            "Назва робочого стеку / пресету (наприклад: 'study' або 'навчання', 'work' або 'робота', 'дизайн'). За замовчуванням 'study'.",
        },
        urls: {
          type: Type.ARRAY,
          items: { type: Type.STRING },
          description:
            "Необов'язковий явний список веб-посилань. Якщо не вказано при 'save', інструмент автоматично збереже поточні відкриті вкладки браузера.",
        },
      },
      required: ["action"],
    },
  },
  promptRule:
    "manage_session_presets: керування сесіями вкладок та робочими стеками (open/save/close/list/delete) для навчання чи роботи.",
  validateArgs: (raw) => {
    const rawAction = raw.action ?? "open";
    const action = typeof rawAction === "string" ? rawAction.trim().toLowerCase() : "open";
    const rawName = raw.name ?? raw.preset ?? raw.stack;
    const name = typeof rawName === "string" && rawName.trim() ? rawName.trim() : null;

    let urls: string[] | null = null;
    if (Array.isArray(raw.urls)) {
      urls = raw.urls
        .filter((u): u is string => typeof u === "string" && u.trim().length > 0)
        .map((u) => u.trim());
    }

    return {
      ok: true,
      args: { action, name, urls },
    };
  },
  execute: async (args, ctx): Promise<ToolOutcome> => {
    try {
      const res = await ctx.invoke<string>("manage_session_presets", {
        action: args.action,
        name: args.name,
        urls: args.urls,
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
        summary: `Помилка керування сесіями: ${msg}`,
        data: { error: msg },
      };
    }
  },
};
