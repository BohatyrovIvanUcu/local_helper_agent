import { Type } from "@google/genai";
import type { ToolOutcome, ToolSpec } from "../types.ts";

export interface SummarizeWebpageArgs {
  url?: string | null;
  focus?: string | null;
}

export const webTool: ToolSpec<SummarizeWebpageArgs> = {
  name: "summarize_webpage",
  uiLabel: "Аналіз вебсторінки",
  risk: "external",
  idempotent: true,
  timeoutMs: 16000,
  declaration: {
    name: "summarize_webpage",
    description:
      "Отримує зміст та чистий текст активної вкладки браузера (Chrome, Firefox, Brave тощо) або за вказаним URL/посиланням з буфера обміну для створення якісного стислого саммарі на 3–5 тез.",
    parameters: {
      type: Type.OBJECT,
      properties: {
        url: {
          type: Type.STRING,
          description:
            "Необов'язкове пряме посилання на веб-сторінку або статтю. Якщо користувач не надав URL, залиш null — інструмент автоматично визначить активну вкладку або посилання з буфера обміну.",
        },
        focus: {
          type: Type.STRING,
          description:
            "Необов'язкове конкретне питання або фокус користувача (наприклад: 'акцентуй на цінах', 'які результати тестування', 'що кажуть про реліз').",
        },
      },
    },
  },
  promptRule:
    "summarize_webpage: читання змісту активної вкладки браузера або за вказаним посиланням для створення стислого переказу.",
  validateArgs: (raw) => {
    const rawUrl = raw.url ?? raw.link;
    const url =
      typeof rawUrl === "string" && rawUrl.trim() ? rawUrl.trim() : null;
    const rawFocus = raw.focus ?? raw.topic ?? raw.query;
    const focus =
      typeof rawFocus === "string" && rawFocus.trim() ? rawFocus.trim() : null;

    return {
      ok: true,
      args: { url, focus },
    };
  },
  execute: async (args, ctx): Promise<ToolOutcome> => {
    try {
      const res = await ctx.invoke<string>("summarize_webpage", {
        url: args.url,
        focus: args.focus,
      });
      return {
        ok: true,
        summary: res,
        data: { result: res },
        untrustedContent: true,
        retryable: true,
      };
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      return {
        ok: false,
        code: "execution_error",
        summary: `Помилка отримання сторінки: ${msg}`,
        data: { error: msg },
      };
    }
  },
};
