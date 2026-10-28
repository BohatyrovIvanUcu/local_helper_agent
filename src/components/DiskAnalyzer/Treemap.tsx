import React, { useMemo, useState } from "react";

export interface TreemapNode {
  name: string;
  path: string;
  size: number;
  is_dir: boolean;
  extension?: string;
  color: string;
  children?: TreemapNode[];
}

export interface TileRect {
  id: string;
  x: number;
  y: number;
  w: number;
  h: number;
  node: TreemapNode;
  depth: number;
  isLeaf: boolean;
}

function formatBytes(bytes: number): string {
  if (!bytes || bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`;
}

// Функція оцінки співвідношення сторін для алгоритму Squarified Treemap (Bruls, Huizing, van Wijk)
function worstAspectRatio(rowAreas: number[], s: number): number {
  const sum = rowAreas.reduce((a, b) => a + b, 0);
  if (sum === 0 || s === 0) return Infinity;
  const sumSq = sum * sum;
  const sSq = s * s;
  let maxAspect = 0;
  for (const area of rowAreas) {
    if (area <= 0) continue;
    const a = (sSq * area) / sumSq;
    const b = sumSq / (sSq * area);
    const aspect = Math.max(a, b);
    if (aspect > maxAspect) maxAspect = aspect;
  }
  return maxAspect;
}

// Справжній алгоритм 2D Squarified Treemap з рекурсивним розбиттям підпапок
function layoutSquarified(
  items: TreemapNode[],
  x: number,
  y: number,
  w: number,
  h: number,
  depth: number,
  results: TileRect[]
) {
  if (items.length === 0 || w < 2 || h < 2) return;

  const validItems = items
    .filter((it) => it.size > 0)
    .sort((a, b) => b.size - a.size);

  if (validItems.length === 0) return;

  const totalSize = validItems.reduce((acc, it) => acc + it.size, 0);
  if (totalSize === 0) return;

  const totalArea = w * h;
  const itemAreas = validItems.map((it) => (it.size / totalSize) * totalArea);

  let curX = x;
  let curY = y;
  let curW = w;
  let curH = h;

  let currentRow: { node: TreemapNode; area: number }[] = [];
  let currentRowAreas: number[] = [];

  const flushRow = () => {
    if (currentRow.length === 0) return;
    const rowAreaSum = currentRowAreas.reduce((a, b) => a + b, 0);
    const isVertical = curW >= curH;

    if (isVertical) {
      // Вертикальна смуга вздовж висоти curH
      const rowWidth = Math.min(curW, rowAreaSum / curH);
      let itemY = curY;

      for (let i = 0; i < currentRow.length; i++) {
        const item = currentRow[i];
        const itemHeight = Math.max(1, (item.area / rowAreaSum) * curH);
        placeTile(
          item.node,
          curX,
          itemY,
          rowWidth,
          itemHeight,
          depth,
          results
        );
        itemY += itemHeight;
      }

      curX += rowWidth;
      curW = Math.max(0, curW - rowWidth);
    } else {
      // Горизонтальна смуга вздовж ширини curW
      const rowHeight = Math.min(curH, rowAreaSum / curW);
      let itemX = curX;

      for (let i = 0; i < currentRow.length; i++) {
        const item = currentRow[i];
        const itemWidth = Math.max(1, (item.area / rowAreaSum) * curW);
        placeTile(
          item.node,
          itemX,
          curY,
          itemWidth,
          rowHeight,
          depth,
          results
        );
        itemX += itemWidth;
      }

      curY += rowHeight;
      curH = Math.max(0, curH - rowHeight);
    }

    currentRow = [];
    currentRowAreas = [];
  };

  for (let i = 0; i < validItems.length; i++) {
    const node = validItems[i];
    const area = itemAreas[i];
    const s = Math.min(curW, curH);

    if (currentRow.length === 0) {
      currentRow.push({ node, area });
      currentRowAreas.push(area);
      continue;
    }

    const currentWorst = worstAspectRatio(currentRowAreas, s);
    const nextWorst = worstAspectRatio([...currentRowAreas, area], s);

    if (nextWorst <= currentWorst) {
      currentRow.push({ node, area });
      currentRowAreas.push(area);
    } else {
      flushRow();
      currentRow.push({ node, area });
      currentRowAreas.push(area);
    }
  }

  flushRow();
}

function placeTile(
  node: TreemapNode,
  x: number,
  y: number,
  w: number,
  h: number,
  depth: number,
  results: TileRect[]
) {
  const hasChildren = Boolean(
    node.children &&
      node.children.length > 0 &&
      node.children.some((c) => c.size > 0)
  );

  // Якщо це папка з дочірніми елементами і вона достатньо велика — розбиваємо її всередині!
  if (hasChildren && w >= 28 && h >= 28 && depth < 3) {
    const pad = 1.5;

    results.push({
      id: `c-${depth}-${node.path.replace(/[^a-zA-Z0-9_-]/g, "_")}`,
      x,
      y,
      w,
      h,
      node,
      depth,
      isLeaf: false,
    });

    layoutSquarified(
      node.children!,
      x + pad,
      y + pad,
      Math.max(1, w - pad * 2),
      Math.max(1, h - pad * 2),
      depth + 1,
      results
    );
  } else {
    // Кінцевий блок (файл або компактна папка)
    results.push({
      id: `l-${depth}-${node.path.replace(/[^a-zA-Z0-9_-]/g, "_")}`,
      x,
      y,
      w,
      h,
      node,
      depth,
      isLeaf: true,
    });
  }
}

interface TreemapProps {
  rootNode: TreemapNode | null;
  width: number;
  height: number;
  selectedPath?: string;
  onSelect: (node: TreemapNode) => void;
  onContextMenu: (e: React.MouseEvent, node: TreemapNode) => void;
}

export const Treemap: React.FC<TreemapProps> = ({
  rootNode,
  width,
  height,
  selectedPath,
  onSelect,
  onContextMenu,
}) => {
  const [hoveredNode, setHoveredNode] = useState<{
    node: TreemapNode;
    x: number;
    y: number;
  } | null>(null);

  const tiles = useMemo(() => {
    if (!rootNode || width <= 10 || height <= 10) return [];
    const results: TileRect[] = [];

    // Якщо у кореня є діти — розміщуємо їх, інакше сам корінь
    if (rootNode.children && rootNode.children.length > 0) {
      layoutSquarified(rootNode.children, 0, 0, width, height, 0, results);
    } else {
      results.push({
        id: rootNode.path,
        x: 0,
        y: 0,
        w: width,
        h: height,
        node: rootNode,
        depth: 0,
        isLeaf: true,
      });
    }

    return results;
  }, [rootNode, width, height]);

  if (!rootNode || tiles.length === 0) {
    return (
      <div className="w-full h-full flex items-center justify-center text-slate-500 text-xs font-sans">
        Немає файлів для відображення карти
      </div>
    );
  }

  // Розділяємо контейнери папок та кінцеві тайли файлів (WizTree cushion blocks)
  const containerTiles = tiles.filter((t) => !t.isLeaf);
  const leafTiles = tiles.filter((t) => t.isLeaf);

  return (
    <div className="relative w-full h-full select-none overflow-hidden bg-slate-950 font-sans">
      <svg
        width={width}
        height={height}
        className="block w-full h-full shape-rendering-crispEdges"
      >
        <defs>
          {/* Світлотіньовий фільтр (Bevel / Cushion) як у WizTree / WinDirStat */}
          <linearGradient id="wizCushion" x1="0%" y1="0%" x2="100%" y2="100%">
            <stop offset="0%" stopColor="#ffffff" stopOpacity="0.22" />
            <stop offset="50%" stopColor="#000000" stopOpacity="0" />
            <stop offset="100%" stopColor="#000000" stopOpacity="0.38" />
          </linearGradient>

          {/* Індивідуальні обрізки (clipPath) для кожного тайла, щоб текст ніколи не вилазив за межі блоку */}
          {leafTiles.map((tile) => {
            if (tile.w < 50 || tile.h < 22) return null;
            return (
              <clipPath key={`clip-${tile.id}`} id={`clip-${tile.id}`}>
                <rect
                  x={tile.x + 2}
                  y={tile.y + 2}
                  width={Math.max(0, tile.w - 4)}
                  height={Math.max(0, tile.h - 4)}
                />
              </clipPath>
            );
          })}
        </defs>

        {/* 1. Контейнери папок (тільки чисті розділові рамки груп без плаваючого тексту, щоб не налізати один на одного) */}
        {containerTiles.map((tile) => {
          const isSelected = selectedPath === tile.node.path;

          return (
            <rect
              key={tile.id}
              x={tile.x}
              y={tile.y}
              width={Math.max(1, tile.w)}
              height={Math.max(1, tile.h)}
              fill="none"
              stroke={isSelected ? "#818cf8" : "rgba(255, 255, 255, 0.12)"}
              strokeWidth={isSelected ? 2 : 1}
              rx={1}
            />
          );
        })}

        {/* 2. Кінцеві прямокутники (WizTree Cushion Blocks з текстом суворо всередині квадрата) */}
        {leafTiles.map((tile) => {
          const isSelected = selectedPath === tile.node.path;
          const hasText = tile.w >= 50 && tile.h >= 22;
          const showSize = tile.w >= 60 && tile.h >= 34;

          return (
            <g
              key={tile.id}
              onClick={(e) => {
                e.stopPropagation();
                onSelect(tile.node);
              }}
              onContextMenu={(e) => {
                e.preventDefault();
                e.stopPropagation();
                onContextMenu(e, tile.node);
              }}
              onMouseEnter={(e) => {
                setHoveredNode({
                  node: tile.node,
                  x: e.clientX,
                  y: e.clientY,
                });
              }}
              onMouseMove={(e) => {
                setHoveredNode({
                  node: tile.node,
                  x: e.clientX,
                  y: e.clientY,
                });
              }}
              onMouseLeave={() => setHoveredNode(null)}
              className="cursor-pointer transition-opacity hover:opacity-95"
            >
              {/* Базовий колір блоку за розширенням */}
              <rect
                x={tile.x + 0.5}
                y={tile.y + 0.5}
                width={Math.max(1, tile.w - 1)}
                height={Math.max(1, tile.h - 1)}
                fill={tile.node.color}
                stroke={isSelected ? "#ffffff" : "#020617"}
                strokeWidth={isSelected ? 2 : 1}
                rx={1}
              />

              {/* 3D Cushion Overlay для класичного ефекту WizTree */}
              <rect
                x={tile.x + 1}
                y={tile.y + 1}
                width={Math.max(1, tile.w - 2)}
                height={Math.max(1, tile.h - 2)}
                fill="url(#wizCushion)"
                className="pointer-events-none"
              />

              {/* Текст прямо на квадраті (суворо обрізаний clipPath, не з'являється на занадто малих блоках) */}
              {hasText && (
                <g clipPath={`url(#clip-${tile.id})`}>
                  <text
                    x={tile.x + 4}
                    y={tile.y + (showSize ? 13 : 15)}
                    fill="#ffffff"
                    fontSize={showSize ? 10 : 9.5}
                    fontWeight="600"
                    className="pointer-events-none select-none font-sans"
                    style={{ textShadow: "0 1px 2px #000, 0 0 2px #000" }}
                  >
                    {tile.node.name}
                  </text>

                  {showSize && (
                    <text
                      x={tile.x + 4}
                      y={tile.y + 25}
                      fill="#f1f5f9"
                      fontSize={8.5}
                      className="pointer-events-none select-none font-mono opacity-90"
                      style={{ textShadow: "0 1px 2px #000, 0 0 2px #000" }}
                    >
                      {formatBytes(tile.node.size)}
                    </text>
                  )}
                </g>
              )}
            </g>
          );
        })}
      </svg>

      {/* Спливаючий Tooltip при наведенні курсора */}
      {hoveredNode && (
        <div
          className="fixed pointer-events-none z-50 bg-slate-900/95 border border-slate-700 backdrop-blur-md rounded-lg px-3 py-2 text-xs shadow-2xl text-white max-w-sm"
          style={{
            left: Math.min(window.innerWidth - 300, hoveredNode.x + 14),
            top: Math.min(window.innerHeight - 90, hoveredNode.y + 14),
          }}
        >
          <div className="font-semibold text-slate-100 truncate mb-1">
            {hoveredNode.node.name}
          </div>
          <div className="flex items-center gap-2 text-slate-300">
            <span>Розмір:</span>
            <span className="font-mono text-emerald-400 font-bold">
              {formatBytes(hoveredNode.node.size)}
            </span>
          </div>
          <div className="text-[10px] text-slate-400 truncate mt-1 font-mono">
            {hoveredNode.node.path}
          </div>
        </div>
      )}
    </div>
  );
};
