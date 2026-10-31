import { Type } from "@google/genai";
import type { ToolSpec } from "../types.ts";

export interface ShareFileWifiArgs {
  file_path: string;
}

export interface ShareSessionResponse {
  file_path: string;
  file_name: string;
  file_size: number;
  is_dir: boolean;
  ip_address: string;
  port: number;
  share_url: string;
  status: string;
}

export const shareFileWifiTool: ToolSpec<ShareFileWifiArgs> = {
  name: "share_file_wifi",
  uiLabel: "Wi-Fi Quick Share",
  risk: "exec",
  idempotent: false,
  timeoutMs: 30000,
  declaration: {
    name: "share_file_wifi",
    description:
      "Піднімає локальний HTTP-сервер для роздачі файлу або папки (у форматі ZIP) по Wi-Fi та відкриває вікно з QR-кодом для швидкого завантаження на мобільний телефон або планшет у локальній мережі.",
    parameters: {
      type: Type.OBJECT,
      properties: {
        file_path: {
          type: Type.STRING,
          description:
            "Абсолютний або локальний шлях до файлу чи папки, яку необхідно скинути на телефон.",
        },
      },
      required: ["file_path"],
    },
  },
  promptRule:
    "Використовуй `share_file_wifi`, коли користувач просить «скинь на телефон», «поділись по вайфаю», «скинути файл по wifi» або показує файл для передачі на смартфон.",
  validateArgs: (raw) => {
    const rawPath = raw.file_path;
    if (typeof rawPath !== "string" || !rawPath.trim()) {
      return {
        ok: false,
        error: "Параметр 'file_path' є обов'язковим непорожнім рядком",
      };
    }
    return {
      ok: true,
      args: {
        file_path: rawPath.trim(),
      },
    };
  },
  execute: async (args, ctx) => {
    try {
      const session = await ctx.invoke<ShareSessionResponse>("start_file_share", {
        filePath: args.file_path,
      });
      await ctx.invoke("open_share_window", {
        filePath: args.file_path,
      });
      const isHttps = session.share_url.startsWith("https://");
      const summary = isHttps
        ? `Захищений HTTPS-сервер роздачі запущено на ${session.share_url}. Вікно з QR-кодом відкрито.`
        : `Локальний сервер роздачі запущено на ${session.share_url}. Вікно з QR-кодом відкрито.`;
      return {
        ok: true,
        summary,
        data: {
          share_url: session.share_url,
          file_name: session.file_name,
          file_size: session.file_size,
        },
      };
    } catch (err) {
      const summary = `Помилка запуску роздачі: ${String(err)}`;
      return {
        ok: false,
        summary,
        code: "execution_error",
      };
    }
  },
};
