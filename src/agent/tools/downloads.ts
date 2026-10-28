import { Type } from "@google/genai";
import type { ToolSpec } from "../types.ts";

export interface CleanDownloadsArgs {
  execute_cleanup?: boolean;
}

export interface DownloadFileItem {
  name: string;
  path: string;
  size: number;
  mtime_secs: number;
  category: string;
  is_kept_recent: boolean;
  is_junk: boolean;
  junk_reason?: string;
}

export interface CategoryPlan {
  category_name: string;
  files: DownloadFileItem[];
  total_size: number;
}

export interface DownloadsAnalysisResult {
  downloads_path: string;
  total_files_count: number;
  total_size: number;
  kept_recent_files: DownloadFileItem[];
  categories: CategoryPlan[];
  junk_candidates: DownloadFileItem[];
}

export interface CleanupExecutionResult {
  moved_count: number;
  deleted_count: number;
  freed_bytes: number;
  message: string;
}

function formatBytes(bytes: number): string {
  if (!bytes || bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`;
}

export const cleanDownloadsTool: ToolSpec<CleanDownloadsArgs> = {
  name: "clean_downloads",
  uiLabel: "Прибирання завантажень",
  risk: "exec",
  idempotent: false,
  timeoutMs: 30000,
  declaration: {
    name: "clean_downloads",
    description:
      "Аналізує папку ~/Downloads, сортує файли за категоріями (Документи, Зображення, Відео та Аудіо, Архіви, Інсталятори, Код та Дані), суворо зберігає останні 5 найновіших завантажених файлів недоторканими в корені, та пропонує очистити виявлений мотлох (.crdownload, дублікати (1), старі .deb).",
    parameters: {
      type: Type.OBJECT,
      properties: {
        execute_cleanup: {
          type: Type.BOOLEAN,
          description:
            "Якщо true — автоматично виконує переміщення файлів по категоріях та очищення мотлоху. Якщо false (за замовчуванням) — повертає детальний план-аналіз для підтвердження користувачем.",
        },
      },
    },
  },
  promptRule:
    "Використовуй `clean_downloads`, коли користувач просить навести лад у завантаженнях, поприбирати папку Downloads або видалити старий мотлох.",
  validateArgs: (raw) => {
    const exec = raw.execute_cleanup;
    return {
      ok: true,
      args: {
        execute_cleanup: typeof exec === "boolean" ? exec : undefined,
      },
    };
  },
  execute: async (args, ctx) => {
    try {
      const analysis = await ctx.invoke<DownloadsAnalysisResult>("analyze_downloads");

      if (args.execute_cleanup) {
        const junkPaths = analysis.junk_candidates.map((j) => j.path);
        const execRes = await ctx.invoke<CleanupExecutionResult>(
          "execute_downloads_cleanup",
          {
            plan: {
              move_categories: true,
              delete_junk_paths: junkPaths,
            },
          }
        );
        return {
          ok: true,
          summary: execRes.message,
          data: {
            moved_count: execRes.moved_count,
            deleted_count: execRes.deleted_count,
            freed_bytes: execRes.freed_bytes,
          },
        };
      }

      const keptNames = analysis.kept_recent_files.map((f) => f.name).join(", ");
      const categoriesSummary = analysis.categories
        .map((c) => `${c.category_name}: ${c.files.length} файлів (${formatBytes(c.total_size)})`)
        .join("; ");
      const junkSummary =
        analysis.junk_candidates.length > 0
          ? `${analysis.junk_candidates.length} файлів мотлоху (${analysis.junk_candidates.map((j) => j.name).join(", ")})`
          : "немає";

      const summary = `Знайдено в ~/Downloads: ${analysis.total_files_count} файлів (${formatBytes(analysis.total_size)}).
Збережено в корені (5 найновіших): ${keptNames || "немає"}.
Категорії для сортування: ${categoriesSummary || "немає файлів для переміщення"}.
Кандидати на видалення: ${junkSummary}.`;

      return {
        ok: true,
        summary,
        data: {
          total_files_count: analysis.total_files_count,
          total_size: analysis.total_size,
          kept_recent_files_count: analysis.kept_recent_files.length,
          junk_candidates_count: analysis.junk_candidates.length,
        },
      };
    } catch (err) {
      const summary = `Помилка аналізу/прибирання завантажень: ${String(err)}`;
      return {
        ok: false,
        summary,
        code: "execution_error",
      };
    }
  },
};
