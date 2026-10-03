import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Node } from "../lib/api";
import { buildTiles, type Tile } from "../lib/treemap";
import { formatBytes, percent } from "../lib/format";

interface Props {
  root: Node;
  selectedPath: string | null;
  onSelect: (n: Node) => void;
  onZoom: (n: Node) => void;
}

interface HoverState {
  tile: Tile;
  x: number;
  y: number;
}

export function Treemap({ root, selectedPath, onSelect, onZoom }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ w: 900, h: 640 });
  const [hover, setHover] = useState<HoverState | null>(null);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const update = () => setSize({ w: el.clientWidth, h: el.clientHeight });
    update();
    const ro = new ResizeObserver(update);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const tiles = useMemo(
    () =>
      buildTiles(root, { x: 0, y: 0, w: size.w, h: size.h }, { maxDepth: 4, minTileSize: 34 }),
    [root, size.w, size.h],
  );

  const handleLeave = useCallback(() => setHover(null), []);

  return (
    <div className="treemap" ref={containerRef}>
      <svg
        width={size.w}
        height={size.h}
        viewBox={`0 0 ${size.w} ${size.h}`}
        onMouseLeave={handleLeave}
      >
        {tiles.map((tile, i) => {
          const { rect, node, depth, isFile } = tile;
          if (rect.w < 1 || rect.h < 1) return null;
          const isSelected = selectedPath === node.path;
          const isHovered = hover?.tile.node.path === node.path;
          const inset = isFile || depth === 0 ? 0.5 : 1;
          const canZoom = node.isDir && (node.truncated || (node.children?.length ?? 0) > 0);

          return (
            <rect
              key={`rect-${node.path}-${i}`}
              x={rect.x + inset}
              y={rect.y + inset}
              width={Math.max(rect.w - inset * 2, 0)}
              height={Math.max(rect.h - inset * 2, 0)}
              rx={isFile ? 2 : 3}
              fill={tile.color}
              fillOpacity={isHovered ? 1 : 0.92}
              stroke={isSelected || isHovered ? "#ffffff" : tile.stroke}
              strokeWidth={isSelected ? 2 : isHovered ? 1.5 : 0.6}
              strokeDasharray={node.truncated && !isFile ? "4 3" : undefined}
              className={canZoom ? "tile tile-zoomable" : "tile"}
              onMouseMove={(e) => {
                const box = containerRef.current?.getBoundingClientRect();
                setHover({
                  tile,
                  x: e.clientX - (box?.left ?? 0),
                  y: e.clientY - (box?.top ?? 0),
                });
              }}
              onClick={() => onSelect(node)}
              onDoubleClick={() => (canZoom ? onZoom(node) : onSelect(node))}
            />
          );
        })}

        <g pointerEvents="none">
          {tiles.map((tile, i) => {
            const { rect, node, depth, isFile } = tile;
            if (rect.w < 62 || rect.h < 18) return null;
            const maxChars = Math.max(4, Math.floor((rect.w - 10) / 7));
            const text =
              node.name.length > maxChars ? `${node.name.slice(0, maxChars - 1)}…` : node.name;
            const dark = !isFile && depth >= 2;
            const fill = isFile ? "#f4f6fb" : dark ? "#e8ecf6" : "#ffffff";
            // 占用大小只显示在当前层级（视图第一层），更深的层级省略，避免标签拥挤。
            const showSize = depth === 0 && rect.h >= 56 && rect.w >= 90;
            // 名称与大小之间留出间距（两行基线相距 15px）。
            const nameY = isFile
              ? rect.y + rect.h / 2 + (showSize ? -7 : 4)
              : rect.y + (showSize ? 12 : 11);
            const sizeY = isFile ? rect.y + rect.h / 2 + 8 : rect.y + 27;
            return (
              <g key={`label-${node.path}-${i}`}>
                <text
                  x={rect.x + 5}
                  y={nameY}
                  fontSize={tile.depth === 0 ? 12.5 : 11}
                  fill={fill}
                  fontWeight={node.isDir ? 600 : 400}
                  className="tile-label"
                >
                  {node.isDir ? text : `📄 ${text}`}
                </text>
                {showSize && (
                  <text
                    x={rect.x + 5}
                    y={sizeY}
                    fontSize={10.5}
                    fill={fill}
                    fillOpacity={0.72}
                    className="tile-label"
                  >
                    {formatBytes(node.size, 1)}
                  </text>
                )}
              </g>
            );
          })}
        </g>
      </svg>

      {hover && (
        <div
          className="treemap-tooltip"
          style={{
            left: Math.min(hover.x + 14, Math.max(size.w - 310, 0)),
            top: Math.min(hover.y + 14, Math.max(size.h - 120, 0)),
          }}
        >
          <div className="tt-name">
            {hover.tile.node.isDir ? "📁" : "📄"} {hover.tile.node.name}
          </div>
          <div className="tt-row">
            <span>大小</span>
            <b>{formatBytes(hover.tile.node.size)}</b>
          </div>
          <div className="tt-row">
            <span>占当前视图</span>
            <b>{percent(hover.tile.node.size, root.size)}</b>
          </div>
          {hover.tile.node.isDir && (
            <div className="tt-row">
              <span>文件数</span>
              <b>{hover.tile.node.files.toLocaleString("zh-CN")}</b>
            </div>
          )}
          <div className="tt-path">{hover.tile.node.path}</div>
          <div className="tt-hint">
            {hover.tile.node.isDir &&
            (hover.tile.node.truncated || (hover.tile.node.children?.length ?? 0) > 0)
              ? "单击选中 · 双击进入"
              : "单击选中"}
          </div>
        </div>
      )}
    </div>
  );
}
