import { Type } from "@google/genai";
import type { ToolOutcome, ToolSpec } from "../types.ts";
import { resolveReminderTime } from "../datetime.ts";

export interface RemindersToolArgs {
  action: string;
  text?: string | null;
  delay_seconds?: number | null;
  target_time?: string | null;
}

export const remindersTool: ToolSpec<RemindersToolArgs> = {
  name: "manage_reminders",
  uiLabel: "Нагадування & Таймери",
  risk: "write",
  idempotent: false,
  timeoutMs: 10000,
  declaration: {
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
  promptRule:
    "manage_reminders: створення та перегляд локальних нагадувань або таймерів (create/list/cancel).",
  validateArgs: (raw) => {
    const rawAction = raw.action ?? "create";
    const action = typeof rawAction === "string" ? rawAction.trim() : "create";
    const rawText = raw.text ?? raw.content ?? raw.title;
    const text =
      typeof rawText === "string" && rawText.trim() ? rawText.trim() : null;

    const rawDelay = raw.delay_seconds ?? raw.delaySeconds ?? raw.seconds;
    const delaySeconds =
      typeof rawDelay === "number"
        ? Math.round(rawDelay)
        : typeof rawDelay === "string" && !isNaN(Number(rawDelay))
        ? Math.round(Number(rawDelay))
        : null;

    const rawTargetTime = raw.target_time ?? raw.targetTime ?? raw.time;
    const targetTime =
      typeof rawTargetTime === "string" && rawTargetTime.trim()
        ? rawTargetTime.trim()
        : null;

    return {
      ok: true,
      args: {
        action,
        text,
        delay_seconds: delaySeconds,
        target_time: targetTime,
      },
    };
  },
  execute: async (args, ctx): Promise<ToolOutcome> => {
    try {
      let delaySeconds = args.delay_seconds;
      let targetTime = args.target_time;

      if (targetTime) {
        const resolved = resolveReminderTime({ remindAt: targetTime }, ctx.now);
        if (resolved.isoLocal) {
          targetTime = resolved.isoLocal;
        }
      }

      const res = await ctx.invoke<string>("manage_reminders", {
        action: args.action,
        text: args.text,
        delay_seconds: delaySeconds,
        target_time: targetTime,
        delaySeconds: delaySeconds,
        targetTime: targetTime,
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
        summary: `Помилка нагадувань: ${msg}`,
        data: { error: msg },
      };
    }
  },
};
