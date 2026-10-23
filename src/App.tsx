import React, { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  currentMonitor,
  getCurrentWindow,
  PhysicalPosition,
} from "@tauri-apps/api/window";
import { AnimatePresence, motion, type TargetAndTransition } from "framer-motion";
import {
  AlertTriangle,
  Bell,
  ChevronsDownUp,
  ChevronsUpDown,
  CornerDownLeft,
  ExternalLink,
  Hourglass,
  Mic,
  Paperclip,
  Sparkles,
  Square,
  Wrench,
  X,
} from "lucide-react";
import {
  askMascot,
  formatGeminiError,
  type MascotState,
  type MediaAttachment,
  type WorkingToolName,
  type ConfirmationRequest,
} from "./services/gemini";

// Синтетичний приємний дзвіночок нагадування через Web Audio API
function playNotificationChime() {
  try {
    const AudioContextClass =
      window.AudioContext ||
      (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
    if (!AudioContextClass) return;
    const ctx = new AudioContextClass();

    const playTone = (freq: number, start: number, duration: number) => {
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.type = "sine";
      osc.frequency.setValueAtTime(freq, ctx.currentTime + start);

      gain.gain.setValueAtTime(0.001, ctx.currentTime + start);
      gain.gain.exponentialRampToValueAtTime(0.32, ctx.currentTime + start + 0.03);
      gain.gain.exponentialRampToValueAtTime(0.001, ctx.currentTime + start + duration);

      osc.connect(gain);
      gain.connect(ctx.destination);

      osc.start(ctx.currentTime + start);
      osc.stop(ctx.currentTime + start + duration);
    };

    // Приємний передзвін: D5 (587Hz) -> A5 (880Hz)
    playTone(587.33, 0.0, 0.6);
    playTone(880.0, 0.22, 0.8);
  } catch {
    // Ігноруємо помилки блокування аудіо
  }
}

const TOOL_LABELS: Record<WorkingToolName, string> = {
  append_to_obsidian: "Записую в Obsidian...",
  create_or_update_file: "Майструю файл...",
  read_file_or_dir: "Вивчаю креслення й файли...",
  run_cli_command: "Виконую команду...",
  control_media: "Керую музикою 🎵...",
  control_display: "Налаштовую екран 🔆...",
  manage_memory: "Запам'ятовую контекст 🧠...",
  manage_reminders: "Налаштовую нагадування ⏰...",
  summarize_webpage: "Аналізую веб-сторінку 🌐...",
};

interface ClipboardImagePayload {
  mime_type: string;
  base64_data: string;
  name: string;
}

const MAX_IMAGE_DIMENSION = 1280;

async function compressImageFile(file: File): Promise<MediaAttachment> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();

    reader.onerror = () => {
      reject(new Error("Не вдалося зчитати файл зображення."));
    };

    reader.onload = (e) => {
      const dataUrl = e.target?.result as string;
      if (!dataUrl) {
        reject(new Error("Порожні дані зображення."));
        return;
      }

      const commaIdx = dataUrl.indexOf(",");
      const fallbackBase64 = commaIdx >= 0 ? dataUrl.slice(commaIdx + 1) : dataUrl;
      const detectedMime = file.type || "image/png";

      const fallbackAttachment: MediaAttachment = {
        mimeType: detectedMime,
        base64Data: fallbackBase64,
        previewUrl: dataUrl,
        name: file.name || "image.png",
      };

      const img = new Image();
      img.onload = () => {
        let { width, height } = img;
        if (width <= MAX_IMAGE_DIMENSION && height <= MAX_IMAGE_DIMENSION) {
          resolve(fallbackAttachment);
          return;
        }

        const ratio = Math.min(
          MAX_IMAGE_DIMENSION / width,
          MAX_IMAGE_DIMENSION / height
        );
        width = Math.max(1, Math.round(width * ratio));
        height = Math.max(1, Math.round(height * ratio));

        const canvas = document.createElement("canvas");
        canvas.width = width;
        canvas.height = height;
        const ctx = canvas.getContext("2d");

        if (!ctx) {
          resolve(fallbackAttachment);
          return;
        }

        ctx.drawImage(img, 0, 0, width, height);
        const outputMime = file.type === "image/png" ? "image/png" : "image/jpeg";
        const resizedDataUrl = canvas.toDataURL(outputMime, 0.85);
        const resizedComma = resizedDataUrl.indexOf(",");
        const resizedBase64 =
          resizedComma >= 0 ? resizedDataUrl.slice(resizedComma + 1) : resizedDataUrl;

        resolve({
          mimeType: outputMime,
          base64Data: resizedBase64,
          previewUrl: resizedDataUrl,
          name: file.name || "image.jpg",
        });
      };

      img.onerror = () => {
        resolve(fallbackAttachment);
      };

      img.src = dataUrl;
    };

    reader.readAsDataURL(file);
  });
}

function getMascotSprite(
  state: MascotState,
  activeTool?: WorkingToolName,
  isMusicPlaying = false
): string {
  switch (state) {
    case "IDLE":
      return isMusicPlaying ? "/mascot/idle-music.png" : "/mascot/idle.png";
    case "THINKING":
      return "/mascot/thinking.png";
    case "WORKING":
      if (
        activeTool === "read_file_or_dir" ||
        activeTool === "append_to_obsidian" ||
        activeTool === "summarize_webpage"
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

function getMascotAnimation(
  state: MascotState,
  isMusicPlaying = false
): TargetAndTransition {
  switch (state) {
    case "IDLE":
      if (isMusicPlaying) {
        // Ритмічне радісне погойдування під музику в навушниках!
        return {
          y: [0, -4, 0, -2, 0],
          rotate: [-3, 3, -2, 2, -3],
          scale: [1, 1.025, 0.99, 1.02, 1],
          transition: {
            duration: 1.15,
            repeat: Infinity,
            ease: "easeInOut",
          },
        };
      }
      return {
        y: [0, -2.5, 0],
        scale: [1, 1.01, 1],
        rotate: 0,
        transition: {
          duration: 3.2,
          repeat: Infinity,
          ease: "easeInOut",
        },
      };
    case "THINKING":
      return {
        y: [0, -1.5, 0],
        rotate: [-1.5, 1.5, -1.5],
        scale: 1.01,
        transition: {
          duration: 1.6,
          repeat: Infinity,
          ease: "easeInOut",
        },
      };
    case "WORKING":
      return {
        y: [0, -2.5, 0],
        rotate: [-2.5, 2.5, -2.5],
        scale: [1, 1.02, 1],
        transition: {
          duration: 0.55,
          repeat: Infinity,
          ease: "easeInOut",
        },
      };
    case "SUCCESS":
      return {
        y: [0, -4, 0, -1.5, 0],
        scale: [1, 1.03, 1],
        rotate: [0, -1.5, 1.5, 0],
        transition: {
          duration: 1.0,
          repeat: Infinity,
          ease: "easeOut",
        },
      };
    case "ERROR":
      return {
        x: [0, -4, 4, -2, 2, 0],
        rotate: [0, -3, 3, -1, 1, 0],
        transition: {
          duration: 0.45,
        },
      };
  }
}

function renderBubbleContent(text: string) {
  const linkRegex = /\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)|(https?:\/\/[^\s)]+)/g;
  const elements: React.ReactNode[] = [];
  let lastIndex = 0;
  let match: RegExpExecArray | null;

  while ((match = linkRegex.exec(text)) !== null) {
    if (match.index > lastIndex) {
      elements.push(text.slice(lastIndex, match.index));
    }

    const label = match[1] || match[3];
    const url = match[2] || match[3];

    elements.push(
      <a
        key={`${url}-${match.index}`}
        href={url}
        onClick={(e) => {
          e.preventDefault();
          invoke("open_external_url", { url }).catch(() => {});
        }}
        title={`Відкрити посилання: ${url}`}
        className="inline-flex items-center gap-0.5 font-medium text-amber-400 underline decoration-amber-400/50 underline-offset-2 transition-colors hover:text-amber-300 hover:decoration-amber-300 cursor-pointer"
      >
        <span>{label}</span>
        <ExternalLink className="inline h-2.5 w-2.5 opacity-80" />
      </a>
    );

    lastIndex = match.index + match[0].length;
  }

  if (lastIndex < text.length) {
    elements.push(text.slice(lastIndex));
  }

  return elements.length > 0 ? elements : text;
}

export function cleanBubbleText(text: string | null | undefined): string | null {
  if (!text) return null;
  let cleaned = text.trim();
  while (/^\[[^\]]+\]\s*/.test(cleaned)) {
    cleaned = cleaned.replace(/^\[[^\]]+\]\s*/, "").trim();
  }
  cleaned = cleaned.replace(/<\/?untrusted_web_content[^>]*>/gi, "").trim();
  return cleaned || null;
}

export default function App() {
  const [mascotState, setMascotState] = useState<MascotState>("IDLE");
  const [activeTool, setActiveTool] = useState<WorkingToolName | undefined>(
    undefined
  );
  const [isInputOpen, setIsInputOpen] = useState<boolean>(false);
  const [prompt, setPrompt] = useState<string>("");
  const [bubbleText, setBubbleText] = useState<string | null>(null);
  const [isExpanded, setIsExpanded] = useState<boolean>(false);
  const [attachedImage, setAttachedImage] = useState<MediaAttachment | null>(
    null
  );
  const [photoPop, setPhotoPop] = useState<boolean>(false);
  const [isDraggingFile, setIsDraggingFile] = useState<boolean>(false);
  const [isRecording, setIsRecording] = useState<boolean>(false);
  const [recordingSeconds, setRecordingSeconds] = useState<number>(0);
  const [isMusicPlaying, setIsMusicPlaying] = useState<boolean>(false);
  const [pendingConfirmation, setPendingConfirmation] = useState<{
    request: ConfirmationRequest;
    resolve: (approved: boolean) => void;
  } | null>(null);

  const inputRef = useRef<HTMLInputElement | null>(null);
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const successTimerRef = useRef<number | null>(null);
  const bubbleTimerRef = useRef<number | null>(null);
  const recordingTimerRef = useRef<number | null>(null);

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

  const rawBubble = pendingConfirmation
    ? pendingConfirmation.request.title
    : statusBanner ?? bubbleText;
  const displayedBubble = cleanBubbleText(rawBubble);
  const isPopupActive =
    isInputOpen || Boolean(displayedBubble) || Boolean(pendingConfirmation);
  const isOverloadNotice = Boolean(
    displayedBubble?.includes("⏳") || displayedBubble?.includes("перевантажені")
  );

  const toggleExpanded = async () => {
    const next = !isExpanded;
    setIsExpanded(next);
    try {
      await invoke("set_window_expanded", { expanded: next });
    } catch (err) {
      console.error("Failed to resize window:", err);
    }
  };

  const handleCloseBubble = async () => {
    if (isExpanded) {
      setIsExpanded(false);
      invoke("set_window_expanded", { expanded: false }).catch(() => {});
    }
    if (pendingConfirmation) {
      const cb = pendingConfirmation.resolve;
      setPendingConfirmation(null);
      cb(false);
    } else {
      setBubbleText(null);
      setMascotState((current) =>
        current === "SUCCESS" || current === "ERROR" ? "IDLE" : current
      );
    }
  };

  // Авто-зсув вікна в безпечну зону, якщо воно стоїть біля краю екрана і відкривається інпут/хмаринка
  useEffect(() => {
    let cancelled = false;

    const adjustWindowNearEdges = async () => {
      try {
        const win = getCurrentWindow();
        if (isPopupActive) {
          const [pos, monitor] = await Promise.all([
            win.outerPosition(),
            currentMonitor(),
          ]);
          if (cancelled || !monitor) return;

          const monitorBottom = monitor.position.y + monitor.size.height;
          const monitorRight = monitor.position.x + monitor.size.width;

          // Ліміт по нижньому краю (висота вікна 258px або 440px + безпечний відступ 16px)
          const windowHeight = isExpanded ? 440 : 258;
          const bottomSafeMargin = 16;
          const maxAllowedY = monitorBottom - windowHeight - bottomSafeMargin;
          const minAllowedY = monitor.position.y + 16;
          const targetY = Math.max(minAllowedY, Math.min(pos.y, maxAllowedY));

          // Ліміт по правому краю (ширина вікна 280px або 380px + безпечний відступ 16px)
          const windowWidth = isExpanded ? 380 : 280;
          const rightSafeMargin = 16;
          const maxAllowedX = monitorRight - windowWidth - rightSafeMargin;
          const minAllowedX = monitor.position.x + 16;
          const targetX = Math.max(minAllowedX, Math.min(pos.x, maxAllowedX));

          const needsShift = targetX !== pos.x || targetY !== pos.y;

          if (needsShift) {
            await win.setPosition(new PhysicalPosition(targetX, targetY));
          }
        }
      } catch {
        // Ігноруємо помилку поза середовищем Tauri
      }
    };

    void adjustWindowNearEdges();
    return () => {
      cancelled = true;
    };
  }, [isPopupActive, isExpanded]);

  // Автофокус на рядок вводу при його відкритті
  useEffect(() => {
    if (isInputOpen) {
      const id = window.setTimeout(() => {
        inputRef.current?.focus();
      }, 40);
      return () => window.clearTimeout(id);
    }
  }, [isInputOpen]);

  // Захисний таймер: маскот ніколи не повинен зависати в стані WORKING/THINKING довше 16 секунд
  useEffect(() => {
    if (!isBusy || pendingConfirmation !== null) return;
    const safetyTimer = window.setTimeout(() => {
      setMascotState("IDLE");
      setActiveTool(undefined);
      setBubbleText((prev) => prev || "Дію завершено ⏱️");
    }, 16000);

    return () => window.clearTimeout(safetyTimer);
  }, [isBusy, pendingConfirmation]);

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

  // Автозникнення хмаринки відповіді та повернення маскота в спокійний стан IDLE
  useEffect(() => {
    if (bubbleTimerRef.current) {
      window.clearTimeout(bubbleTimerRef.current);
      bubbleTimerRef.current = null;
    }

    if (
      bubbleText &&
      !isBusy &&
      !isExpanded &&
      pendingConfirmation === null &&
      bubbleText.length <= 180
    ) {
      const isReminder = bubbleText.startsWith("🔔") || bubbleText.startsWith("⚠️");
      const timeoutMs = isReminder ? 14000 : 9000;

      bubbleTimerRef.current = window.setTimeout(() => {
        setBubbleText(null);
        setMascotState((current) =>
          current === "SUCCESS" || current === "ERROR" ? "IDLE" : current
        );
      }, timeoutMs);
    }

    return () => {
      if (bubbleTimerRef.current) {
        window.clearTimeout(bubbleTimerRef.current);
      }
    };
  }, [bubbleText, isBusy, isExpanded]);

  // Прослуховування системних нагадувань і таймерів від фонового планувальника Rust
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<{ text: string; missed?: boolean }>("reminder-alert", (event) => {
      const payload = event.payload;
      playNotificationChime();
      setIsInputOpen(false);
      setMascotState("SUCCESS");
      setBubbleText(`🔔 ${payload.text}`);
      getCurrentWindow().unminimize().catch(() => {});
      getCurrentWindow().show().catch(() => {});
      getCurrentWindow().setAlwaysOnTop(true).catch(() => {});
      getCurrentWindow().setFocus().catch(() => {});
    }).then((fn) => {
      unlisten = fn;
    });

    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  // Відстеження активного відтворення музики (MPRIS / браузер) для перемикання спрайту в навушниках
  useEffect(() => {
    invoke<boolean>("is_music_playing")
      .then((playing) => setIsMusicPlaying(Boolean(playing)))
      .catch(() => {});

    let unlisten: (() => void) | undefined;
    listen<boolean>("media-playback-changed", (event) => {
      setIsMusicPlaying(Boolean(event.payload));
    }).then((fn) => {
      unlisten = fn;
    });

    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  // При старті гарантуємо стан вікна поверх усіх вікон та активну область кліку (після мапінгу XWayland)
  useEffect(() => {
    const id = window.setTimeout(() => {
      const win = getCurrentWindow();
      win.setIgnoreCursorEvents(false).catch(() => {});
      win.setAlwaysOnTop(true).catch(() => {});
    }, 450);
    return () => window.clearTimeout(id);
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
    const isImageMime = Boolean(file.type && file.type.startsWith("image/"));
    const isImageExt = /\.(png|jpe?g|webp|gif|bmp|svg)$/i.test(file.name);
    if (!isImageMime && !isImageExt) {
      setBubbleText("Підтримуються лише файли зображень (PNG, JPG, WebP).");
      return;
    }
    try {
      const compressed = await compressImageFile(file);
      setAttachedImage(compressed);
      setBubbleText("Фото прикріплено! 📸");
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

  const pasteLockRef = useRef<number>(0);

  const applyClipboardImage = (res: ClipboardImagePayload) => {
    setAttachedImage({
      mimeType: res.mime_type,
      base64Data: res.base64_data,
      previewUrl: `data:${res.mime_type};base64,${res.base64_data}`,
      name: res.name,
    });
    setBubbleText("Фото прикріплено! 📸");
    setPhotoPop(true);
    window.setTimeout(() => setPhotoPop(false), 850);
    if (!isInputOpen) {
      setIsInputOpen(true);
    }
    window.setTimeout(() => inputRef.current?.focus(), 60);
  };

  const handlePasteEvent = async (
    e: React.ClipboardEvent | ClipboardEvent
  ) => {
    const now = Date.now();
    if (now - pasteLockRef.current < 350) return;

    const clipboardData = "clipboardData" in e ? e.clipboardData : null;
    if (clipboardData) {
      // 1. Прямі файли з буфера обміну
      const files = clipboardData.files;
      if (files && files.length > 0) {
        for (let i = 0; i < files.length; i++) {
          const file = files[i];
          if (
            (file.type && file.type.startsWith("image/")) ||
            /\.(png|jpe?g|webp|gif|bmp)$/i.test(file.name)
          ) {
            pasteLockRef.current = Date.now();
            e.preventDefault();
            void attachFile(file);
            return;
          }
        }
      }

      // 2. Елементи буфера (скріншоти з пам'яті / PrintScreen / Copy Image)
      const items = clipboardData.items;
      if (items && items.length > 0) {
        for (let i = 0; i < items.length; i++) {
          const item = items[i];
          if (item.type.startsWith("image/")) {
            const file = item.getAsFile();
            if (file) {
              pasteLockRef.current = Date.now();
              e.preventDefault();
              void attachFile(file);
              return;
            }
          }
        }
      }

      // 3. Текстовий вміст буфера (копіювання файлу в Nautilus / файловому менеджері)
      const uriList = clipboardData.getData("text/uri-list") || "";
      const gnomeFiles =
        clipboardData.getData("x-special/gnome-copied-files") || "";
      const plainText = clipboardData.getData("text/plain") || "";
      const combined = `${uriList}\n${gnomeFiles}\n${plainText}`.trim();
      if (combined) {
        const lines = combined.split("\n");
        for (const rawLine of lines) {
          let clean = rawLine.trim();
          if (clean.startsWith("copy") || clean.startsWith("cut")) {
            clean = clean.replace(/^(copy|cut)\s*/, "").trim();
          }
          clean = clean.replace(/^file:\/\//, "");
          try {
            clean = decodeURIComponent(clean);
          } catch {}
          if (/\.(png|jpe?g|webp|gif|bmp)$/i.test(clean)) {
            pasteLockRef.current = Date.now();
            e.preventDefault();
            try {
              const base64Data = await invoke<string>("read_image_base64", {
                filepath: clean,
              });
              const mime = clean.endsWith(".png") ? "image/png" : "image/jpeg";
              applyClipboardImage({
                mime_type: mime,
                base64_data: base64Data,
                name: clean.split("/").pop() || "image.png",
              });
              return;
            } catch {
              // Якщо шлях недоступний, пробуємо через read_clipboard_image
            }
          }
        }
        // Якщо користувач вставляє звичайний текст (не шлях до зображення), не блокуємо стандартну вставку тексту
        if (plainText.trim().length > 0 && !uriList.trim() && !gnomeFiles.trim()) {
          pasteLockRef.current = Date.now();
          return;
        }
      }
    }

    // 4. Вбудоване читання системного буфера через GTK (без зовнішніх утиліт)
    pasteLockRef.current = Date.now();
    try {
      const res = await invoke<ClipboardImagePayload | null>(
        "read_clipboard_image"
      );
      if (res) {
        e.preventDefault();
        applyClipboardImage(res);
      }
    } catch {}
  };

  // Глобальна підтримка Ctrl+V (перехоплює на будь-якій розкладці: EN / UA)
  useEffect(() => {
    const onDocPaste = (e: ClipboardEvent) => {
      void handlePasteEvent(e);
    };

    const onKeyDown = async (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        if (isExpanded) {
          e.preventDefault();
          setIsExpanded(false);
          invoke("set_window_expanded", { expanded: false }).catch(() => {});
          return;
        } else if (bubbleText && !isBusy) {
          e.preventDefault();
          setBubbleText(null);
          return;
        }
      }

      const isPasteKey =
        (e.ctrlKey || e.metaKey) &&
        (e.code === "KeyV" ||
          e.key === "v" ||
          e.key === "V" ||
          e.key === "м" ||
          e.key === "М");

      if (isPasteKey && document.activeElement !== inputRef.current) {
        const now = Date.now();
        if (now - pasteLockRef.current < 350) return;
        pasteLockRef.current = now;
        try {
          const res = await invoke<ClipboardImagePayload | null>(
            "read_clipboard_image"
          );
          if (res) {
            e.preventDefault();
            applyClipboardImage(res);
          }
        } catch {}
      }
    };

    window.addEventListener("paste", onDocPaste, true);
    window.addEventListener("keydown", onKeyDown, true);
    return () => {
      window.removeEventListener("paste", onDocPaste, true);
      window.removeEventListener("keydown", onKeyDown, true);
    };
  }, [isInputOpen, isExpanded, bubbleText, isBusy]);

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
          .catch(() => {});
      }
    };

    const handleWindowMouseUp = () => {
      const state = pointerStateRef.current;
      const wasClick = state.isDown && !state.hasDragged;
      state.isDown = false;
      cleanupListeners();

      if (wasClick) {
        if (isExpanded) {
          setIsExpanded(false);
          invoke("set_window_expanded", { expanded: false }).catch(() => {});
        }
        if (isBusy) {
          // Якщо маскот завис під час тривалої операції — клік по ньому повертає управління
          setMascotState("IDLE");
          setActiveTool(undefined);
          setBubbleText(null);
          setIsInputOpen(true);
          return;
        }
        if (bubbleText && !isInputOpen) {
          setBubbleText(null);
        }
        setMascotState((current) =>
          current === "SUCCESS" || current === "ERROR" ? "IDLE" : current
        );
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
    if (isExpanded) {
      setIsExpanded(false);
      invoke("set_window_expanded", { expanded: false }).catch(() => {});
    }

    try {
      const reply = await askMascot(
        textPrompt,
        {
          onStateChange: (nextState, toolName) => {
            setMascotState(nextState);
            setActiveTool(nextState === "THINKING" ? undefined : toolName);
          },
          onConfirmRequest: (request) =>
            new Promise<boolean>((resolve) => {
              setPendingConfirmation({ request, resolve });
            }),
        },
        mediaItems
      );

      setBubbleText(cleanBubbleText(reply));
      setMascotState("SUCCESS");
      setActiveTool(undefined);

      successTimerRef.current = window.setTimeout(() => {
        setMascotState("IDLE");
      }, 2500);
    } catch (err) {
      const msg = formatGeminiError(err);
      setBubbleText(msg);
      setMascotState("ERROR");
      setActiveTool(undefined);

      const isOverloaded = msg.includes("⏳") || msg.includes("перевантажені");
      successTimerRef.current = window.setTimeout(() => {
        setMascotState("IDLE");
      }, isOverloaded ? 7500 : 4500);
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
        const msg = formatGeminiError(err);
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
        const msg = formatGeminiError(err);
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

  const spriteSrc = getMascotSprite(mascotState, activeTool, isMusicPlaying);

  return (
    <div
      onPaste={(e) => void handlePasteEvent(e)}
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
      className={`relative flex ${
        isExpanded ? "h-[440px] w-[380px]" : "h-[258px] w-[280px]"
      } flex-col items-center justify-start bg-transparent px-3 pt-6 select-none transition-all duration-200`}
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
                  scale: [1, 1.04, 1],
                  y: [0, -3.5, 0],
                  rotate: [0, -1.5, 1.5, 0],
                  transition: { duration: 0.4, ease: "easeOut" },
                }
              : getMascotAnimation(mascotState, isMusicPlaying)
          }
          whileHover={{ scale: 1.04 }}
          whileTap={{ scale: 0.96 }}
          title="Клікніть, щоб написати • Перетягніть, щоб перемістити • Киньте фото"
          className="relative flex h-[114px] w-[114px] cursor-grab items-center justify-center active:cursor-grabbing"
        >
          {/* М'яке світіння під час активних станів */}
          <div
            className={`pointer-events-none absolute inset-x-2.5 top-3.5 bottom-1 rounded-full blur-xl transition-opacity duration-300 ${
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
                : isMusicPlaying && mascotState === "IDLE"
                ? "bg-purple-500/35 opacity-100"
                : "opacity-0"
            }`}
          />

          <img
            src={spriteSrc}
            alt={`Mascot ${mascotState}`}
            draggable={false}
            className={`pointer-events-none h-[110px] w-[110px] object-contain drop-shadow-[0_6px_12px_rgba(0,0,0,0.45)] transition-all duration-200 ${
              mascotState === "ERROR" && !isOverloadNotice
                ? "hue-rotate-[-35deg] saturate-150"
                : ""
            }`}
          />
        </motion.div>
      </div>

      {/* Нижня зона: Хмаринка відповіді АБО Рядок вводу (з'являються прямо під маскотом) */}
      <div className="relative mt-1 flex w-full flex-col items-center justify-start">
        {/* Хмаринка повідомлення/статусу */}
        <AnimatePresence>
          {displayedBubble && !isInputOpen && (
            <motion.div
              key="mascot-speech-bubble"
              initial={{ opacity: 0, y: -6, scale: 0.96 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, y: -4, scale: 0.96 }}
              transition={{ duration: 0.15, ease: "easeOut" }}
              className={`relative ${
                isExpanded ? "max-h-[295px]" : "max-h-[105px]"
              } w-full overflow-y-auto overflow-x-hidden rounded-2xl border px-3 py-2 text-xs leading-relaxed shadow-xl transition-[max-height] duration-150 ease-out ${
                mascotState === "ERROR"
                  ? isOverloadNotice
                    ? "border-amber-400/80 bg-zinc-900/98 text-amber-100 ring-1 ring-amber-400/50 shadow-md shadow-amber-500/20"
                    : "border-red-500/60 bg-zinc-900/98 text-red-200"
                  : displayedBubble?.startsWith("🔔")
                  ? "border-amber-400 bg-zinc-900/98 text-amber-100 ring-1 ring-amber-400/50 shadow-md shadow-amber-500/20"
                  : "border-zinc-700/70 bg-zinc-900/98 text-zinc-100"
              }`}
            >
              {/* Хвостик хмаринки зверху по центру, що вказує на маскота */}
              <div
                className={`absolute -top-1.5 left-1/2 h-3 w-3 -translate-x-1/2 rotate-45 border-l border-t ${
                  mascotState === "ERROR"
                    ? isOverloadNotice
                      ? "border-amber-400/80 bg-zinc-900/98"
                      : "border-red-500/60 bg-zinc-900/98"
                    : displayedBubble?.startsWith("🔔")
                    ? "border-amber-400 bg-zinc-900/98"
                    : "border-zinc-700/70 bg-zinc-900/98"
                }`}
              />

              <div className="flex items-center justify-between gap-1.5 pb-1">
                {/* Лівий кут: Хрестик для закриття + статусний індикатор */}
                <div className="flex items-center gap-1.5 font-medium text-amber-400">
                  {!isBusy && (
                    <button
                      type="button"
                      onClick={handleCloseBubble}
                      aria-label="Закрити повідомлення"
                      title="Закрити (Esc)"
                      className="rounded-md p-0.5 text-zinc-400 transition-colors hover:bg-zinc-800 hover:text-zinc-100"
                    >
                      <X className="h-3.5 w-3.5" />
                    </button>
                  )}
                  {displayedBubble?.startsWith("🔔") && (
                    <Bell className="h-3.5 w-3.5 animate-bounce text-amber-400" />
                  )}
                  {mascotState === "WORKING" && (
                    <Wrench className="h-3.5 w-3.5 animate-spin text-amber-400" />
                  )}
                  {mascotState === "THINKING" && (
                    <Sparkles className="h-3.5 w-3.5 animate-pulse text-amber-400" />
                  )}
                  {mascotState === "ERROR" && (
                    isOverloadNotice ? (
                      <Hourglass className="h-3.5 w-3.5 animate-pulse text-amber-400" />
                    ) : (
                      <AlertTriangle className="h-3.5 w-3.5 text-red-400" />
                    )
                  )}
                </div>

                {/* Правий кут: Дві стрілочки в лінію (розгорнути / згорнути вікно виводу) */}
                {!isBusy && !pendingConfirmation && (
                  <button
                    type="button"
                    onClick={toggleExpanded}
                    aria-label={
                      isExpanded ? "Зменшити вікно виводу" : "Збільшити вікно виводу"
                    }
                    title={
                      isExpanded ? "Зменшити вікно виводу" : "Збільшити вікно виводу"
                    }
                    className="rounded-md p-0.5 text-zinc-400 transition-colors hover:bg-zinc-800 hover:text-zinc-100"
                  >
                    {isExpanded ? (
                      <ChevronsDownUp className="h-3.5 w-3.5 text-amber-400" />
                    ) : (
                      <ChevronsUpDown className="h-3.5 w-3.5" />
                    )}
                  </button>
                )}
              </div>

              {pendingConfirmation ? (
                <div className="mt-1 flex flex-col gap-1.5">
                  <div className="flex items-center gap-1.5 text-[11px] font-semibold text-amber-400">
                    <AlertTriangle className="h-3.5 w-3.5 shrink-0 text-amber-400" />
                    <span className="truncate">{pendingConfirmation.request.title}</span>
                  </div>
                  <div className="max-h-[36px] overflow-hidden text-ellipsis whitespace-nowrap rounded border border-zinc-700/50 bg-black/50 px-2 py-1 font-mono text-[10px] text-zinc-300">
                    {pendingConfirmation.request.details}
                  </div>
                  <div className="flex items-center justify-end gap-1.5 pt-1">
                    <button
                      type="button"
                      onClick={() => {
                        const cb = pendingConfirmation.resolve;
                        setPendingConfirmation(null);
                        cb(false);
                      }}
                      className="rounded bg-zinc-800 px-2.5 py-1 text-[11px] text-zinc-300 transition-colors hover:bg-zinc-700 hover:text-white"
                    >
                      Скасувати
                    </button>
                    <button
                      type="button"
                      onClick={() => {
                        const cb = pendingConfirmation.resolve;
                        setPendingConfirmation(null);
                        cb(true);
                      }}
                      className="rounded bg-amber-500 px-2.5 py-1 text-[11px] font-medium text-black transition-colors hover:bg-amber-400"
                    >
                      Підтвердити
                    </button>
                  </div>
                </div>
              ) : (
                <div className="mt-0.5 whitespace-pre-wrap break-words pr-1 text-[12px] text-zinc-100">
                  {renderBubbleContent(displayedBubble)}
                </div>
              )}
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
              onPaste={(e) => void handlePasteEvent(e)}
              className="mt-0.5 flex w-full flex-col gap-1.5 rounded-xl border border-zinc-700/70 bg-zinc-900/98 px-2.5 py-2 shadow-xl"
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
                    onPaste={(e) => void handlePasteEvent(e)}
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

