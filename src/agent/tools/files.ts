import { Type } from "@google/genai";
import type { ToolOutcome, ToolSpec } from "../types.ts";

export interface CreateFileArgs {
  filepath: string;
  content: string;
}

export const createFileTool: ToolSpec<CreateFileArgs> = {
  name: "create_or_update_file",
  uiLabel: "Створення файлу",
  risk: "write",
  idempotent: false,
  timeoutMs: 10000,
  declaration: {
    name: "create_or_update_file",
    description:
      "Створює новий файл або перезаписує існуючий файл на диску. Підтримує '~' для домашньої директорії та автоматично створює батьківські папки.",
    parameters: {
      type: Type.OBJECT,
      properties: {
        filepath: {
          type: Type.STRING,
          description:
            "Шлях до файлу (наприклад, '~/projects/demo/README.md' або '/tmp/script.sh').",
        },
        content: {
          type: Type.STRING,
          description: "Повний текстовий вміст файлу для запису.",
        },
      },
      required: ["filepath", "content"],
    },
  },
  promptRule:
    "create_or_update_file: створення або запис файлу на диску (підтримує '~/' і автоматично створює каталоги).",
  validateArgs: (raw) => {
    const rawPath = raw.filepath ?? raw.path ?? raw.file_path;
    const filepath = typeof rawPath === "string" ? rawPath.trim() : "";
    if (!filepath) {
      return { ok: false, error: "Поле 'filepath' є обов'язковим." };
    }
    const rawContent = raw.content ?? raw.text ?? "";
    const content = typeof rawContent === "string" ? rawContent : String(rawContent);
    return {
      ok: true,
      args: { filepath, content },
    };
  },
  execute: async (args, ctx): Promise<ToolOutcome> => {
    try {
      const res = await ctx.invoke<string>("create_or_update_file", {
        filepath: args.filepath,
        content: args.content,
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
        summary: `Помилка створення файлу: ${msg}`,
        data: { error: msg },
      };
    }
  },
};

export interface ReadFileArgs {
  target_path: string;
}

export const readFileTool: ToolSpec<ReadFileArgs> = {
  name: "read_file_or_dir",
  uiLabel: "Читання файлів",
  risk: "read",
  idempotent: true,
  timeoutMs: 10000,
  declaration: {
    name: "read_file_or_dir",
    description:
      "Читає вміст текстового файлу або повертає список файлів і папок у директорії. Підтримує '~' для домашньої директорії.",
    parameters: {
      type: Type.OBJECT,
      properties: {
        target_path: {
          type: Type.STRING,
          description:
            "Шлях до файлу або директорії (наприклад, '~', '~/Documents', './src').",
        },
      },
      required: ["target_path"],
    },
  },
  promptRule:
    "read_file_or_dir: перегляд вмісту текстового файлу або структури каталогу (підтримує '~/').",
  validateArgs: (raw) => {
    const rawPath = raw.target_path ?? raw.path ?? raw.file_path ?? raw.dir_path ?? ".";
    const targetPath = typeof rawPath === "string" ? rawPath.trim() : ".";
    return {
      ok: true,
      args: { target_path: targetPath || "." },
    };
  },
  execute: async (args, ctx): Promise<ToolOutcome> => {
    try {
      const res = await ctx.invoke<string>("read_file_or_dir", {
        target_path: args.target_path,
      });
      return {
        ok: true,
        retryable: true,
        summary: res,
        data: { result: res },
      };
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      return {
        ok: false,
        code: "execution_error",
        summary: `Помилка читання шляху '${args.target_path}': ${msg}`,
        data: { error: msg },
      };
    }
  },
};
