import { Type } from "@google/genai";
import type { ToolOutcome, ToolSpec } from "../types.ts";

export interface MediaToolArgs {
  action: string;
  query?: string | null;
}

export const mediaTool: ToolSpec<MediaToolArgs> = {
  name: "control_media",
  uiLabel: "Музика & Медіа",
  risk: "exec",
  idempotent: false,
  timeoutMs: 10000,
  declaration: {
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
  promptRule:
    "control_media: керування музичним плеєром або пошук і запуск треків на YouTube Music чи в браузері (play/pause/next/prev/mute).",
  validateArgs: (raw) => {
    const rawAction = raw.action ?? "play";
    const action = typeof rawAction === "string" ? rawAction.trim() : "play";
    const rawQuery = raw.query;
    const query =
      typeof rawQuery === "string" && rawQuery.trim() ? rawQuery.trim() : null;

    return {
      ok: true,
      args: { action, query },
    };
  },
  execute: async (args, ctx): Promise<ToolOutcome> => {
    try {
      const res = await ctx.invoke<string>("control_media", {
        action: args.action,
        query: args.query,
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
        summary: `Помилка керування медіа: ${msg}`,
        data: { error: msg },
      };
    }
  },
};
