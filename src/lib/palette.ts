// 目录用色相区分（同一顶层目录的子目录共享色调），文件按类型着色。

const DIR_SAT = [58, 52, 46, 42];
const DIR_LIGHT = [52, 42, 34, 28];

export function dirColor(hue: number, depth: number): string {
  const d = Math.min(depth, DIR_LIGHT.length - 1);
  const h = (hue + depth * 8) % 360;
  return `hsl(${h} ${DIR_SAT[d]}% ${DIR_LIGHT[d]}%)`;
}

export function dirStroke(hue: number, depth: number): string {
  const d = Math.min(depth, DIR_LIGHT.length - 1);
  const h = (hue + depth * 8) % 360;
  return `hsl(${h} ${DIR_SAT[d] + 8}% ${DIR_LIGHT[d] + 12}%)`;
}

export const EXT_GROUPS: { label: string; color: string; exts: string[] }[] = [
  { label: "视频", color: "#e05a6b", exts: ["mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "ts", "rmvb"] },
  { label: "图片", color: "#e2a33c", exts: ["jpg", "jpeg", "png", "gif", "bmp", "webp", "tif", "tiff", "psd", "raw", "heic", "svg"] },
  { label: "音频", color: "#b678e0", exts: ["mp3", "flac", "wav", "aac", "ogg", "m4a", "wma"] },
  { label: "压缩包", color: "#4fb477", exts: ["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "iso", "cab", "msi", "msix"] },
  { label: "程序", color: "#4a8fe0", exts: ["exe", "dll", "sys", "so", "dylib", "bin", "com", "app", "lib", "obj", "pdb"] },
  { label: "文档", color: "#dcd6a0", exts: ["pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "md", "csv", "log"] },
  { label: "数据/缓存", color: "#6b7280", exts: ["db", "dat", "cache", "tmp", "idx", "pack", "log1", "etl", "evtx", "pf"] },
];

const OTHER_COLOR = "#5a6472";

const EXT_MAP: Record<string, string> = (() => {
  const map: Record<string, string> = {};
  for (const g of EXT_GROUPS) for (const e of g.exts) map[e] = g.color;
  return map;
})();

export function extColor(ext?: string | null): string {
  if (!ext) return OTHER_COLOR;
  return EXT_MAP[ext] ?? OTHER_COLOR;
}

export function extGroupLabel(ext?: string | null): string {
  if (!ext) return "其它";
  for (const g of EXT_GROUPS) if (g.exts.includes(ext)) return g.label;
  return "其它";
}
