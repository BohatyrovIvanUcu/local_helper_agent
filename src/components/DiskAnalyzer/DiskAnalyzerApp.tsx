import React, { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  AlertTriangle,
  ChevronDown,
  ChevronRight,
  Folder,
  HardDrive,
  RefreshCw,
  Trash2,
  ExternalLink,
} from "lucide-react";
import { Treemap, type TreemapNode } from "./Treemap";

interface DiskItem {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
  children_count: number;
  children?: DiskItem[];
  extension?: string;
}

interface ExtensionStat {
  extension: string;
  total_size: number;
  count: number;
  color: string;
}

interface DevClutterItem {
  path: string;
  name: string;
  category: string;
  size: number;
}

interface DiskScanResult {
  root_path: string;
  total_space: number;
  available_space: number;
  used_space: number;
  root_item: DiskItem;
  extensions: ExtensionStat[];
  dev_clutter: DevClutterItem[];
  scan_duration_ms: number;
}

function formatBytes(bytes: number): string {
  if (!bytes || bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`;
}

export const DiskAnalyzerApp: React.FC = () => {
  const [currentPath, setCurrentPath] = useState<string>("~");
  const [isScanning, setIsScanning] = useState<boolean>(false);
  const [scanResult, setScanResult] = useState<DiskScanResult | null>(null);
  const [selectedNode, setSelectedNode] = useState<DiskItem | null>(null);
  const [expandedPaths, setExpandedPaths] = useState<Set<string>>(new Set());

  // Контекстне меню
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    item: { path: string; name: string; size: number; is_dir: boolean };
  } | null>(null);

  // Діалог підтвердження видалення
  const [deleteConfirm, setDeleteConfirm] = useState<{
    path: string;
    name: string;
    size: number;
    toTrash: boolean;
  } | null>(null);

  // Виміри для Treemap
  const treemapRef = useRef<HTMLDivElement>(null);
  const [treemapDimensions, setTreemapDimensions] = useState<{
    width: number;
    height: number;
  }>({ width: 800, height: 260 });

  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const initialPath = params.get("path") || "~";
    setCurrentPath(initialPath);
    startScan(initialPath);
  }, []);

  useEffect(() => {
    if (!treemapRef.current) return;
    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        setTreemapDimensions({
          width: Math.max(300, entry.contentRect.width),
          height: Math.max(150, entry.contentRect.height),
        });
      }
    });
    observer.observe(treemapRef.current);
    return () => observer.disconnect();
  }, [scanResult]);

  // Закриття контекстного меню при кліку в будь-якому місці
  useEffect(() => {
    const handleClick = () => setContextMenu(null);
    window.addEventListener("click", handleClick);
    return () => window.removeEventListener("click", handleClick);
  }, []);

  const startScan = async (pathTarget?: string) => {
    const target = pathTarget !== undefined ? pathTarget : currentPath;
    setIsScanning(true);
    try {
      const res = await invoke<DiskScanResult>("scan_disk_space", {
        targetPath: target,
        maxDepth: 4,
      });
      setScanResult(res);
      setSelectedNode(res.root_item);
      setExpandedPaths(new Set([res.root_item.path]));
    } catch (err) {
      console.error("Scan error:", err);
      alert(`Помилка сканування: ${String(err)}`);
    } finally {
      setIsScanning(false);
    }
  };

  const toggleExpand = (path: string) => {
    setExpandedPaths((prev) => {
      const next = new Set(prev);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  };

  const handleOpenInFileManager = async (path: string) => {
    try {
      await invoke("reveal_in_file_manager", { path });
    } catch (err) {
      console.error(err);
    }
  };

  const handleExecuteDelete = async () => {
    if (!deleteConfirm) return;
    const { path, toTrash } = deleteConfirm;
    setDeleteConfirm(null);
    try {
      await invoke("delete_disk_item", { path, toTrash });
      // Оновлюємо сканування після видалення
      await startScan(currentPath);
    } catch (err) {
      alert(`Помилка видалення: ${String(err)}`);
    }
  };

  // Перетворюємо ієрархію вузлів у дерево з підпапками для Treemap
  const treemapTree: TreemapNode | null = React.useMemo(() => {
    if (!scanResult) return null;
    const extColorMap = new Map<string, string>();
    for (const ext of scanResult.extensions) {
      extColorMap.set(ext.extension.toLowerCase(), ext.color);
    }

    const mapItem = (item: DiskItem): TreemapNode => {
      const ext = item.extension?.toLowerCase() || "";
      const color = item.is_dir
        ? "#3b82f6"
        : extColorMap.get(ext) || "#64748b";

      return {
        name: item.name,
        path: item.path,
        size: item.size,
        is_dir: item.is_dir,
        extension: item.extension,
        color,
        children: item.children ? item.children.map(mapItem) : undefined,
      };
    };

    const targetItem = selectedNode || scanResult.root_item;
    return mapItem(targetItem);
  }, [scanResult, selectedNode]);

  const usedPercent =
    scanResult && scanResult.total_space > 0
      ? ((scanResult.used_space / scanResult.total_space) * 100).toFixed(1)
      : "0";

  const renderTreeItem = (
    item: DiskItem,
    parentSize: number,
    depth: number = 0
  ) => {
    const isExpanded = expandedPaths.has(item.path);
    const isSelected = selectedNode?.path === item.path;
    const percentOfParent =
      parentSize > 0 ? ((item.size / parentSize) * 100).toFixed(1) : "0";

    return (
      <div key={item.path} className="flex flex-col">
        <div
          onClick={() => setSelectedNode(item)}
          onContextMenu={(e) => {
            e.preventDefault();
            setContextMenu({
              x: e.clientX,
              y: e.clientY,
              item: {
                path: item.path,
                name: item.name,
                size: item.size,
                is_dir: item.is_dir,
              },
            });
          }}
          className={`flex items-center text-xs py-1 px-2 cursor-pointer transition-colors border-b border-slate-900/40 select-none ${
            isSelected
              ? "bg-indigo-600/30 text-white font-medium"
              : "hover:bg-slate-800/60 text-slate-300"
          }`}
          style={{ paddingLeft: `${depth * 16 + 8}px` }}
        >
          {/* Кнопка розгортання */}
          <button
            onClick={(e) => {
              e.stopPropagation();
              toggleExpand(item.path);
            }}
            className="w-4 h-4 flex items-center justify-center mr-1 text-slate-400 hover:text-white"
          >
            {item.is_dir && item.children && item.children.length > 0 ? (
              isExpanded ? (
                <ChevronDown className="w-3.5 h-3.5" />
              ) : (
                <ChevronRight className="w-3.5 h-3.5" />
              )
            ) : (
              <span className="w-3.5" />
            )}
          </button>

          {/* Іконка */}
          {item.is_dir ? (
            <Folder className="w-3.5 h-3.5 mr-1.5 text-amber-400 shrink-0" />
          ) : (
            <span className="w-3.5 h-3.5 mr-1.5 inline-block text-center text-[10px] text-slate-400">
              📄
            </span>
          )}

          {/* Назва */}
          <span className="truncate flex-1 pr-2" title={item.path}>
            {item.name}
          </span>

          {/* % від батька */}
          <div className="w-24 flex items-center gap-1.5 shrink-0 px-2">
            <div className="flex-1 bg-slate-800 rounded-full h-1.5 overflow-hidden">
              <div
                className="bg-indigo-500 h-full rounded-full"
                style={{ width: `${Math.min(100, Number(percentOfParent))}%` }}
              />
            </div>
            <span className="text-[10px] font-mono text-slate-400 w-9 text-right">
              {percentOfParent}%
            </span>
          </div>

          {/* Розмір */}
          <span className="w-20 font-mono text-right text-emerald-400 shrink-0">
            {formatBytes(item.size)}
          </span>

          {/* Кількість */}
          <span className="w-14 font-mono text-right text-slate-500 shrink-0">
            {item.is_dir ? item.children_count : 1}
          </span>
        </div>

        {/* Дочірні елементи */}
        {isExpanded && item.children && item.children.length > 0 && (
          <div>
            {item.children.map((child) =>
              renderTreeItem(child, item.size, depth + 1)
            )}
          </div>
        )}
      </div>
    );
  };

  return (
    <div className="w-screen h-screen flex flex-col bg-slate-950 text-slate-100 overflow-hidden select-none font-sans">
      {/* 1. Верхня панель інструментів */}
      <div className="bg-slate-900 border-b border-slate-800 p-2.5 flex items-center justify-between gap-3 shrink-0">
        <div className="flex items-center gap-2 flex-1">
          <HardDrive className="w-5 h-5 text-indigo-400 shrink-0" />
          <span className="text-xs font-semibold text-slate-400 shrink-0">
            Шлях:
          </span>
          <div className="relative flex-1 max-w-md flex items-center">
            <input
              type="text"
              value={currentPath}
              onChange={(e) => setCurrentPath(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && startScan()}
              className="w-full bg-slate-950 border border-slate-700 rounded px-2.5 py-1 text-xs text-white focus:outline-none focus:border-indigo-500 font-mono"
            />
          </div>

          {/* Швидкі пресети */}
          <div className="flex items-center gap-1">
            <button
              onClick={() => {
                setCurrentPath("~");
                startScan("~");
              }}
              className="text-[11px] bg-slate-800 hover:bg-slate-700 px-2 py-1 rounded text-slate-300 transition-colors"
            >
              Домашня (~)
            </button>
            <button
              onClick={() => {
                setCurrentPath("~/Downloads");
                startScan("~/Downloads");
              }}
              className="text-[11px] bg-slate-800 hover:bg-slate-700 px-2 py-1 rounded text-slate-300 transition-colors"
            >
              Downloads
            </button>
            <button
              onClick={() => {
                setCurrentPath("/");
                startScan("/");
              }}
              className="text-[11px] bg-slate-800 hover:bg-slate-700 px-2 py-1 rounded text-slate-300 transition-colors"
            >
              Корінь (/)
            </button>
          </div>

          <button
            onClick={() => startScan()}
            disabled={isScanning}
            className="flex items-center gap-1.5 bg-indigo-600 hover:bg-indigo-500 disabled:bg-slate-700 text-white text-xs font-medium px-3 py-1.5 rounded transition-colors ml-2 shadow-sm"
          >
            <RefreshCw
              className={`w-3.5 h-3.5 ${isScanning ? "animate-spin" : ""}`}
            />
            {isScanning ? "Сканування..." : "Сканувати"}
          </button>
        </div>

        {/* Статистика файлової системи */}
        {scanResult && (
          <div className="flex items-center gap-3 shrink-0 text-xs">
            <div className="flex flex-col items-end">
              <div className="text-[11px] text-slate-400">
                Диск:{" "}
                <span className="text-white font-medium">
                  {formatBytes(scanResult.used_space)}
                </span>{" "}
                / {formatBytes(scanResult.total_space)} ({usedPercent}%)
              </div>
              <div className="w-32 bg-slate-800 h-1.5 rounded-full overflow-hidden mt-1">
                <div
                  className="bg-indigo-500 h-full rounded-full"
                  style={{ width: `${Math.min(100, Number(usedPercent))}%` }}
                />
              </div>
            </div>
            <div className="border-l border-slate-800 pl-3 text-right">
              <span className="text-[10px] text-slate-400 block">Час:</span>
              <span className="font-mono text-emerald-400 font-medium">
                {(scanResult.scan_duration_ms / 1000).toFixed(2)} с
              </span>
            </div>
          </div>
        )}
      </div>

      {/* 2. Середня частина: Дерево (зліва) та Розширення (справа) */}
      <div className="flex-1 flex overflow-hidden border-b border-slate-800 min-h-[220px]">
        {/* Ліва колонка: Деревоподібний список */}
        <div className="flex-1 flex flex-col border-r border-slate-800 bg-slate-900/30 overflow-hidden">
          {/* Заголовки таблиці дерева */}
          <div className="flex items-center text-[11px] font-semibold text-slate-400 bg-slate-900 border-b border-slate-800 py-1 px-2 select-none">
            <span className="flex-1 pl-6">Папка / Файл</span>
            <span className="w-24 text-right pr-2">% від батька</span>
            <span className="w-20 text-right">Розмір</span>
            <span className="w-14 text-right pr-1">Елементи</span>
          </div>

          {/* Вміст дерева */}
          <div className="flex-1 overflow-y-auto">
            {scanResult ? (
              renderTreeItem(
                scanResult.root_item,
                scanResult.root_item.size,
                0
              )
            ) : (
              <div className="p-8 text-center text-slate-500 text-xs">
                {isScanning ? "Сканування директорії..." : "Натисніть «Сканувати» для аналізу"}
              </div>
            )}
          </div>
        </div>

        {/* Права колонка: Таблиця розширень у стилі WizTree */}
        <div className="w-80 flex flex-col bg-slate-900/50 shrink-0 overflow-hidden">
          <div className="flex items-center text-[11px] font-semibold text-slate-400 bg-slate-900 border-b border-slate-800 py-1 px-3">
            <span className="flex-1">Розширення</span>
            <span className="w-16 text-right">Розмір</span>
            <span className="w-14 text-right">К-сть</span>
          </div>

          <div className="flex-1 overflow-y-auto">
            {scanResult &&
              scanResult.extensions.slice(0, 40).map((ext) => (
                <div
                  key={ext.extension}
                  className="flex items-center text-xs py-1 px-3 border-b border-slate-900/40 hover:bg-slate-800/40"
                >
                  <span
                    className="w-2.5 h-2.5 rounded-sm mr-2 shrink-0"
                    style={{ backgroundColor: ext.color }}
                  />
                  <span className="font-mono font-medium flex-1 text-slate-200">
                    .{ext.extension}
                  </span>
                  <span className="w-16 font-mono text-right text-emerald-400">
                    {formatBytes(ext.total_size)}
                  </span>
                  <span className="w-14 font-mono text-right text-slate-400">
                    {ext.count}
                  </span>
                </div>
              ))}
          </div>
        </div>
      </div>

      {/* 3. Нижня частина: Справжній 2D Squarified Treemap (WizTree) */}
      <div className="h-72 flex flex-col bg-slate-950 shrink-0">
        <div className="bg-slate-900 px-3 py-1 text-[11px] font-semibold text-slate-400 border-b border-slate-800 flex items-center justify-between">
          <div className="flex items-center">
            <span>
              🗺️ Карта дискового простору (WizTree Treemap) —{" "}
              <span className="text-slate-200 font-normal">
                {selectedNode ? selectedNode.name : "Всі файли"}
              </span>
            </span>
            {selectedNode && selectedNode.path !== scanResult?.root_item.path && (
              <button
                onClick={() => setSelectedNode(scanResult?.root_item || null)}
                className="text-[10px] bg-slate-800 hover:bg-slate-700 px-2 py-0.5 rounded text-indigo-300 ml-2 transition-colors"
              >
                ← До кореня ({scanResult?.root_item.name})
              </button>
            )}
          </div>
          <span className="text-[10px] text-slate-500">
            Клік — вибрати • Правий клік — контекстне меню дій
          </span>
        </div>

        <div ref={treemapRef} className="flex-1 relative overflow-hidden">
          <Treemap
            rootNode={treemapTree}
            width={treemapDimensions.width}
            height={treemapDimensions.height}
            selectedPath={selectedNode?.path}
            onSelect={(node) => {
              if (scanResult) {
                // Знаходимо вузол в дереві або встановлюємо як активний
                setSelectedNode({
                  name: node.name,
                  path: node.path,
                  size: node.size,
                  is_dir: node.is_dir,
                  children_count: 0,
                  extension: node.extension,
                });
              }
            }}
            onContextMenu={(e, node) => {
              setContextMenu({
                x: e.clientX,
                y: e.clientY,
                item: {
                  path: node.path,
                  name: node.name,
                  size: node.size,
                  is_dir: node.is_dir,
                },
              });
            }}
          />
        </div>
      </div>

      {/* 4. Контекстне меню при правому кліку */}
      {contextMenu && (
        <div
          className="fixed z-50 bg-slate-900 border border-slate-700 rounded-lg shadow-2xl py-1 text-xs w-56 text-slate-200 backdrop-blur-md"
          style={{
            left: Math.min(window.innerWidth - 230, contextMenu.x),
            top: Math.min(window.innerHeight - 150, contextMenu.y),
          }}
          onClick={(e) => e.stopPropagation()}
        >
          <div className="px-3 py-1.5 border-b border-slate-800 font-medium truncate text-white">
            {contextMenu.item.name}
          </div>
          <button
            onClick={() => {
              handleOpenInFileManager(contextMenu.item.path);
              setContextMenu(null);
            }}
            className="w-full text-left px-3 py-1.5 hover:bg-slate-800 flex items-center gap-2"
          >
            <ExternalLink className="w-3.5 h-3.5 text-indigo-400" />
            Відкрити у файловому менеджері
          </button>
          <button
            onClick={() => {
              setDeleteConfirm({
                path: contextMenu.item.path,
                name: contextMenu.item.name,
                size: contextMenu.item.size,
                toTrash: true,
              });
              setContextMenu(null);
            }}
            className="w-full text-left px-3 py-1.5 hover:bg-amber-500/20 text-amber-300 flex items-center gap-2"
          >
            <Trash2 className="w-3.5 h-3.5" />
            Перемістити у кошик
          </button>
          <button
            onClick={() => {
              setDeleteConfirm({
                path: contextMenu.item.path,
                name: contextMenu.item.name,
                size: contextMenu.item.size,
                toTrash: false,
              });
              setContextMenu(null);
            }}
            className="w-full text-left px-3 py-1.5 hover:bg-rose-500/20 text-rose-400 flex items-center gap-2"
          >
            <Trash2 className="w-3.5 h-3.5" />
            Видалити назавжди
          </button>
        </div>
      )}

      {/* 5. Модальне вікно підтвердження видалення */}
      {deleteConfirm && (
        <div className="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm flex items-center justify-center p-4">
          <div className="bg-slate-900 border border-slate-700 rounded-xl p-5 max-w-md w-full shadow-2xl">
            <div className="flex items-center gap-3 mb-3 text-amber-400">
              <AlertTriangle className="w-6 h-6 shrink-0" />
              <h3 className="font-semibold text-base text-white">
                {deleteConfirm.toTrash
                  ? "Переміщення у кошик"
                  : "Безповоротне видалення"}
              </h3>
            </div>
            <p className="text-xs text-slate-300 mb-2">
              Ви впевнені, що бажаєте{" "}
              {deleteConfirm.toTrash
                ? "перемістити у кошик"
                : "назавжди видалити"}{" "}
              цей елемент?
            </p>
            <div className="bg-slate-950 p-2.5 rounded border border-slate-800 mb-4 font-mono text-[11px] text-slate-400 break-all">
              <div className="text-white font-medium mb-1 truncate">
                {deleteConfirm.name}
              </div>
              <div>Розмір: {formatBytes(deleteConfirm.size)}</div>
              <div className="text-[10px] text-slate-500 truncate mt-1">
                {deleteConfirm.path}
              </div>
            </div>
            <div className="flex justify-end gap-2 text-xs">
              <button
                onClick={() => setDeleteConfirm(null)}
                className="px-4 py-2 bg-slate-800 hover:bg-slate-700 rounded text-slate-300 transition-colors font-medium"
              >
                Скасувати
              </button>
              <button
                onClick={handleExecuteDelete}
                className={`px-4 py-2 rounded text-white font-medium transition-colors ${
                  deleteConfirm.toTrash
                    ? "bg-amber-600 hover:bg-amber-500"
                    : "bg-rose-600 hover:bg-rose-500"
                }`}
              >
                {deleteConfirm.toTrash
                  ? "Перемістити у кошик"
                  : "Видалити назавжди"}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
