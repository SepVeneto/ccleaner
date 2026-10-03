import type { Node } from "./api";
import { dirColor, dirStroke, extColor } from "./palette";

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Tile {
  node: Node;
  rect: Rect;
  depth: number;
  hue: number;
  color: string;
  stroke: string;
  isFile: boolean;
}

function worstRatio(row: number[], side: number, scale: number): number {
  if (row.length === 0) return Number.POSITIVE_INFINITY;
  const sum = row.reduce((a, b) => a + b, 0) * scale;
  if (sum <= 0) return Number.POSITIVE_INFINITY;
  const mx = Math.max(...row) * scale;
  const mn = Math.min(...row) * scale;
  const s2 = sum * sum;
  const l2 = side * side;
  return Math.max((l2 * mx) / s2, s2 / (l2 * mn || 1));
}

/**
 * Squarified treemap 布局：给定权重（按降序排好）与容器矩形，返回每个权重的矩形。
 * 这是 SpaceSniffer / WinDirStat 这类工具的核心布局算法。
 */
export function squarify(weights: number[], rect: Rect): Rect[] {
  const n = weights.length;
  const rects: Rect[] = new Array(n);
  if (n === 0) return rects;

  const order = weights
    .map((v, i) => ({ v: Math.max(v, 0), i }))
    .filter((o) => o.v > 0)
    .sort((a, b) => b.v - a.v);

  const total = order.reduce((a, o) => a + o.v, 0);
  if (total <= 0 || rect.w <= 0 || rect.h <= 0) {
    for (let i = 0; i < n; i++) rects[i] = { x: rect.x, y: rect.y, w: 0, h: 0 };
    return rects;
  }

  const scale = (rect.w * rect.h) / total;
  let x = rect.x;
  let y = rect.y;
  let w = rect.w;
  let h = rect.h;

  let row: { v: number; i: number }[] = [];
  let idx = 0;

  const layoutRow = () => {
    const sum = row.reduce((a, r) => a + r.v, 0) * scale;
    if (row.length === 0 || sum <= 0) return;
    if (w >= h) {
      const stripW = Math.min(sum / h, w);
      let cursor = y;
      row.forEach((r, k) => {
        const remain = h - (cursor - y);
        const rh = k === row.length - 1 ? remain : (r.v * scale) / stripW;
        rects[r.i] = { x, y: cursor, w: stripW, h: Math.max(rh, 0) };
        cursor += rh;
      });
      x += stripW;
      w -= stripW;
    } else {
      const stripH = Math.min(sum / w, h);
      let cursor = x;
      row.forEach((r, k) => {
        const remain = w - (cursor - x);
        const rw = k === row.length - 1 ? remain : (r.v * scale) / stripH;
        rects[r.i] = { x: cursor, y, w: Math.max(rw, 0), h: stripH };
        cursor += rw;
      });
      y += stripH;
      h -= stripH;
    }
    row = [];
  };

  while (idx < order.length) {
    const side = Math.min(w, h);
    const next = order[idx];
    const withNext = [...row.map((r) => r.v), next.v];
    if (row.length === 0 || worstRatio(withNext, side, scale) <= worstRatio(row.map((r) => r.v), side, scale)) {
      row.push(next);
      idx += 1;
    } else {
      layoutRow();
    }
  }
  layoutRow();

  for (let i = 0; i < n; i++) if (!rects[i]) rects[i] = { x: rect.x, y: rect.y, w: 0, h: 0 };
  return rects;
}

function pad(rect: Rect, t: number, r: number, b: number, l: number): Rect {
  return {
    x: rect.x + l,
    y: rect.y + t,
    w: Math.max(rect.w - l - r, 0),
    h: Math.max(rect.h - t - b, 0),
  };
}

export interface BuildOptions {
  maxDepth: number;
  minTileSize: number;
}

/**
 * 把目录树递归布局成扁平的瓦片列表。
 * 父瓦片先于子瓦片入列，渲染时后画的在上层。
 */
export function buildTiles(root: Node, rect: Rect, opts: BuildOptions): Tile[] {
  const tiles: Tile[] = [];

  const children = root.children ?? [];
  if (children.length === 0) return tiles;

  const weights = children.map((c) => c.size);
  const rects = squarify(weights, rect);

  // 顶层每个目录分配一个色相，其子目录共享该色调
  const hueStep = Math.max(1, Math.round(360 / Math.max(children.filter((c) => c.isDir).length, 1)));

  children.forEach((child, i) => {
    const r = rects[i];
    if (!r || r.w <= 0 || r.h <= 0) return;

    const hue = (i * hueStep * 1.7) % 360;
    const isFile = !child.isDir;
    const tile: Tile = {
      node: child,
      rect: r,
      depth: 0,
      hue,
      color: isFile ? extColor(child.ext) : dirColor(hue, 0),
      stroke: isFile ? "rgba(0,0,0,0.25)" : dirStroke(hue, 0),
      isFile,
    };
    tiles.push(tile);
    recurse(child, r, 1, hue, isFile);
  });

  function recurse(node: Node, parentRect: Rect, depth: number, hue: number, parentIsFile: boolean) {
    if (parentIsFile || depth > opts.maxDepth) return;
    const kids = node.children ?? [];
    if (kids.length === 0) return;
    if (parentRect.w < opts.minTileSize || parentRect.h < opts.minTileSize) return;

    // 目录顶部预留一条标题栏，避免标签压住子块。
    // 当前视图的第一层目录（后面跟着的子块 depth 为 1，说明父块是最上层）
    // 要在名称下方再显示占用大小，因此留一条更高的标题栏，保证两行之间有间距。
    const twoLineHeader = depth === 1 && parentRect.h >= 56 && parentRect.w >= 90;
    const headerH = twoLineHeader
      ? 28
      : Math.min(16, Math.max(parentRect.h * 0.12, 12));
    const inner = pad(parentRect, headerH, 2, 2, 2);
    if (inner.w <= 4 || inner.h <= 4) return;

    const weights = kids.map((c) => c.size);
    const rects = squarify(weights, inner);
    kids.forEach((child, i) => {
      const r = rects[i];
      if (!r || r.w <= 0 || r.h <= 0) return;
      const isFile = !child.isDir;
      const tile: Tile = {
        node: child,
        rect: r,
        depth,
        hue,
        color: isFile ? extColor(child.ext) : dirColor(hue, depth),
        stroke: isFile ? "rgba(0,0,0,0.22)" : dirStroke(hue, depth),
        isFile,
      };
      tiles.push(tile);
      recurse(child, r, depth + 1, hue, isFile);
    });
  }

  return tiles;
}

/** 在当前视图中按路径查找节点。 */
export function findNode(root: Node, path: string): Node | null {
  if (root.path.toLowerCase() === path.toLowerCase()) return root;
  for (const c of root.children ?? []) {
    const found = findNode(c, path);
    if (found) return found;
  }
  return null;
}
