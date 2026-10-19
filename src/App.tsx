import React, { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  currentMonitor,
  getCurrentWindow,
  PhysicalPosition,
} from "@tauri-apps/api/window";
import { AnimatePresence, motion, type TargetAndTransition } from "framer-motion";
import {
  AlertTriangle,
  CornerDownLeft,
  Mic,
  Paperclip,
  Sparkles,
  Square,
  Wrench,
  X,
} from "lucide-react";
import {
  askMascot,
  type MascotState,
  type MediaAttachment,
  type WorkingToolName,
} from "./services/gemini";

const TOOL_LABELS: Record<WorkingToolName, string> = {
  append_to_obsidian: "Записую в Obsidian...",
  create_or_update_file: "Майструю файл...",
  read_file_or_dir: "Вивчаю креслення й файли...",
  run_cli_command: "Виконую команду...",
};

const MAX_IMAGE_DIMENSION = 1280;

async function compressImageFile(file: File): Promise<MediaAttachment> {
  return new Promise((resolve, reject) => {
    const objectUrl = URL.createObjectURL(file);
    const img = new Image();

    img.onload = () => {
      URL.revokeObjectURL(objectUrl);
      let { width, height } = img;

      if (width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION) {
        const ratio = Math.min(
          MAX_IMAGE_DIMENSION / width,
          MAX_IMAGE_DIMENSION / height
        );
        width = Math.max(1, Math.round(width * ratio));
        height = Math.max(1, Math.round(height * ratio));
      }

      const canvas = document.createElement("canvas");
      canvas.width = width;
      canvas.height = height;
      const ctx = canvas.getContext("2d");

      if (!ctx) {
        reject(new Error("Не вдалося ініціалізувати Canvas для стиснення фото."));
        return;
      }

      ctx.drawImage(img, 0, 0, width, height);
      const outputMime =
        file.type === "image/png" && width <= 800 && height <= 800
          ? "image/png"
          : "image/jpeg";
      const dataUrl = canvas.toDataURL(outputMime, 0.85);
      const commaIdx = dataUrl.indexOf(",");
      const base64Data = commaIdx >= 0 ? dataUrl.slice(commaIdx + 1) : dataUrl;

      resolve({
        mimeType: outputMime,
        base64Data,
        previewUrl: dataUrl,
        name: file.name || "image.jpg",
      });
    };

    img.onerror = () => {
      URL.revokeObjectURL(objectUrl);
      reject(new Error("Не вдалося завантажити вибране зображення."));
    };

    img.src = objectUrl;
  });
}

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
  const [attachedImage, setAttachedImage] = useState<MediaAttachment | null>(
    null
  );
  const [photoPop, setPhotoPop] = useState<boolean>(false);
  const [isDraggingFile, setIsDraggingFile] = useState<boolean>(false);
  const [isRecording, setIsRecording] = useState<boolean>(false);
  const [recordingSeconds, setRecordingSeconds] = useState<number>(0);

  const inputRef = useRef<HTMLInputElement | null>(null);
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const successTimerRef = useRef<number | null>(null);
  const bubbleTimerRef = useRef<number | null>(null);
  const recordingTimerRef = useRef<number | null>(null);

  // Зберігає координату вікна до авто-зсуву біля нижнього краю екрана
  const shiftedPosRef = useRef<{
    x: number;
    origY: number;
    shiftedY: number;
  } | null>(null);

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

  const statusBanner =
    mascotState === "THINKING"
      ? "Міркую над задачею..."
      : mascotState === "WORKING"
      ? activeTool
        ? TOOL_LABELS[activeTool]
        : "Виконую локальну дію..."
      : null;

  const displayedBubble = statusBanner ?? bubbleText;
  const isPopupActive = isInputOpen || Boolean(displayedBubble);

  // Ідея 2: Авто-зсув вікна вгору, якщо воно стоїть біля самого низу екрана і відкривається інпут/бабл
  useEffect(() => {
    let cancelled = false;

    const adjustWindowNearBottom = async () => {
      try {
        const win = getCurrentWindow();
        if (isPopupActive) {
          const [pos, size, monitor] = await Promise.all([
            win.outerPosition(),
            win.outerSize(),
            currentMonitor(),
          ]);
          if (cancelled || !monitor) return;

          const monitorBottom = monitor.position.y + monitor.size.height;
          const bottomSafeMargin = 48;
          const maxAllowedY = monitorBottom - size.height - bottomSafeMargin;

          if (pos.y > maxAllowedY) {
            const targetY = Math.max(monitor.position.y + 16, maxAllowedY);
            if (!shiftedPosRef.current) {
              shiftedPosRef.current = {
                x: pos.x,
                origY: pos.y,
                shiftedY: targetY,
              };
            } else {
              shiftedPosRef.current.shiftedY = targetY;
            }
            await win.setPosition(new PhysicalPosition(pos.x, targetY));
          }
        } else if (shiftedPosRef.current) {
          const saved = shiftedPosRef.current;
          shiftedPosRef.current = null;
          const pos = await win.outerPosition();
          if (cancelled) return;

          if (
            Math.abs(pos.x - saved.x) <= 20 &&
            Math.abs(pos.y - saved.shiftedY) <= 20
          ) {
            await win.setPosition(new PhysicalPosition(pos.x, saved.origY));
          }
        }
      } catch {
        // Ігноруємо помилку поза середовищем Tauri
      }
    };

    void adjustWindowNearBottom();
    return () => {
      cancelled = true;
    };
  }, [isPopupActive]);

  // Автофокус на рядок вводу при його відкритті
  useEffect(() => {
    if (isInputOpen) {
      const id = window.setTimeout(() => {
        inputRef.current?.focus();
      }, 40);
      return () => window.clearTimeout(id);
    }
  }, [isInputOpen]);

  // Таймер секунд під час активного запису голосу
  useEffect(() => {
    if (isRecording) {
      setRecordingSeconds(0);
      recordingTimerRef.current = window.setInterval(() => {
        setRecordingSeconds((prev) => prev + 1);
      }, 1000);
    } else if (recordingTimerRef.current) {
      window.clearInterval(recordingTimerRef.current);
      recordingTimerRef.current = null;
    }

    return () => {
      if (recordingTimerRef.current) {
        window.clearInterval(recordingTimerRef.current);
        recordingTimerRef.current = null;
      }
    };
  }, [isRecording]);

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

  // Очищення таймерів та запису при розмонтуванні
  useEffect(() => {
    return () => {
      if (successTimerRef.current) window.clearTimeout(successTimerRef.current);
      if (bubbleTimerRef.current) window.clearTimeout(bubbleTimerRef.current);
      if (recordingTimerRef.current)
        window.clearInterval(recordingTimerRef.current);
      invoke("cancel_voice_recording").catch(() => {});
    };
  }, []);

  const attachFile = async (file: File) => {
    if (!file.type.startsWith("image/")) {
      setBubbleText("Підтримуються лише файли зображень (PNG, JPG, WebP).");
      return;
    }
    try {
      const compressed = await compressImageFile(file);
      setAttachedImage(compressed);
      setPhotoPop(true);
      window.setTimeout(() => setPhotoPop(false), 850);
      if (!isInputOpen) {
        setIsInputOpen(true);
      }
      window.setTimeout(() => inputRef.current?.focus(), 60);
    } catch (err) {
      const msg =
        err instanceof Error ? err.message : "Помилка обробки зображення.";
      setBubbleText(msg);
    }
  };

  // Глобальна підтримка Ctrl+V (навіть якщо інпут закритий — відкриває й прикріплює)
  useEffect(() => {
    const handleGlobalPaste = (e: ClipboardEvent) => {
      const items = e.clipboardData?.items;
      if (!items) return;

      for (let i = 0; i < items.length; i++) {
        const item = items[i];
        if (item.type.startsWith("image/")) {
          const file = item.getAsFile();
          if (file) {
            e.preventDefault();
            void attachFile(file);
            break;
          }
        }
      }
    };

    window.addEventListener("paste", handleGlobalPaste);
    return () => window.removeEventListener("paste", handleGlobalPaste);
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
        shiftedPosRef.current = null;
        cleanupListeners();
        getCurrentWindow()
          .startDragging()
          .catch(() => {});
      }
    };

    const handleWindowMouseUp = () => {
      const state = pointerStateRef.current;
      const wasClick = state.isDown && !state.hasDragged;
      state.isDown = false;
      cleanupListeners();

      if (wasClick) {
        if (bubbleText && !isInputOpen) {
          setBubbleText(null);
        }
        if (isRecording) {
          setIsRecording(false);
          invoke("cancel_voice_recording").catch(() => {});
        }
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

  const executePromptRequest = async (
    textPrompt: string,
    mediaItems: MediaAttachment[]
  ) => {
    if (successTimerRef.current) {
      window.clearTimeout(successTimerRef.current);
      successTimerRef.current = null;
    }

    setPrompt("");
    setAttachedImage(null);
    setIsInputOpen(false);
    setBubbleText(null);

    try {
      const reply = await askMascot(
        textPrompt,
        {
          onStateChange: (nextState, toolName) => {
            setMascotState(nextState);
            setActiveTool(toolName);
          },
        },
        mediaItems
      );

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

  const handleToggleVoiceRecording = async () => {
    if (isBusy) return;

    if (!isRecording) {
      try {
        setBubbleText(null);
        await invoke("start_voice_recording");
        setIsRecording(true);
      } catch (err) {
        const msg =
          err instanceof Error ? err.message : String(err);
        setBubbleText(msg);
        setMascotState("ERROR");
      }
    } else {
      setIsRecording(false);
      try {
        const wavBase64 = await invoke<string>("stop_voice_recording");
        const attachments: MediaAttachment[] = [];
        if (attachedImage) {
          attachments.push(attachedImage);
        }
        attachments.push({
          mimeType: "audio/wav",
          base64Data: wavBase64,
          name: "voice.wav",
        });
        await executePromptRequest(prompt.trim(), attachments);
      } catch (err) {
        const msg =
          err instanceof Error ? err.message : String(err);
        setBubbleText(msg);
        setMascotState("ERROR");
      }
    }
  };

  const handleSubmit = async (e?: React.FormEvent) => {
    e?.preventDefault();
    if (isBusy) return;

    if (isRecording) {
      await handleToggleVoiceRecording();
      return;
    }

    const trimmed = prompt.trim();
    if (!trimmed && !attachedImage) return;

    const attachments: MediaAttachment[] = attachedImage ? [attachedImage] : [];
    await executePromptRequest(trimmed, attachments);
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Escape") {
      e.preventDefault();
      if (isRecording) {
        setIsRecording(false);
        invoke("cancel_voice_recording").catch(() => {});
      } else {
        setIsInputOpen(false);
      }
    } else if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void handleSubmit();
    }
  };

  const handleFileChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (file) {
      void attachFile(file);
    }
    e.target.value = "";
  };

  const handleDrop = (e: React.DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    setIsDraggingFile(false);
    const file = e.dataTransfer.files?.[0];
    if (file) {
      void attachFile(file);
    }
  };

  const spriteSrc = getMascotSprite(mascotState, activeTool);

  return (
    <div
      onDragEnter={(e) => {
        e.preventDefault();
        setIsDraggingFile(true);
      }}
      onDragOver={(e) => {
        e.preventDefault();
        setIsDraggingFile(true);
      }}
      onDragLeave={(e) => {
        // Якщо курсор вийшов за межі вікна
        if (!e.currentTarget.contains(e.relatedTarget as Node)) {
          setIsDraggingFile(false);
        }
      }}
      onDrop={handleDrop}
      className="relative flex h-[240px] w-[280px] flex-col items-center justify-start bg-transparent px-3 pt-3 select-none"
    >
      {/* Прихований input для кнопки «Скріпка» */}
      <input
        ref={fileInputRef}
        type="file"
        accept="image/png,image/jpeg,image/webp,image/gif"
        onChange={handleFileChange}
        className="hidden"
      />

      {/* Оверлей Drag-and-Drop, коли користувач тягне фото у вікно */}
      <AnimatePresence>
        {isDraggingFile && (
          <motion.div
            initial={{ opacity: 0, scale: 0.92 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.92 }}
            transition={{ duration: 0.15 }}
            className="pointer-events-none absolute inset-2 z-50 flex flex-col items-center justify-center gap-1.5 rounded-2xl border-2 border-dashed border-amber-400 bg-zinc-950/90 shadow-2xl backdrop-blur-md"
          >
            <Paperclip className="h-6 w-6 animate-bounce text-amber-400" />
            <span className="text-xs font-semibold text-zinc-100">
              Кидай фото сюди! 📸
            </span>
          </motion.div>
        )}
      </AnimatePresence>

      {/* Верхня зона: Маскот (починається з самого верху, без порожнього мертвого простору) */}
      <div className="relative flex flex-col items-center justify-center">
        <motion.div
          onMouseDown={handleMascotMouseDown}
          animate={
            photoPop
              ? {
                  scale: [1, 1.15, 0.96, 1.05, 1],
                  y: [0, -12, 2, -4, 0],
                  rotate: [0, -4, 4, -2, 0],
                  transition: { duration: 0.75, ease: "easeOut" },
                }
              : getMascotAnimation(mascotState)
          }
          whileHover={{ scale: 1.04 }}
          whileTap={{ scale: 0.96 }}
          title="Клікніть, щоб написати • Перетягніть, щоб перемістити • Киньте фото"
          className="relative flex h-[114px] w-[114px] cursor-grab items-center justify-center active:cursor-grabbing"
        >
          {/* М'яке світіння під час активних станів */}
          <div
            className={`pointer-events-none absolute inset-1 rounded-full blur-xl transition-opacity duration-300 ${
              photoPop
                ? "bg-amber-400/50 opacity-100"
                : isRecording
                ? "bg-red-500/40 opacity-100"
                : mascotState === "SUCCESS"
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
            className={`pointer-events-none h-[110px] w-[110px] object-contain drop-shadow-[0_6px_12px_rgba(0,0,0,0.45)] transition-all duration-200 ${
              mascotState === "ERROR" ? "hue-rotate-[-35deg] saturate-150" : ""
            }`}
          />
        </motion.div>
      </div>

      {/* Нижня зона: Хмаринка відповіді АБО Рядок вводу (з'являються прямо під маскотом) */}
      <div className="relative mt-1 flex w-full flex-col items-center justify-start">
        {/* Хмаринка повідомлення/статусу */}
        <AnimatePresence mode="wait">
          {displayedBubble && !isInputOpen && (
            <motion.div
              key={statusBanner ? `status-${mascotState}-${activeTool}` : "reply"}
              initial={{ opacity: 0, y: -6, scale: 0.94 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, y: -4, scale: 0.94 }}
              transition={{ duration: 0.18, ease: "easeOut" }}
              className={`relative max-h-[95px] w-full overflow-y-auto rounded-2xl border px-3 py-2 text-xs leading-relaxed shadow-lg backdrop-blur-md ${
                mascotState === "ERROR"
                  ? "border-red-500/60 bg-zinc-900/95 text-red-200"
                  : "border-zinc-700/60 bg-zinc-900/90 text-zinc-100"
              }`}
            >
              {/* Хвостик хмаринки зверху по центру, що вказує на маскота */}
              <div
                className={`absolute -top-1.5 left-1/2 h-3 w-3 -translate-x-1/2 rotate-45 border-l border-t ${
                  mascotState === "ERROR"
                    ? "border-red-500/60 bg-zinc-900/95"
                    : "border-zinc-700/60 bg-zinc-900/90"
                }`}
              />

              <div className="flex items-start justify-between gap-1.5">
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

              <p className="mt-0.5 whitespace-pre-wrap break-words pr-1 text-[12px] text-zinc-100">
                {displayedBubble}
              </p>
            </motion.div>
          )}
        </AnimatePresence>

        {/* Рядок вводу з прев'ю картинки, скріпкою та мікрофоном */}
        <AnimatePresence>
          {isInputOpen && (
            <motion.form
              initial={{ opacity: 0, y: -6, scale: 0.95 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, y: -4, scale: 0.95 }}
              transition={{ duration: 0.18, ease: "easeOut" }}
              onSubmit={handleSubmit}
              className="mt-0.5 flex w-full flex-col gap-1.5 rounded-xl border border-zinc-700/60 bg-zinc-900/90 px-2.5 py-2 shadow-lg backdrop-blur-md"
            >
              {/* Прев'ю прикріпленого зображення з анімацією та статусом */}
              <AnimatePresence>
                {attachedImage && (
                  <motion.div
                    initial={{ opacity: 0, scale: 0.85, y: -6 }}
                    animate={{ opacity: 1, scale: 1, y: 0 }}
                    exit={{ opacity: 0, scale: 0.85, y: -6 }}
                    transition={{ type: "spring", stiffness: 350, damping: 24 }}
                    className="flex items-center justify-between gap-2 rounded-lg border border-amber-400/40 bg-zinc-800/90 px-2 py-1 shadow-md ring-1 ring-amber-400/20"
                  >
                    <div className="flex items-center gap-2 overflow-hidden">
                      {attachedImage.previewUrl && (
                        <img
                          src={attachedImage.previewUrl}
                          alt="Preview"
                          className="h-7 w-7 shrink-0 rounded border border-amber-400/30 object-cover shadow-sm"
                        />
                      )}
                      <div className="flex flex-col overflow-hidden leading-tight">
                        <span className="truncate text-[11px] font-medium text-zinc-200">
                          {attachedImage.name ?? "Зображення"}
                        </span>
                        <span className="text-[9.5px] text-amber-400/80">
                          Фото прикріплено ✓
                        </span>
                      </div>
                    </div>
                    <button
                      type="button"
                      onClick={() => setAttachedImage(null)}
                      title="Видалити зображення"
                      aria-label="Видалити зображення"
                      className="rounded p-0.5 text-zinc-400 transition-colors hover:bg-zinc-700 hover:text-zinc-100"
                    >
                      <X className="h-3 w-3" />
                    </button>
                  </motion.div>
                )}
              </AnimatePresence>

              <div className="flex w-full items-center gap-1.5">
                {/* Кнопка «Скріпка» для прикріплення фото з активною пімпочкою */}
                <button
                  type="button"
                  onClick={() => fileInputRef.current?.click()}
                  disabled={isBusy || isRecording}
                  title={
                    attachedImage
                      ? `Прикріплено: ${attachedImage.name}`
                      : "Прикріпити фото (або Ctrl+V)"
                  }
                  aria-label="Прикріпити фото"
                  className={`relative flex h-6 w-6 shrink-0 items-center justify-center rounded-lg transition-all disabled:opacity-40 ${
                    attachedImage
                      ? "bg-amber-400/20 text-amber-400 hover:bg-amber-400/30"
                      : "text-zinc-400 hover:bg-zinc-800 hover:text-zinc-100"
                  }`}
                >
                  <Paperclip className="h-3.5 w-3.5" />
                  {/* Пімпочка-індикатор прикріпленого фото */}
                  {attachedImage && (
                    <span className="absolute -right-0.5 -top-0.5 flex h-2 w-2">
                      <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-amber-400 opacity-75" />
                      <span className="relative inline-flex h-2 w-2 rounded-full border border-zinc-900 bg-amber-500" />
                    </span>
                  )}
                </button>

                {isRecording ? (
                  <div className="flex w-full items-center gap-2 px-1 text-xs font-medium text-red-400">
                    <span className="h-2 w-2 animate-ping rounded-full bg-red-500" />
                    <span className="truncate">
                      Слухаю... {recordingSeconds}с
                    </span>
                  </div>
                ) : (
                  <input
                    ref={inputRef}
                    type="text"
                    value={prompt}
                    onChange={(e) => setPrompt(e.target.value)}
                    onKeyDown={handleKeyDown}
                    disabled={isBusy}
                    placeholder={
                      attachedImage ? "Додати підпис..." : "Що зробити?"
                    }
                    className="w-full bg-transparent text-xs text-zinc-100 placeholder-zinc-400 outline-none disabled:opacity-50"
                  />
                )}

                {/* Кнопка голосового вводу (Мікрофон / Стоп) */}
                <button
                  type="button"
                  onClick={() => void handleToggleVoiceRecording()}
                  disabled={isBusy}
                  title={
                    isRecording
                      ? "Зупинити й надіслати голос"
                      : "Голосове введення"
                  }
                  aria-label="Голосове введення"
                  className={`flex h-6 w-6 shrink-0 items-center justify-center rounded-lg transition-all disabled:opacity-40 ${
                    isRecording
                      ? "animate-pulse bg-red-500 text-white hover:bg-red-400"
                      : "text-zinc-400 hover:bg-zinc-800 hover:text-zinc-100"
                  }`}
                >
                  {isRecording ? (
                    <Square className="h-3 w-3 fill-current" />
                  ) : (
                    <Mic className="h-3.5 w-3.5" />
                  )}
                </button>

                {/* Кнопка відправки */}
                <button
                  type="submit"
                  disabled={
                    (!prompt.trim() && !attachedImage && !isRecording) || isBusy
                  }
                  aria-label="Відправити запит"
                  className="flex h-6 w-6 shrink-0 items-center justify-center rounded-lg bg-amber-400/90 text-zinc-950 transition-all hover:bg-amber-300 disabled:opacity-40"
                >
                  <CornerDownLeft className="h-3.5 w-3.5" />
                </button>
              </div>
            </motion.form>
          )}
        </AnimatePresence>
      </div>
    </div>
  );
}

