import React, { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { AnimatePresence, motion, type TargetAndTransition } from "framer-motion";
import { AlertTriangle, CornerDownLeft, Sparkles, Wrench, X } from "lucide-react";
import {
  askMascot,
  type MascotState,
  type WorkingToolName,
} from "./services/gemini";

const TOOL_LABELS: Record<WorkingToolName, string> = {
  append_to_obsidian: "Записую в Obsidian...",
  create_or_update_file: "Майструю файл...",
  read_file_or_dir: "Вивчаю креслення й файли...",
  run_cli_command: "Виконую команду...",
};

function getMascotSprite(
  state: MascotState,
  activeTool?: WorkingToolName
): string {
  switch (state) {
    case "IDLE":
      return "/mascot/idle.png";
    case "THINKING":
      return "/mascot/thinking.png";
    case "WORKING":
      if (
        activeTool === "read_file_or_dir" ||
        activeTool === "append_to_obsidian"
      ) {
        return "/mascot/working-blueprint.png";
      }
      return "/mascot/working-hammer.png";
    case "SUCCESS":
      return "/mascot/success.png";
    case "ERROR":
      return "/mascot/thinking.png";
  }
}

function getMascotAnimation(state: MascotState): TargetAndTransition {
  switch (state) {
    case "IDLE":
      return {
        y: [0, -6, 0],
        scale: [1, 1.02, 1],
        rotate: 0,
        transition: {
          duration: 2.8,
          repeat: Infinity,
          ease: "easeInOut",
        },
      };
    case "THINKING":
      return {
        y: [0, -3, 0],
        rotate: [-3, 3, -3],
        scale: 1.02,
        transition: {
          duration: 1.2,
          repeat: Infinity,
          ease: "easeInOut",
        },
      };
    case "WORKING":
      return {
        y: [0, -5, 0],
        rotate: [-5, 5, -5],
        scale: [1, 1.05, 1],
        transition: {
          duration: 0.45,
          repeat: Infinity,
          ease: "easeInOut",
        },
      };
    case "SUCCESS":
      return {
        y: [0, -12, 0, -6, 0],
        scale: [1, 1.08, 1.02, 1.06, 1],
        rotate: [0, -4, 4, 0],
        transition: {
          duration: 0.9,
          repeat: Infinity,
          ease: "easeOut",
        },
      };
    case "ERROR":
      return {
        x: [0, -7, 7, -5, 5, 0],
        rotate: [0, -6, 6, -3, 3, 0],
        transition: {
          duration: 0.5,
        },
      };
  }
}

export default function App() {
  const [mascotState, setMascotState] = useState<MascotState>("IDLE");
  const [activeTool, setActiveTool] = useState<WorkingToolName | undefined>(
    undefined
  );
  const [isInputOpen, setIsInputOpen] = useState<boolean>(false);
  const [prompt, setPrompt] = useState<string>("");
  const [bubbleText, setBubbleText] = useState<string | null>(null);

  const inputRef = useRef<HTMLInputElement | null>(null);
  const successTimerRef = useRef<number | null>(null);
  const bubbleTimerRef = useRef<number | null>(null);

  // Реф для відстеження Drag vs Click (> 5px)
  const pointerStateRef = useRef<{
    isDown: boolean;
    startX: number;
    startY: number;
    hasDragged: boolean;
  }>({
    isDown: false,
    startX: 0,
    startY: 0,
    hasDragged: false,
  });

  const isBusy = mascotState === "THINKING" || mascotState === "WORKING";

  // Автофокус на рядок вводу при його відкритті
  useEffect(() => {
    if (isInputOpen) {
      const id = window.setTimeout(() => {
        inputRef.current?.focus();
      }, 40);
      return () => window.clearTimeout(id);
    }
  }, [isInputOpen]);

  // Автозникнення хмаринки відповіді через 9 секунд, якщо текст короткий
  useEffect(() => {
    if (bubbleTimerRef.current) {
      window.clearTimeout(bubbleTimerRef.current);
      bubbleTimerRef.current = null;
    }

    if (bubbleText && !isBusy && bubbleText.length <= 180) {
      bubbleTimerRef.current = window.setTimeout(() => {
        setBubbleText(null);
      }, 9000);
    }

    return () => {
      if (bubbleTimerRef.current) {
        window.clearTimeout(bubbleTimerRef.current);
      }
    };
  }, [bubbleText, isBusy]);

  // При старті гарантуємо стан вікна поверх усіх вікон
  useEffect(() => {
    getCurrentWindow().setAlwaysOnTop(true).catch(() => {});
  }, []);

  // Очищення таймерів при розмонтуванні
  useEffect(() => {
    return () => {
      if (successTimerRef.current) window.clearTimeout(successTimerRef.current);
      if (bubbleTimerRef.current) window.clearTimeout(bubbleTimerRef.current);
    };
  }, []);

  // Обробка перетягування вікна vs короткого кліку по маскоту
  const handleMascotMouseDown = (e: React.MouseEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    pointerStateRef.current = {
      isDown: true,
      startX: e.clientX,
      startY: e.clientY,
      hasDragged: false,
    };

    const handleWindowMouseMove = (moveEvent: MouseEvent) => {
      const state = pointerStateRef.current;
      if (!state.isDown || state.hasDragged) return;

      const dx = moveEvent.clientX - state.startX;
      const dy = moveEvent.clientY - state.startY;

      if (Math.hypot(dx, dy) > 5) {
        state.hasDragged = true;
        state.isDown = false;
        cleanupListeners();
        getCurrentWindow()
          .startDragging()
          .catch(() => {
            // Ігноруємо помилку, якщо відкрито у звичайному браузері поза Tauri
          });
      }
    };

    const handleWindowMouseUp = () => {
      const state = pointerStateRef.current;
      const wasClick = state.isDown && !state.hasDragged;
      state.isDown = false;
      cleanupListeners();

      if (wasClick) {
        setIsInputOpen((prev) => !prev);
      }
    };

    const cleanupListeners = () => {
      window.removeEventListener("mousemove", handleWindowMouseMove);
      window.removeEventListener("mouseup", handleWindowMouseUp);
    };

    window.addEventListener("mousemove", handleWindowMouseMove);
    window.addEventListener("mouseup", handleWindowMouseUp);
  };

  const handleSubmit = async (e?: React.FormEvent) => {
    e?.preventDefault();
    const trimmed = prompt.trim();
    if (!trimmed || isBusy) return;

    if (successTimerRef.current) {
      window.clearTimeout(successTimerRef.current);
      successTimerRef.current = null;
    }

    setPrompt("");
    setIsInputOpen(false);
    setBubbleText(null);

    try {
      const reply = await askMascot(trimmed, {
        onStateChange: (nextState, toolName) => {
          setMascotState(nextState);
          setActiveTool(toolName);
        },
      });

      setBubbleText(reply);
      setMascotState("SUCCESS");
      setActiveTool(undefined);

      successTimerRef.current = window.setTimeout(() => {
        setMascotState("IDLE");
      }, 2500);
    } catch (err) {
      const msg =
        err instanceof Error ? err.message : "Сталася непередбачена помилка.";
      setBubbleText(msg);
      setMascotState("ERROR");
      setActiveTool(undefined);

      successTimerRef.current = window.setTimeout(() => {
        setMascotState("IDLE");
      }, 4000);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Escape") {
      e.preventDefault();
      setIsInputOpen(false);
    } else if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void handleSubmit();
    }
  };

  const statusBanner =
    mascotState === "THINKING"
      ? "Міркую над задачею..."
      : mascotState === "WORKING"
      ? activeTool
        ? TOOL_LABELS[activeTool]
        : "Виконую локальну дію..."
      : null;

  const displayedBubble = statusBanner ?? bubbleText;
  const spriteSrc = getMascotSprite(mascotState, activeTool);

  return (
    <div className="relative flex h-[400px] w-[320px] flex-col items-center justify-between bg-transparent px-3 py-3 select-none">
      {/* Верхня зона: Хмаринка відповіді (Speech Bubble) */}
      <div className="relative flex min-h-[110px] w-full items-end justify-center">
        <AnimatePresence mode="wait">
          {displayedBubble && (
            <motion.div
              key={statusBanner ? `status-${mascotState}-${activeTool}` : "reply"}
              initial={{ opacity: 0, y: 10, scale: 0.92 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, y: 8, scale: 0.92 }}
              transition={{ duration: 0.2, ease: "easeOut" }}
              className={`relative max-h-[115px] w-full overflow-y-auto rounded-2xl border px-3.5 py-2.5 text-xs leading-relaxed shadow-lg backdrop-blur-md ${
                mascotState === "ERROR"
                  ? "border-red-500/60 bg-zinc-900/95 text-red-200"
                  : "border-zinc-700/60 bg-zinc-900/90 text-zinc-100"
              }`}
            >
              <div className="flex items-start justify-between gap-2">
                <div className="flex items-center gap-1.5 font-medium text-amber-400">
                  {mascotState === "WORKING" && (
                    <Wrench className="h-3.5 w-3.5 animate-spin text-amber-400" />
                  )}
                  {mascotState === "THINKING" && (
                    <Sparkles className="h-3.5 w-3.5 animate-pulse text-amber-400" />
                  )}
                  {mascotState === "ERROR" && (
                    <AlertTriangle className="h-3.5 w-3.5 text-red-400" />
                  )}
                </div>

                {!isBusy && (
                  <button
                    type="button"
                    onClick={() => setBubbleText(null)}
                    aria-label="Закрити повідомлення"
                    className="rounded-md p-0.5 text-zinc-400 transition-colors hover:bg-zinc-800 hover:text-zinc-100"
                  >
                    <X className="h-3.5 w-3.5" />
                  </button>
                )}
              </div>

              <p className="mt-0.5 whitespace-pre-wrap break-words pr-1 text-[12.5px] text-zinc-100">
                {displayedBubble}
              </p>

              {/* Хвостик хмаринки знизу по центру */}
              <div
                className={`absolute -bottom-1.5 left-1/2 h-3 w-3 -translate-x-1/2 rotate-45 border-b border-r ${
                  mascotState === "ERROR"
                    ? "border-red-500/60 bg-zinc-900/95"
                    : "border-zinc-700/60 bg-zinc-900/90"
                }`}
              />
            </motion.div>
          )}
        </AnimatePresence>
      </div>

      {/* Центральна зона: 2D-маскот (~130x130px) */}
      <div className="relative flex flex-col items-center justify-center my-1">
        <motion.div
          onMouseDown={handleMascotMouseDown}
          animate={getMascotAnimation(mascotState)}
          whileHover={{ scale: 1.04 }}
          whileTap={{ scale: 0.96 }}
          title="Клікніть, щоб написати • Перетягніть, щоб перемістити"
          className="relative flex h-[134px] w-[134px] cursor-grab items-center justify-center active:cursor-grabbing"
        >
          {/* М'яке світіння під час активних станів */}
          <div
            className={`pointer-events-none absolute inset-2 rounded-full blur-xl transition-opacity duration-300 ${
              mascotState === "SUCCESS"
                ? "bg-amber-400/30 opacity-100"
                : mascotState === "WORKING"
                ? "bg-purple-500/25 opacity-100"
                : mascotState === "ERROR"
                ? "bg-red-500/35 opacity-100"
                : "opacity-0"
            }`}
          />

          <img
            src={spriteSrc}
            alt={`Mascot ${mascotState}`}
            draggable={false}
            className={`pointer-events-none h-[130px] w-[130px] object-contain drop-shadow-[0_6px_12px_rgba(0,0,0,0.45)] transition-all duration-200 ${
              mascotState === "ERROR" ? "hue-rotate-[-35deg] saturate-150" : ""
            }`}
          />
        </motion.div>
      </div>

      {/* Нижня зона: Виїзний рядок вводу (Input Bar) */}
      <div className="flex min-h-[56px] w-full items-start justify-center">
        <AnimatePresence>
          {isInputOpen && (
            <motion.form
              initial={{ opacity: 0, y: -10, scale: 0.95 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, y: -8, scale: 0.95 }}
              transition={{ duration: 0.18, ease: "easeOut" }}
              onSubmit={handleSubmit}
              className="flex w-full items-center gap-2 rounded-xl border border-zinc-700/60 bg-zinc-900/90 px-3 py-2 shadow-lg backdrop-blur-md"
            >
              <input
                ref={inputRef}
                type="text"
                value={prompt}
                onChange={(e) => setPrompt(e.target.value)}
                onKeyDown={handleKeyDown}
                disabled={isBusy}
                placeholder="Що зробити?"
                className="w-full bg-transparent text-xs text-zinc-100 placeholder-zinc-400 outline-none disabled:opacity-50"
              />
              <button
                type="submit"
                disabled={!prompt.trim() || isBusy}
                aria-label="Відправити запит"
                className="flex h-6 w-6 shrink-0 items-center justify-center rounded-lg bg-amber-400/90 text-zinc-950 transition-all hover:bg-amber-300 disabled:opacity-40"
              >
                <CornerDownLeft className="h-3.5 w-3.5" />
              </button>
            </motion.form>
          )}
        </AnimatePresence>
      </div>
    </div>
  );
}

