import { Type } from "@google/genai";
import type { ToolSpec } from "../types.ts";

export interface ConvertFileArgs {
  input_path: string;
  target_format: string;
  custom_output_path?: string | null;
}

export interface BackendConversionResult {
  success: boolean;
  input_path: string;
  output_path: string;
  output_filename: string;
  output_size_bytes: number;
  engine_used: string;
  message: string;
}

function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`;
}

export const convertFileTool: ToolSpec<ConvertFileArgs> = {
  name: "convert_file",
  uiLabel: "Конвертер файлів",
  risk: "exec",
  idempotent: false,
  timeoutMs: 60000,
  declaration: {
    name: "convert_file",
    description:
      "Універсальний локальний конвертер файлів та папок (за принципом Filee). Підтримує перетворення документів та таблиць у Markdown через Microsoft MarkItDown (docx/xlsx/pptx/pdf -> md), створення GIF з відео (mp4->gif), вилучення аудіо (mp4->mp3/wav), транскодування відео та аудіо, конвертацію зображень (png/jpg/webp/ico/bmp), експорт нотаток у PDF, вилучення тексту з PDF (pdf->txt), архівацію папок та файлів у ZIP чи TAR.GZ, а також розархівацію архівів (zip/tar.gz/7z/rar -> unzip/extract).",
    parameters: {
      type: Type.OBJECT,
      properties: {
        input_path: {
          type: Type.STRING,
          description:
            "Абсолютний або локальний шлях до вхідного файлу чи директорії, які необхідно конвертувати або розархівувати.",
        },
        target_format: {
          type: Type.STRING,
          description:
            "Цільовий формат без крапки (наприклад: 'gif', 'mp3', 'wav', 'mp4', 'webp', 'png', 'jpg', 'ico', 'pdf', 'zip', 'tar.gz', 'unzip', 'extract').",
        },
        custom_output_path: {
          type: Type.STRING,
          description:
            "Необов'язковий бажаний шлях або ім'я для вихідного файлу чи папки.",
        },
      },
      required: ["input_path", "target_format"],
    },
  },
  promptRule:
    "convert_file: локальна конвертація файлів та папок, архівація та розархівація (docx/xlsx/pptx/pdf -> md через markitdown, відео у gif/mp3, картинки, pdf, txt, zip/tar, розархівація архівів unzip/extract). Коли користувач просить 'конвертуй', 'зроби gif', 'витягни звук', 'зроби mp3', 'перетвори на pdf', 'запакуй в zip', 'розархівуй', 'розпакуй', 'видобудь з архіву', 'unzip' — викликай convert_file.",
  validateArgs: (raw) => {
    const rawInput = raw.input_path;
    const rawTarget = raw.target_format;
    if (typeof rawInput !== "string" || !rawInput.trim()) {
      return { ok: false, error: "Параметр 'input_path' є обов'язковим непорожнім рядком" };
    }
    if (typeof rawTarget !== "string" || !rawTarget.trim()) {
      return { ok: false, error: "Параметр 'target_format' є обов'язковим непорожнім рядком" };
    }

    const custom =
      typeof raw.custom_output_path === "string" && raw.custom_output_path.trim()
        ? raw.custom_output_path.trim()
        : null;

    return {
      ok: true,
      args: {
        input_path: rawInput.trim(),
        target_format: rawTarget.trim().replace(/^\.+/, "").toLowerCase(),
        custom_output_path: custom,
      },
    };
  },
  execute: async (args, ctx) => {
    try {
      const res = await ctx.invoke<BackendConversionResult>("convert_file", {
        input_path: args.input_path,
        target_format: args.target_format,
        custom_output_path: args.custom_output_path ?? null,
      });

      const sizeStr = formatBytes(res.output_size_bytes);
      const isExtract =
        args.target_format === "unzip" ||
        args.target_format === "extract" ||
        args.target_format === "unarchive";
      const encodedPath = encodeURIComponent(res.output_path);
      const summary = isExtract
        ? `Готово! Архів розархівовано в папку «${res.output_filename}» (${sizeStr}) через ${res.engine_used}. [Відкрити папку](action://open?path=${encodedPath}) • [У файловому менеджері](action://reveal?path=${encodedPath})`
        : `Готово! Створено ${res.output_filename} (${sizeStr}) через ${res.engine_used}. [Відкрити файл](action://open?path=${encodedPath}) • [У папці](action://reveal?path=${encodedPath})`;

      return {
        ok: true,
        summary,
        data: {
          output_path: res.output_path,
          output_filename: res.output_filename,
          output_size_bytes: res.output_size_bytes,
          engine_used: res.engine_used,
        },
      };
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      return {
        ok: false,
        code: "execution_error",
        summary: `Помилка конвертації: ${msg}`,
      };
    }
  },
};
