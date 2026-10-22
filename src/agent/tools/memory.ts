import { Type } from "@google/genai";
import type { ToolOutcome, ToolSpec } from "../types.ts";

export interface MemoryToolArgs {
  action: string;
  fact?: string | null;
}

export const memoryTool: ToolSpec<MemoryToolArgs> = {
  name: "manage_memory",
  uiLabel: "Пам'ять",
  risk: "write",
  idempotent: false,
  timeoutMs: 10000,
  declaration: {
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
  promptRule:
    "manage_memory: збереження стабільних фактів про користувача або їх видалення (remember/forget/clear_all/list).",
  validateArgs: (raw) => {
    const rawAction = raw.action ?? "remember";
    const action = typeof rawAction === "string" ? rawAction.trim() : "remember";
    const rawFact = raw.fact ?? raw.text;
    const fact =
      typeof rawFact === "string" && rawFact.trim() ? rawFact.trim() : null;

    return {
      ok: true,
      args: { action, fact },
    };
  },
  execute: async (args, ctx): Promise<ToolOutcome> => {
    try {
      const res = await ctx.invoke<string>("manage_memory", {
        action: args.action,
        fact: args.fact,
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
        summary: `Помилка керування пам'яттю: ${msg}`,
        data: { error: msg },
      };
    }
  },
};
