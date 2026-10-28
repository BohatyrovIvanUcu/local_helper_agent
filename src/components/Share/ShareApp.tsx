import React, { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import QRCode from "qrcode";
import {
  Check,
  Copy,
  FileText,
  FolderArchive,
  Radio,
  Square,
  Wifi,
} from "lucide-react";

interface ShareSessionInfo {
  file_path: string;
  file_name: string;
  file_size: number;
  is_dir: boolean;
  ip_address: string;
  port: number;
  share_url: string;
  local_url?: string;
  status: string;
  temp_zip_path?: string;
}

function formatBytes(bytes: number): string {
  if (!bytes || bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`;
}

export const ShareApp: React.FC = () => {
  const [session, setSession] = useState<ShareSessionInfo | null>(null);
  const [selectedMode, setSelectedMode] = useState<"tunnel" | "local">("tunnel");
  const [qrCodeDataUrl, setQrCodeDataUrl] = useState<string>("");
  const [copied, setCopied] = useState<boolean>(false);
  const [transferStatus, setTransferStatus] = useState<
    "waiting" | "downloading" | "completed"
  >("waiting");
  const [errorMessage, setErrorMessage] = useState<string | null>(null);

  const activeUrl = session
    ? selectedMode === "local" && session.local_url
      ? session.local_url
      : session.share_url
    : "";

  useEffect(() => {
    const init = async () => {
      const params = new URLSearchParams(window.location.search);
      const filePath = params.get("file");

      try {
        let current = await invoke<ShareSessionInfo | null>("get_share_status");
        if (!current && filePath) {
          current = await invoke<ShareSessionInfo>("start_file_share", {
            filePath,
          });
        }

        if (current) {
          setSession(current);
        } else {
          setErrorMessage("Немає активної роздачі файлу.");
        }
      } catch (err) {
        setErrorMessage(`Помилка запуску роздачі: ${String(err)}`);
      }
    };

    init();

    const unlistenStarted = listen("share_download_started", () => {
      setTransferStatus("downloading");
    });
    const unlistenCompleted = listen("share_download_completed", () => {
      setTransferStatus("completed");
    });

    return () => {
      unlistenStarted.then((fn) => fn());
      unlistenCompleted.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    if (!activeUrl) return;
    QRCode.toDataURL(activeUrl, {
      width: 240,
      margin: 1,
      color: {
        dark: "#090d16",
        light: "#ffffff",
      },
    })
      .then(setQrCodeDataUrl)
      .catch((err) => console.error("QR Code generation error:", err));
  }, [activeUrl]);

  const handleCopy = async () => {
    if (!activeUrl) return;
    try {
      await navigator.clipboard.writeText(activeUrl);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // fallback
    }
  };

  const handleStop = async () => {
    try {
      await invoke("stop_file_share");
    } catch (err) {
      console.error("stop_file_share error:", err);
    }
    try {
      const win = getCurrentWindow();
      await win.close();
    } catch {
      window.close();
    }
  };

  if (errorMessage) {
    return (
      <div className="w-screen h-screen flex flex-col items-center justify-center bg-slate-950 text-white p-6 select-none font-sans">
        <div className="bg-slate-900 border border-slate-800 rounded-xl p-6 text-center max-w-sm">
          <div className="text-rose-400 text-sm mb-4">{errorMessage}</div>
          <button
            onClick={() => getCurrentWindow().close()}
            className="px-4 py-2 bg-slate-800 hover:bg-slate-700 rounded text-xs"
          >
            Закрити вікно
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="w-screen h-screen flex flex-col bg-slate-950 text-white select-none font-sans overflow-hidden">
      {/* Шапка */}
      <div className="bg-slate-900 border-b border-slate-800 px-4 py-3 flex items-center justify-between shrink-0">
        <div className="flex items-center gap-2">
          <Wifi className="w-4 h-4 text-indigo-400" />
          <span className="font-semibold text-xs tracking-wide text-slate-100">
            Wi-Fi Quick Share
          </span>
        </div>
        <div className="flex items-center gap-1.5 text-[11px] text-emerald-400 font-mono">
          <Radio className="w-3 h-3 animate-pulse" />
          Роздача активна
        </div>
      </div>

      {/* Основний вміст */}
      <div className="flex-1 flex flex-col items-center justify-center p-5 text-center overflow-y-auto">
        {/* QR Код */}
        {qrCodeDataUrl ? (
          <div className="relative p-3 bg-white rounded-2xl shadow-xl shadow-indigo-500/10 mb-4 border-2 border-indigo-500/20">
            <img
              src={qrCodeDataUrl}
              alt="QR Code"
              className="w-48 h-48 block rounded-lg"
            />
          </div>
        ) : (
          <div className="w-48 h-48 bg-slate-900 rounded-2xl flex items-center justify-center mb-4 text-slate-500 text-xs">
            Генерація QR-коду...
          </div>
        )}

        {/* Перемикач режимів (HTTPS-тунель vs Локальний Wi-Fi) */}
        {session && session.local_url && session.local_url !== session.share_url && (
          <div className="flex items-center gap-1 bg-slate-900/90 p-1 rounded-xl mb-3 border border-slate-800 text-xs">
            <button
              onClick={() => setSelectedMode("tunnel")}
              className={`px-3 py-1 rounded-lg font-medium transition-all ${
                selectedMode === "tunnel"
                  ? "bg-indigo-600 text-white shadow-sm shadow-indigo-500/30"
                  : "text-slate-400 hover:text-white"
              }`}
            >
              🔒 HTTPS-тунель
            </button>
            <button
              onClick={() => setSelectedMode("local")}
              className={`px-3 py-1 rounded-lg font-medium transition-all ${
                selectedMode === "local"
                  ? "bg-indigo-600 text-white shadow-sm shadow-indigo-500/30"
                  : "text-slate-400 hover:text-white"
              }`}
            >
              📶 Локальний Wi-Fi
            </button>
          </div>
        )}

        {/* Підказка */}
        <p className="text-xs text-slate-400 mb-3">
          {activeUrl.startsWith("https://") ? (
            <span className="text-emerald-400 font-medium flex items-center justify-center gap-1.5">
              <span className="w-2 h-2 rounded-full bg-emerald-400 animate-ping" />
              🔒 Захищений HTTPS-тунель активний
            </span>
          ) : (
            <span className="text-amber-400 font-medium">
              📶 Пряма LAN-роздача (одна Wi-Fi мережа)
            </span>
          )}
        </p>

        {/* URL та кнопка копіювання */}
        {session && (
          <div className="flex items-center gap-2 bg-slate-900 border border-slate-800 rounded-xl px-3 py-1.5 max-w-xs w-full mb-3">
            <span className="font-mono text-xs text-indigo-300 truncate flex-1 text-left">
              {activeUrl}
            </span>
            <button
              onClick={handleCopy}
              className="p-1 hover:bg-slate-800 rounded text-slate-400 hover:text-white transition-colors"
              title="Копіювати URL"
            >
              {copied ? (
                <Check className="w-3.5 h-3.5 text-emerald-400" />
              ) : (
                <Copy className="w-3.5 h-3.5" />
              )}
            </button>
          </div>
        )}

        {/* Інформація про файл / папку */}
        {session && (
          <div className="bg-slate-900/60 border border-slate-800/80 rounded-xl p-3 max-w-xs w-full mb-4 text-left flex items-center gap-3">
            {session.is_dir ? (
              <FolderArchive className="w-7 h-7 text-amber-400 shrink-0" />
            ) : (
              <FileText className="w-7 h-7 text-indigo-400 shrink-0" />
            )}
            <div className="truncate flex-1">
              <div className="text-xs font-medium text-slate-200 truncate">
                {session.file_name}
              </div>
              <div className="text-[11px] text-slate-400 font-mono">
                {formatBytes(session.file_size)}{" "}
                {session.is_dir && "• ZIP-архів"}
              </div>
            </div>
          </div>
        )}

        {/* Статус завантаження */}
        <div className="text-[11px] mb-4">
          {transferStatus === "waiting" && (
            <span className="text-slate-400">
              Очікування підключення пристрою...
            </span>
          )}
          {transferStatus === "downloading" && (
            <span className="text-amber-400 font-medium animate-pulse">
              📲 Телефон завантажує файл...
            </span>
          )}
          {transferStatus === "completed" && (
            <span className="text-emerald-400 font-medium">
              ✅ Файл успішно завантажено на телефон!
            </span>
          )}
        </div>

        {/* Кнопка зупинки */}
        <button
          onClick={handleStop}
          className="flex items-center gap-2 bg-rose-600/20 hover:bg-rose-600/30 text-rose-300 border border-rose-500/30 text-xs font-medium px-5 py-2 rounded-xl transition-colors"
        >
          <Square className="w-3 h-3 fill-rose-300" />
          Зупинити роздачу
        </button>
      </div>
    </div>
  );
};
