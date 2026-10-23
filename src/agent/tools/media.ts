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
      "Керує відтворенням музики та системною гучністю звуку на комп'ютері. Може натискати Play/Pause на активному плеєрі, знаходити треки, а також регулювати системну гучність (голосніше, тихіше, встановити 0-100%, mute, unmute).",
    parameters: {
      type: Type.OBJECT,
      properties: {
        action: {
          type: Type.STRING,
          description:
            "Дія з відтворенням або звуком: 'play', 'pause', 'next', 'prev', 'volume_up', 'volume_down', 'set_volume', 'mute', 'unmute', 'get_volume'.",
        },
        query: {
          type: Type.STRING,
          description:
            "Необов'язкова назва пісні, виконавця, жанру або відсоток гучності (наприклад: '0', '50', 'Скрябін', 'lofi beats').",
        },
      },
      required: ["action"],
    },
  },
  promptRule:
    "control_media: керування музичним плеєром та системною гучністю звуку (play, pause, next, prev, volume_up, volume_down, set_volume, mute, unmute). Будь-який запит на запуск музики ('дай музики', 'включи музику', 'play'), зупинку ('пауза', 'офни музику', 'стоп'), перемикання треків ('наступний', 'next', 'next song', 'skip', 'попередній', 'prev') чи зміну гучності — ЗАВЖДИ викликай control_media.",
  validateArgs: (raw) => {
    const rawAction = raw.action ?? "play";
    const action = typeof rawAction === "string" ? rawAction.trim() : "play";
    const rawQuery = raw.query;
    let query =
      typeof rawQuery === "string" && rawQuery.trim()
        ? rawQuery.trim()
        : typeof rawQuery === "number"
        ? String(rawQuery)
        : null;

    if (query && action !== "set_volume") {
      const lower = query.toLowerCase();
      if (
        lower.includes("лайк") ||
        lower.includes("улюблен") ||
        lower.includes("люблю") ||
        lower.includes("мої треки") ||
        lower.includes("мої пісні") ||
        lower.includes("моя музика") ||
        lower.includes("мій плейлист") ||
        lower === "liked" ||
        lower === "favorites"
      ) {
        query = "liked";
      } else if (
        [
          "музика",
          "музику",
          "музики",
          "музичко",
          "music",
          "track",
          "трек",
          "пісня",
          "пісню",
          "song",
          "play",
          "плей",
        ].includes(lower)
      ) {
        query = null;
      }
    }

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
