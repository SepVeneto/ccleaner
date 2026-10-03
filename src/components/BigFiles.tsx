import { useState } from "react";
import { api, type BigFile } from "../lib/api";
import { formatBytes, formatDate, shortenPath } from "../lib/format";

interface Props {
  files: BigFile[];
  onChanged: () => void;
}

export function BigFiles({ files, onChanged }: Props) {
  const [busy, setBusy] = useState<string | null>(null);
  const [filter, setFilter] = useState("");

  const shown = files.filter((f) => f.path.toLowerCase().includes(filter.toLowerCase()));

  async function remove(path: string, permanent: boolean) {
    const tip = permanent ? "永久删除（不可恢复）" : "删除到回收站";
    if (!confirm(`确定要${tip}吗？\n\n${path}`)) return;
    setBusy(path);
    try {
      const res = await api.deletePaths([path], permanent);
      if (res.errors.length) alert(`删除未完成：\n${res.errors.join("\n")}`);
      onChanged();
    } finally {
      setBusy(null);
    }
  }

  if (files.length === 0) {
    return <div className="empty">扫描完成后，这里会列出当前范围内最大的文件。</div>;
  }

  return (
    <div className="bigfiles">
      <input
        className="filter"
        placeholder="按路径筛选…"
        value={filter}
        onChange={(e) => setFilter(e.target.value)}
      />
      <div className="bf-list">
        {shown.map((f) => (
          <div key={f.path} className="bf-item">
            <div className="bf-info">
              <div className="bf-name" title={f.path}>
                {f.name}
              </div>
              <div className="bf-path" title={f.path}>
                {shortenPath(f.path, 60)}
              </div>
              <div className="bf-meta">{formatDate(f.modified)}</div>
            </div>
            <div className="bf-size">{formatBytes(f.size)}</div>
            <div className="bf-buttons">
              <button
                className="icon-btn"
                title="在文件管理器中定位"
                onClick={() => api.reveal(f.path).catch(() => {})}
              >
                🔍
              </button>
              <button
                className="icon-btn"
                title="删除到回收站"
                disabled={busy === f.path}
                onClick={() => remove(f.path, false)}
              >
                🗑
              </button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
