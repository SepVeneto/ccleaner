import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface Node {
  name: string;
  path: string;
  size: number;
  isDir: boolean;
  files: number;
  ext?: string | null;
  modified: number;
  children: Node[];
  truncated: boolean;
}

export interface BigFile {
  path: string;
  name: string;
  size: number;
  modified: number;
}

export interface ScanResult {
  root: Node;
  totalSize: number;
  totalFiles: number;
  totalDirs: number;
  elapsedMs: number;
  topFiles: BigFile[];
  skipped: number;
}

export interface ScanOptions {
  maxDepth: number;
  minFileSize: number;
  minDirSize: number;
  topFiles: number;
}

export interface Volume {
  name: string;
  mount: string;
  total: number;
  free: number;
  fileSystem: string;
  removable: boolean;
  isSystem: boolean;
}

export type Risk = "safe" | "low" | "medium" | "high";

export interface CleanupTarget {
  id: string;
  name: string;
  category: string;
  description: string;
  safety: string;
  risk: Risk;
  admin: boolean;
  defaultSelected: boolean;
  kind: string;
  templates: string[];
  resolved: string[];
  exists: boolean;
  size: number | null;
  note: string;
}

export interface SizeEntry {
  id: string;
  size: number;
  files: number;
}

export interface TargetResult {
  id: string;
  name: string;
  freedBytes: number;
  deletedFiles: number;
  errors: string[];
}

export interface CleanResult {
  freedBytes: number;
  deletedFiles: number;
  errors: string[];
  perTarget: TargetResult[];
}

export interface DeleteResponse {
  freedBytes: number;
  deletedFiles: number;
  errors: string[];
}

export interface ScanProgress {
  files: number;
  bytes: number;
  current: string;
  elapsedMs: number;
}

export interface CleanProgress {
  done: number;
  total: number;
  current: string;
}

export const api = {
  platform: () => invoke<string>("platform"),
  listVolumes: () => invoke<Volume[]>("list_volumes"),
  defaultRoot: () => invoke<string>("default_root"),
  homeDir: () => invoke<string>("home_dir"),
  pathExists: (path: string) => invoke<boolean>("path_exists", { path }),
  pathSize: (path: string) => invoke<number>("path_size", { path }),
  scan: (path: string, options?: Partial<ScanOptions>) =>
    invoke<ScanResult>("scan", { path, options: options ?? null }),
  cancelScan: () => invoke<void>("cancel_scan"),
  cleanupTargets: () => invoke<CleanupTarget[]>("cleanup_targets"),
  estimateCleanup: (ids: string[]) => invoke<SizeEntry[]>("estimate_cleanup", { ids }),
  runCleanup: (ids: string[]) => invoke<CleanResult>("run_cleanup", { ids }),
  deletePaths: (paths: string[], permanent: boolean) =>
    invoke<DeleteResponse>("delete_paths", { req: { paths, permanent } }),
  reveal: (path: string) => invoke<void>("reveal", { path }),
};

export function onScanProgress(cb: (p: ScanProgress) => void): Promise<UnlistenFn> {
  return listen<ScanProgress>("scan:progress", (e) => cb(e.payload));
}

export function onCleanProgress(cb: (p: CleanProgress) => void): Promise<UnlistenFn> {
  return listen<CleanProgress>("cleanup:progress", (e) => cb(e.payload));
}
