import { Type } from "@google/genai";
import type { ToolOutcome, ToolSpec } from "../types.ts";

export interface CliCommandArgs {
  command: string;
  working_dir?: string | null;
}

export const cliTool: ToolSpec<CliCommandArgs> = {
  name: "run_cli_command",
  uiLabel: "Термінал",
  risk: "exec",
  idempotent: false,
  timeoutMs: 15000,
  declaration: {
    name: "run_cli_command",
    description:
      "Виконує shell-команду на комп'ютері користувача та повертає stdout, stderr і код завершення.",
    parameters: {
      type: Type.OBJECT,
      properties: {
        command: {
          type: Type.STRING,
          description: "Рядок shell-команди для виконання (наприклад, 'git status -s').",
        },
        working_dir: {
          type: Type.STRING,
          description:
            "Необов'язкова робоча директорія для виконання команди (підтримує '~').",
        },
      },
      required: ["command"],
    },
  },
  promptRule:
    "run_cli_command: виконання безпечних команд у терміналі (читання статусу системи, git, відкриття браузера через xdg-open/open/firefox чи запуск скриптів).",
  validateArgs: (raw) => {
    const rawCmd = raw.command ?? raw.cmd;
    const command = typeof rawCmd === "string" ? rawCmd.trim() : "";
    if (!command) {
      return { ok: false, error: "Поле 'command' не може бути порожнім." };
    }
    const rawDir = raw.working_dir ?? raw.cwd ?? raw.dir;
    const workingDir =
      typeof rawDir === "string" && rawDir.trim() ? rawDir.trim() : null;

    return {
      ok: true,
      args: { command, working_dir: workingDir },
    };
  },
  execute: async (args, ctx): Promise<ToolOutcome> => {
    try {
      const targetDir = args.working_dir || ctx.activeWorkspacePath || null;
      const res = await ctx.invoke<string>("run_cli_command", {
        command: args.command,
        working_dir: targetDir,
      });

      const exitCodeMatch = res.match(/Exit code:\s*(\d+|timeout)/i);
      const isTimeout = exitCodeMatch && exitCodeMatch[1].toLowerCase() === "timeout";
      const exitCode = exitCodeMatch && !isTimeout ? parseInt(exitCodeMatch[1], 10) : (isTimeout ? 124 : 0);
      const isOk = !isTimeout && exitCode === 0;

      return {
        ok: isOk,
        code: isOk ? undefined : (isTimeout ? "timeout" : "execution_error"),
        summary: res,
        data: { result: res, exitCode },
      };
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      return {
        ok: false,
        code: "execution_error",
        summary: `Помилка виконання команди: ${msg}`,
        data: { error: msg },
      };
    }
  },
};
