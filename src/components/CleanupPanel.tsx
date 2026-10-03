import { useEffect, useMemo, useState } from "react";
import {
  api,
  onCleanProgress,
  type CleanProgress,
  type CleanResult,
  type CleanupTarget,
  type Risk,
} from "../lib/api";
import { formatBytes } from "../lib/format";

const RISK_LABEL: Record<Risk, string> = {
  safe: "安全",
  low: "基本安全",
  medium: "需确认",
  high: "高风险",
};

interface Props {
  platform: string;
  onCleaned: () => void;
}

export function CleanupPanel({ platform, onCleaned }: Props) {
  const [targets, setTargets] = useState<CleanupTarget[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [estimating, setEstimating] = useState(false);
  const [cleaning, setCleaning] = useState(false);
  const [progress, setProgress] = useState<CleanProgress | null>(null);
  const [result, setResult] = useState<CleanResult | null>(null);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    let alive = true;
    api
      .cleanupTargets()
      .then((list) => {
        if (!alive) return;
        setTargets(list);
        setSelected(new Set(list.filter((t) => t.defaultSelected && t.exists).map((t) => t.id)));
        setLoaded(true);
      })
      .catch((e) => console.error(e));
    return () => {
      alive = false;
    };
  }, []);

  const groups = useMemo(() => {
    const map = new Map<string, CleanupTarget[]>();
    for (const t of targets) {
      const arr = map.get(t.category) ?? [];
      arr.push(t);
      map.set(t.category, arr);
    }
    return Array.from(map.entries());
  }, [targets]);

  const selectedTargets = targets.filter((t) => selected.has(t.id));
  const selectedSize = selectedTargets.reduce((a, t) => a + (t.size ?? 0), 0);
  const estimatedCount = targets.filter((t) => t.size !== null).length;
  const totalCleanable = targets.reduce((a, t) => a + (t.size ?? 0), 0);

  async function scanSizes() {
    setEstimating(true);
    setResult(null);
    const un = await onCleanProgress(setProgress);
    try {
      const sizes = await api.estimateCleanup(targets.filter((t) => t.exists).map((t) => t.id));
      const map = new Map(sizes.map((s) => [s.id, s]));
      setTargets((prev) =>
        prev.map((t) => {
          const hit = map.get(t.id);
          return hit ? { ...t, size: hit.size } : t;
        }),
      );
    } catch (e) {
      console.error(e);
    } finally {
      un();
      setProgress(null);
      setEstimating(false);
    }
  }

  async function runClean() {
    const ids = Array.from(selected).filter((id) => targets.find((t) => t.id === id)?.exists);
    if (ids.length === 0) return;
    const names = ids
      .map((id) => targets.find((t) => t.id === id)?.name)
      .filter(Boolean)
      .join("、");
    const ok = confirm(
      `确定清理以下 ${ids.length} 项吗？\n\n${names}\n\n清理后这些缓存/临时文件将被删除（回收站内容与高风险项请谨慎）。`,
    );
    if (!ok) return;

    setCleaning(true);
    setResult(null);
    try {
      const res = await api.runCleanup(ids);
      setResult(res);
      const sizes = await api.estimateCleanup(ids);
      const map = new Map(sizes.map((s) => [s.id, s.size]));
      setTargets((prev) => prev.map((t) => (map.has(t.id) ? { ...t, size: map.get(t.id)! } : t)));
      onCleaned();
    } catch (e) {
      console.error(e);
      setResult({ freedBytes: 0, deletedFiles: 0, errors: [String(e)], perTarget: [] });
    } finally {
      setCleaning(false);
    }
  }

  function toggle(id: string) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  function toggleExpand(id: string) {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  return (
    <div className="cleanup">
      {platform !== "windows" && (
        <div className="banner">
          当前系统不是 Windows，下面显示的是跨平台回退项。在 Windows 上运行时会列出 C 盘的
          临时文件、Windows 更新缓存、系统日志、浏览器缓存等目标。
        </div>
      )}

      <div className="cleanup-head">
        <div className="cleanup-stats">
          <div>
            <span>已选 {selectedTargets.length} 项</span>
            <b>{selectedSize > 0 ? formatBytes(selectedSize) : "未测量"}</b>
          </div>
          <div>
            <span>可清理总量</span>
            <b>{estimatedCount > 0 ? formatBytes(totalCleanable) : "点击下方测量"}</b>
          </div>
        </div>
        <div className="cleanup-actions">
          <button className="btn ghost" onClick={scanSizes} disabled={estimating || cleaning}>
            {estimating ? "测量中…" : "测量占用大小"}
          </button>
          <button
            className="btn primary"
            onClick={runClean}
            disabled={cleaning || selectedTargets.length === 0}
          >
            {cleaning ? "清理中…" : `清理选中 (${selectedTargets.filter((t) => t.exists).length})`}
          </button>
        </div>
      </div>

      {progress && (
        <div className="cleanup-progress">
          正在统计 {progress.done}/{progress.total}：{progress.current}
        </div>
      )}

      {result && (
        <div className={result.errors.length > 0 ? "result warn" : "result ok"}>
          <div>
            已释放 <b>{formatBytes(result.freedBytes)}</b>，删除{" "}
            {result.deletedFiles.toLocaleString("zh-CN")} 个文件
            {result.errors.length > 0 ? `，${result.errors.length} 项未能删除` : ""}。
          </div>
          {result.errors.length > 0 && (
            <details>
              <summary>查看未删除的项（通常是被占用的文件，跳过即可）</summary>
              <ul>
                {result.errors.slice(0, 40).map((e, i) => (
                  <li key={i}>{e}</li>
                ))}
              </ul>
            </details>
          )}
        </div>
      )}

      {!loaded && <div className="empty">正在读取清理目标…</div>}

      <div className="cleanup-list">
        {groups.map(([category, items]) => {
          const catSize = items.reduce((a, t) => a + (t.size ?? 0), 0);
          const allSelected = items.every((t) => selected.has(t.id));
          return (
            <section key={category} className="cleanup-group">
              <header>
                <button
                  className="link"
                  onClick={() =>
                    setSelected((prev) => {
                      const next = new Set(prev);
                      items.forEach((t) => {
                        if (!t.exists) return;
                        if (allSelected) next.delete(t.id);
                        else next.add(t.id);
                      });
                      return next;
                    })
                  }
                >
                  {allSelected ? "☑" : "☐"} {category}
                </button>
                <span className="cat-size">{catSize > 0 ? formatBytes(catSize) : ""}</span>
              </header>
              {items.map((t) => {
                const isOpen = expanded.has(t.id);
                return (
                  <div
                    key={t.id}
                    className={`cleanup-item risk-${t.risk} ${t.exists ? "" : "missing"}`}
                  >
                    <div className="ci-main">
                      <input
                        type="checkbox"
                        checked={selected.has(t.id)}
                        disabled={!t.exists}
                        onChange={() => toggle(t.id)}
                      />
                      <div className="ci-body" onClick={() => toggleExpand(t.id)}>
                        <div className="ci-title">
                          <span className="ci-name">{t.name}</span>
                          <span className={`badge badge-${t.risk}`}>{RISK_LABEL[t.risk]}</span>
                          {t.admin && <span className="badge badge-admin">需管理员</span>}
                          {!t.exists && <span className="badge badge-missing">未发现</span>}
                          <span className="ci-size">
                            {t.size !== null ? formatBytes(t.size) : "—"}
                          </span>
                        </div>
                        <div className="ci-desc">{t.description}</div>
                        {isOpen && (
                          <div className="ci-detail">
                            <p>
                              <b>为什么可以清理：</b>
                              {t.safety}
                            </p>
                            {t.note && (
                              <p className="ci-note">
                                <b>注意：</b>
                                {t.note}
                              </p>
                            )}
                            <p className="ci-paths">
                              <b>对应位置：</b>
                              {t.resolved.length > 0
                                ? t.resolved.map((p, i) => (
                                    <span
                                      key={i}
                                      className="path-chip"
                                      title="点击在文件管理器中打开"
                                      onClick={(ev) => {
                                        ev.stopPropagation();
                                        api.reveal(p).catch(() => {});
                                      }}
                                    >
                                      {p}
                                    </span>
                                  ))
                                : t.templates.map((p, i) => (
                                    <span key={i} className="path-chip muted">
                                      {p}
                                    </span>
                                  ))}
                            </p>
                          </div>
                        )}
                      </div>
                    </div>
                  </div>
                );
              })}
            </section>
          );
        })}
      </div>
    </div>
  );
}
