import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  api,
  onScanProgress,
  type BigFile,
  type Node,
  type ScanOptions,
  type ScanProgress,
  type ScanResult,
  type Volume,
} from "./lib/api";
import { formatBytes, formatCount, formatDuration, percent } from "./lib/format";
import { EXT_GROUPS } from "./lib/palette";
import { Treemap } from "./components/Treemap";
import { CleanupPanel } from "./components/CleanupPanel";
import { BigFiles } from "./components/BigFiles";

interface View {
  root: Node;
  bigFiles: BigFile[];
}

type Tab = "overview" | "cleanup" | "files";

const DEFAULT_OPTIONS: ScanOptions = {
  maxDepth: 4,
  minFileSize: 1024 * 1024,
  minDirSize: 2 * 1024 * 1024,
  topFiles: 200,
};

export default function App() {
  const [platform, setPlatform] = useState("");
  const [volumes, setVolumes] = useState<Volume[]>([]);
  const [rootPath, setRootPath] = useState("");
  const [options, setOptions] = useState<ScanOptions>(DEFAULT_OPTIONS);
  const [views, setViews] = useState<View[]>([]);
  const [stats, setStats] = useState<ScanResult | null>(null);
  const [scanning, setScanning] = useState(false);
  const [progress, setProgress] = useState<ScanProgress | null>(null);
  const [selected, setSelected] = useState<Node | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("cleanup");

  const current = views.length ? views[views.length - 1] : null;
  const scanningRef = useRef(false);

  const showToast = useCallback((msg: string) => {
    setToast(msg);
    window.setTimeout(() => setToast(null), 3200);
  }, []);

  useEffect(() => {
    (async () => {
      try {
        const [p, vols, root] = await Promise.all([api.platform(), api.listVolumes(), api.defaultRoot()]);
        setPlatform(p);
        setVolumes(vols);
        setRootPath(root);
      } catch (e) {
        setError(String(e));
      }
    })();
  }, []);

  const runScan = useCallback(
    async (path: string, opts: ScanOptions) => {
      if (scanningRef.current) return;
      scanningRef.current = true;
      setScanning(true);
      setError(null);
      setProgress(null);
      setSelected(null);
      const un = await onScanProgress(setProgress);
      try {
        const res = await api.scan(path, opts);
        setViews([{ root: res.root, bigFiles: res.topFiles }]);
        setStats(res);
        setRootPath(path);
      } catch (e) {
        setError(String(e));
      } finally {
        un();
        setProgress(null);
        setScanning(false);
        scanningRef.current = false;
      }
    },
    [],
  );

  // 首次启动自动扫描默认盘（Windows 上即 C 盘）
  useEffect(() => {
    if (rootPath && views.length === 0 && !scanningRef.current) {
      runScan(rootPath, options);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rootPath]);

  async function cancel() {
    await api.cancelScan().catch(() => {});
    showToast("已请求取消扫描");
  }

  async function pickFolder() {
    try {
      const picked = await open({ directory: true, multiple: false, title: "选择要分析的目录" });
      if (typeof picked === "string") {
        setRootPath(picked);
        runScan(picked, options);
      }
    } catch (e) {
      setError(String(e));
    }
  }

  function zoom(node: Node) {
    if (!node.isDir || !current) return;
    if ((node.children?.length ?? 0) > 0 && !node.truncated) {
      setViews((v) => [...v, { root: node, bigFiles: current.bigFiles }]);
      setSelected(node);
      return;
    }
    // 深度不够，重新扫描该子目录
    (async () => {
      setScanning(true);
      const un = await onScanProgress(setProgress);
      try {
        const res = await api.scan(node.path, options);
        setViews((v) => [...v, { root: res.root, bigFiles: res.topFiles }]);
        setSelected(res.root);
      } catch (e) {
        setError(String(e));
      } finally {
        un();
        setProgress(null);
        setScanning(false);
      }
    })();
  }

  function goUp() {
    setViews((v) => (v.length > 1 ? v.slice(0, -1) : v));
  }

  function goTo(index: number) {
    setViews((v) => v.slice(0, index + 1));
  }

  async function deleteSelected(permanent: boolean) {
    if (!selected) return;
    const tip = permanent ? "永久删除（不可恢复）" : "删除到回收站";
    if (!confirm(`确定要${tip}吗？\n\n${selected.path}\n大小：${formatBytes(selected.size)}`)) return;
    try {
      const res = await api.deletePaths([selected.path], permanent);
      if (res.errors.length) {
        alert(`删除未完成：\n${res.errors.join("\n")}`);
      } else {
        showToast(`已删除，释放 ${formatBytes(res.freedBytes)}`);
      }
      setSelected(null);
      if (current) runScan(current.root.path, options);
    } catch (e) {
      setError(String(e));
    }
  }

  const systemVolume = useMemo(() => volumes.find((v) => v.isSystem) ?? volumes[0], [volumes]);
  const usedPct = systemVolume
    ? ((systemVolume.total - systemVolume.free) / Math.max(systemVolume.total, 1)) * 100
    : 0;

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <span className="logo">🧹</span>
          <div>
            <div className="brand-title">
              {platform === "windows" ? "C 盘清理助手" : "磁盘清理助手"}
            </div>
            <div className="brand-sub">目录占用可视化 · 安全清理</div>
          </div>
        </div>

        <div className="drives">
          {volumes.slice(0, 6).map((v) => (
            <button
              key={v.mount}
              className={`drive-chip ${rootPath.toLowerCase().startsWith(v.mount.toLowerCase()) ? "active" : ""}`}
              title={`${v.fileSystem} · 可用 ${formatBytes(v.free)}`}
              onClick={() => runScan(v.mount, options)}
            >
              {v.mount.replace(/\\$/, "")}
              <span className="drive-free">{formatBytes(v.free, 0)}</span>
            </button>
          ))}
        </div>

        <div className="pathbar">
          <input
            value={rootPath}
            onChange={(e) => setRootPath(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && rootPath) runScan(rootPath, options);
            }}
            placeholder="输入要分析的路径，例如 C:\\"
          />
          <button className="btn ghost" onClick={pickFolder}>
            浏览…
          </button>
          {scanning ? (
            <button className="btn danger" onClick={cancel}>
              取消
            </button>
          ) : (
            <button className="btn primary" onClick={() => rootPath && runScan(rootPath, options)}>
              开始扫描
            </button>
          )}
        </div>

        <div className="opts">
          <label title="返回的目录层级，越深越慢">
            层级
            <select
              value={options.maxDepth}
              onChange={(e) => setOptions({ ...options, maxDepth: Number(e.target.value) })}
            >
              {[2, 3, 4, 5, 6].map((d) => (
                <option key={d} value={d}>
                  {d}
                </option>
              ))}
            </select>
          </label>
          <label title="小于该大小的文件不单独显示">
            最小文件
            <select
              value={options.minFileSize}
              onChange={(e) => setOptions({ ...options, minFileSize: Number(e.target.value) })}
            >
              <option value={0}>全部</option>
              <option value={1024 * 100}>100 KB</option>
              <option value={1024 * 1024}>1 MB</option>
              <option value={10 * 1024 * 1024}>10 MB</option>
              <option value={100 * 1024 * 1024}>100 MB</option>
            </select>
          </label>
        </div>
      </header>

      {scanning && (
        <div className="scanbar">
          <div className="scanbar-fill" />
          <span>
            正在扫描… 已统计 {formatCount(progress?.files ?? 0)} 个文件 ·{" "}
            {formatBytes(progress?.bytes ?? 0)} · {formatDuration(progress?.elapsedMs ?? 0)}
          </span>
          <span className="scanbar-path">{progress?.current ?? rootPath}</span>
        </div>
      )}

      {error && (
        <div className="error-banner" onClick={() => setError(null)}>
          {error}（点击关闭）
        </div>
      )}
      {toast && <div className="toast">{toast}</div>}

      <main className="content">
        <section className="viz">
          {current ? (
            <>
              <div className="breadcrumb">
                <button className="btn tiny" onClick={goUp} disabled={views.length <= 1}>
                  ↑ 上一级
                </button>
                <div className="crumbs">
                  {views.map((v, i) => (
                    <span key={v.root.path + i} className="crumb-wrap">
                      {i > 0 && <span className="sep">›</span>}
                      <button
                        className={`crumb ${i === views.length - 1 ? "active" : ""}`}
                        onClick={() => goTo(i)}
                        title={v.root.path}
                      >
                        {v.root.name || v.root.path}
                      </button>
                    </span>
                  ))}
                </div>
                <div className="viz-summary">
                  <b>{formatBytes(current.root.size)}</b>
                  <span>
                    {formatCount(current.root.files)} 个文件 · {current.root.path}
                  </span>
                </div>
              </div>

              <div className="treemap-wrap">
                <Treemap
                  root={current.root}
                  selectedPath={selected?.path ?? null}
                  onSelect={setSelected}
                  onZoom={zoom}
                />
                {selected && (
                  <div className="detail-card">
                    <div className="dc-title">
                      {selected.isDir ? "📁" : "📄"} {selected.name}
                    </div>
                    <div className="dc-path" title={selected.path}>
                      {selected.path}
                    </div>
                    <div className="dc-rows">
                      <div>
                        <span>大小</span>
                        <b>{formatBytes(selected.size)}</b>
                      </div>
                      <div>
                        <span>占当前视图</span>
                        <b>{percent(selected.size, current.root.size)}</b>
                      </div>
                      <div>
                        <span>{selected.isDir ? "包含文件" : "类型"}</span>
                        <b>
                          {selected.isDir
                            ? `${formatCount(selected.files)} 个`
                            : (selected.ext ?? "未知").toUpperCase()}
                        </b>
                      </div>
                    </div>
                    <div className="dc-actions">
                      {selected.isDir &&
                        (selected.truncated || (selected.children?.length ?? 0) > 0) && (
                          <button className="btn primary" onClick={() => zoom(selected)}>
                            进入目录
                          </button>
                        )}
                      <button
                        className="btn ghost"
                        onClick={() => api.reveal(selected.path).catch(() => {})}
                      >
                        定位
                      </button>
                      <button className="btn ghost" onClick={() => deleteSelected(false)}>
                        回收站
                      </button>
                      <button className="btn danger" onClick={() => deleteSelected(true)}>
                        永久删除
                      </button>
                      <button className="btn tiny" onClick={() => setSelected(null)}>
                        ✕
                      </button>
                    </div>
                  </div>
                )}
              </div>

              <div className="legend">
                <span className="legend-dir">目录（颜色区分顶层目录）</span>
                {EXT_GROUPS.map((g) => (
                  <span key={g.label} className="legend-item">
                    <i style={{ background: g.color }} />
                    {g.label}
                  </span>
                ))}
                <span className="legend-hint">双击色块可逐层进入 · 虚线边框表示还可以继续深入</span>
              </div>
            </>
          ) : (
            <div className="viz-empty">
              <div className="ve-icon">🗂️</div>
              <h2>{scanning ? "正在扫描…" : "选择磁盘或目录开始分析"}</h2>
              <p>
                扫描后会用矩形树图展示每个目录/文件占用的空间，面积越大代表占用越多。
              </p>
              <button className="btn primary big" onClick={() => rootPath && runScan(rootPath, options)}>
                扫描 {rootPath || "默认磁盘"}
              </button>
            </div>
          )}
        </section>

        <aside className="side">
          <div className="tabs">
            <button className={tab === "cleanup" ? "tab active" : "tab"} onClick={() => setTab("cleanup")}>
              清理建议
            </button>
            <button className={tab === "files" ? "tab active" : "tab"} onClick={() => setTab("files")}>
              大文件
            </button>
            <button
              className={tab === "overview" ? "tab active" : "tab"}
              onClick={() => setTab("overview")}
            >
              概览
            </button>
          </div>

          <div className="side-body">
            {tab === "cleanup" && (
              <CleanupPanel
                platform={platform}
                onCleaned={() => current && runScan(current.root.path, options)}
              />
            )}
            {tab === "files" && (
              <BigFiles
                files={current?.bigFiles ?? []}
                onChanged={() => current && runScan(current.root.path, options)}
              />
            )}
            {tab === "overview" && (
              <div className="overview">
                {systemVolume && (
                  <div className="ov-card">
                    <div className="ov-title">
                      {systemVolume.mount} 空间使用
                    </div>
                    <div className="usage-bar">
                      <div className="usage-fill" style={{ width: `${Math.min(usedPct, 100)}%` }} />
                    </div>
                    <div className="ov-row">
                      <span>已用 {formatBytes(systemVolume.total - systemVolume.free)}</span>
                      <span>可用 {formatBytes(systemVolume.free)}</span>
                      <span>共 {formatBytes(systemVolume.total)}</span>
                    </div>
                  </div>
                )}
                {stats && (
                  <div className="ov-card">
                    <div className="ov-title">本次扫描</div>
                    <div className="ov-grid">
                      <div>
                        <span>总大小</span>
                        <b>{formatBytes(stats.totalSize)}</b>
                      </div>
                      <div>
                        <span>文件数</span>
                        <b>{formatCount(stats.totalFiles)}</b>
                      </div>
                      <div>
                        <span>目录数</span>
                        <b>{formatCount(stats.totalDirs)}</b>
                      </div>
                      <div>
                        <span>耗时</span>
                        <b>{formatDuration(stats.elapsedMs)}</b>
                      </div>
                      <div>
                        <span>跳过</span>
                        <b>{formatCount(stats.skipped)}</b>
                      </div>
                    </div>
                    {stats.skipped > 0 && (
                      <p className="ov-note">
                        跳过的多为符号链接、目录联结点，或没有权限访问的系统目录。
                      </p>
                    )}
                  </div>
                )}
                {!stats && <div className="empty">还没有扫描数据。</div>}
              </div>
            )}
          </div>
        </aside>
      </main>
    </div>
  );
}
