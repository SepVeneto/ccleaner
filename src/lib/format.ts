const UNITS = ["B", "KB", "MB", "GB", "TB", "PB"];

export function formatBytes(bytes: number, digits = 2): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  let value = bytes;
  let idx = 0;
  while (value >= 1024 && idx < UNITS.length - 1) {
    value /= 1024;
    idx += 1;
  }
  const fixed = idx === 0 ? 0 : digits;
  return `${value.toFixed(fixed)} ${UNITS[idx]}`;
}

export function formatCount(n: number): string {
  if (n >= 1_0000_0000) return `${(n / 1_0000_0000).toFixed(2)} 亿`;
  if (n >= 1_0000) return `${(n / 1_0000).toFixed(1)} 万`;
  return n.toLocaleString("zh-CN");
}

export function formatDate(secs: number): string {
  if (!secs) return "—";
  const d = new Date(secs * 1000);
  const pad = (x: number) => String(x).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

export function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  const s = ms / 1000;
  if (s < 60) return `${s.toFixed(1)} 秒`;
  const m = Math.floor(s / 60);
  return `${m} 分 ${Math.round(s % 60)} 秒`;
}

export function percent(part: number, total: number): string {
  if (!total) return "0%";
  const p = (part / total) * 100;
  if (p >= 10) return `${p.toFixed(0)}%`;
  if (p >= 1) return `${p.toFixed(1)}%`;
  return `${p.toFixed(2)}%`;
}

/** 紧凑显示路径（保留头尾，中间省略）。 */
export function shortenPath(path: string, max = 48): string {
  if (path.length <= max) return path;
  const head = Math.ceil((max - 3) / 3);
  const tail = max - 3 - head;
  return `${path.slice(0, head)}...${path.slice(-tail)}`;
}

export function baseName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts.length ? parts[parts.length - 1] : path;
}

export function splitPath(path: string): string[] {
  return path.split(/[\\/]/).filter(Boolean);
}
