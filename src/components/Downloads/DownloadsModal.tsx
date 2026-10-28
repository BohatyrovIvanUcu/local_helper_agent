import React, { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Archive,
  Check,
  Code,
  FileText,
  Film,
  FolderCheck,
  ImageIcon,
  Package,
  Sparkles,
  Trash2,
  X,
} from "lucide-react";

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

function getCategoryIcon(cat: string) {
  switch (cat) {
    case "Документи":
      return <FileText className="w-3.5 h-3.5 text-blue-400" />;
    case "Зображення":
      return <ImageIcon className="w-3.5 h-3.5 text-emerald-400" />;
    case "Відео та Аудіо":
      return <Film className="w-3.5 h-3.5 text-amber-400" />;
    case "Архіви":
      return <Archive className="w-3.5 h-3.5 text-rose-400" />;
    case "Інсталятори":
      return <Package className="w-3.5 h-3.5 text-pink-400" />;
    case "Код та Дані":
      return <Code className="w-3.5 h-3.5 text-purple-400" />;
    default:
      return <FolderCheck className="w-3.5 h-3.5 text-slate-400" />;
  }
}

interface DownloadsModalProps {
  onClose: () => void;
  onSuccess: (message: string) => void;
}

export const DownloadsModal: React.FC<DownloadsModalProps> = ({
  onClose,
  onSuccess,
}) => {
  const [analysis, setAnalysis] = useState<DownloadsAnalysisResult | null>(null);
  const [isLoading, setIsLoading] = useState<boolean>(true);
  const [isExecuting, setIsExecuting] = useState<boolean>(false);
  const [moveCategories, setMoveCategories] = useState<boolean>(true);
  const [selectedJunk, setSelectedJunk] = useState<Set<string>>(new Set());

  useEffect(() => {
    const loadAnalysis = async () => {
      try {
        const res = await invoke<DownloadsAnalysisResult>("analyze_downloads");
        setAnalysis(res);
        // За замовчуванням вибираємо весь знайдений мотлох для видалення
        const initialJunk = new Set(res.junk_candidates.map((j) => j.path));
        setSelectedJunk(initialJunk);
      } catch (err) {
        alert(`Помилка аналізу: ${String(err)}`);
        onClose();
      } finally {
        setIsLoading(false);
      }
    };
    loadAnalysis();
  }, [onClose]);

  const toggleJunkItem = (path: string) => {
    setSelectedJunk((prev) => {
      const next = new Set(prev);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  };

  const handleExecute = async () => {
    setIsExecuting(true);
    try {
      const res = await invoke<CleanupExecutionResult>(
        "execute_downloads_cleanup",
        {
          plan: {
            move_categories: moveCategories,
            delete_junk_paths: Array.from(selectedJunk),
          },
        }
      );
      onSuccess(res.message);
      onClose();
    } catch (err) {
      alert(`Помилка прибирання: ${String(err)}`);
      setIsExecuting(false);
    }
  };

  if (isLoading) {
    return (
      <div className="fixed inset-0 z-50 bg-black/70 backdrop-blur-sm flex items-center justify-center p-3 select-none">
        <div className="bg-slate-900 border border-slate-700 rounded-xl p-5 text-center text-xs text-slate-300">
          <Sparkles className="w-6 h-6 text-amber-400 animate-spin mx-auto mb-2" />
          Аналізую папку завантажень...
        </div>
      </div>
    );
  }

  if (!analysis) return null;

  return (
    <div className="fixed inset-0 z-50 bg-black/75 backdrop-blur-sm flex items-center justify-center p-3 select-none font-sans">
      <div className="bg-slate-900 border border-slate-700/80 rounded-2xl w-full max-w-sm flex flex-col shadow-2xl overflow-hidden max-h-[85vh]">
        {/* Шапка модалки */}
        <div className="bg-slate-800/80 px-4 py-3 flex items-center justify-between border-b border-slate-700/60">
          <div className="flex items-center gap-2">
            <span className="text-base">🧹</span>
            <div>
              <h2 className="text-xs font-bold text-white tracking-wide">
                Розумне прибирання Downloads
              </h2>
              <div className="text-[10px] text-slate-400">
                Всього файлів: {analysis.total_files_count} ({formatBytes(analysis.total_size)})
              </div>
            </div>
          </div>
          <button
            onClick={onClose}
            className="p-1 rounded-lg hover:bg-slate-700 text-slate-400 hover:text-white transition-colors"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Прокручуваний контент */}
        <div className="p-4 overflow-y-auto space-y-4 text-xs text-slate-300">
          {/* Блок 1: 5 найновіших файлів, які лишаються в корені */}
          <div className="bg-slate-950/70 border border-slate-800 rounded-xl p-3">
            <div className="flex items-center justify-between mb-2">
              <span className="font-semibold text-emerald-400 text-[11px] flex items-center gap-1">
                <Check className="w-3.5 h-3.5" />
                Лишаються в корені (5 найновіших)
              </span>
              <span className="text-[10px] text-slate-500 font-mono">
                Не чіпатимемо
              </span>
            </div>
            <div className="space-y-1">
              {analysis.kept_recent_files.map((file) => (
                <div
                  key={file.path}
                  className="flex items-center justify-between text-[11px] text-slate-300 bg-slate-900/50 px-2 py-1 rounded"
                >
                  <span className="truncate flex-1 pr-2" title={file.name}>
                    {file.name}
                  </span>
                  <span className="font-mono text-[10px] text-slate-400 shrink-0">
                    {formatBytes(file.size)}
                  </span>
                </div>
              ))}
              {analysis.kept_recent_files.length === 0 && (
                <div className="text-[10px] text-slate-500 italic">
                  Папка завантажень порожня
                </div>
              )}
            </div>
          </div>

          {/* Блок 2: Сортування за категоріями */}
          <div className="bg-slate-950/70 border border-slate-800 rounded-xl p-3">
            <label className="flex items-center justify-between cursor-pointer mb-2">
              <span className="font-semibold text-indigo-300 text-[11px]">
                📁 Сортувати старі файли за категоріями
              </span>
              <input
                type="checkbox"
                checked={moveCategories}
                onChange={(e) => setMoveCategories(e.target.checked)}
                className="rounded border-slate-700 bg-slate-800 text-indigo-500 focus:ring-0"
              />
            </label>

            {moveCategories && (
              <div className="space-y-1 mt-2 pt-2 border-t border-slate-800/80">
                {analysis.categories.map((cat) => (
                  <div
                    key={cat.category_name}
                    className="flex items-center justify-between text-[11px] text-slate-400 px-1"
                  >
                    <span className="flex items-center gap-1.5">
                      {getCategoryIcon(cat.category_name)}
                      {cat.category_name}
                    </span>
                    <span className="font-mono text-[10px] text-slate-500">
                      {cat.files.length} шт. ({formatBytes(cat.total_size)})
                    </span>
                  </div>
                ))}
              </div>
            )}
          </div>

          {/* Блок 3: Виявлений мотлох на видалення */}
          {analysis.junk_candidates.length > 0 && (
            <div className="bg-rose-950/20 border border-rose-900/40 rounded-xl p-3">
              <div className="flex items-center gap-1.5 text-rose-300 font-semibold text-[11px] mb-2">
                <Trash2 className="w-3.5 h-3.5" />
                Виявлений мотлох (рекомендується видалити)
              </div>
              <div className="space-y-1.5">
                {analysis.junk_candidates.map((junk) => (
                  <label
                    key={junk.path}
                    className="flex items-start gap-2 bg-slate-900/70 p-2 rounded cursor-pointer hover:bg-slate-900 transition-colors"
                  >
                    <input
                      type="checkbox"
                      checked={selectedJunk.has(junk.path)}
                      onChange={() => toggleJunkItem(junk.path)}
                      className="mt-0.5 rounded border-slate-700 bg-slate-800 text-rose-500 focus:ring-0 shrink-0"
                    />
                    <div className="flex-1 min-w-0">
                      <div className="font-medium text-slate-200 truncate text-[11px]">
                        {junk.name}
                      </div>
                      <div className="text-[10px] text-rose-400/90 leading-tight">
                        {junk.junk_reason} • {formatBytes(junk.size)}
                      </div>
                    </div>
                  </label>
                ))}
              </div>
            </div>
          )}
        </div>

        {/* Нижні кнопки дій */}
        <div className="bg-slate-800/80 px-4 py-3 flex items-center justify-end gap-2 border-t border-slate-700/60 shrink-0">
          <button
            type="button"
            onClick={onClose}
            disabled={isExecuting}
            className="px-3 py-1.5 rounded-lg bg-slate-700/60 hover:bg-slate-700 text-slate-300 text-xs font-medium transition-colors"
          >
            Скасувати
          </button>
          <button
            type="button"
            onClick={handleExecute}
            disabled={isExecuting}
            className="px-4 py-1.5 rounded-lg bg-indigo-600 hover:bg-indigo-500 disabled:bg-slate-700 text-white text-xs font-semibold shadow-md transition-colors flex items-center gap-1.5"
          >
            {isExecuting ? (
              <>
                <Sparkles className="w-3.5 h-3.5 animate-spin" />
                Прибираю...
              </>
            ) : (
              <>
                <Check className="w-3.5 h-3.5" />
                Застосувати прибирання
              </>
            )}
          </button>
        </div>
      </div>
    </div>
  );
};
