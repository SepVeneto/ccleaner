#!/usr/bin/env node
/**
 * 构建 Windows 安装包的统一入口。
 *
 *   npm run build:win              # 自动判断：Windows 本机构建 / Linux·macOS 交叉编译
 *   npm run build:win -- --arch arm64
 *   npm run build:win -- --bundles nsis
 *   npm run build:win:check        # 只检查环境，不构建
 *
 * 说明：
 * - 在 Windows 上可以同时产出 NSIS(.exe) 与 MSI(.msi)。
 * - 在 Linux / macOS 上通过 cargo-xwin 交叉编译，只能产出 NSIS(.exe)，
 *   因为 MSI 依赖只能在 Windows 上运行的 WiX 工具链。
 */
import { spawnSync } from "node:child_process";
import { existsSync, readdirSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const argv = process.argv.slice(2);
const checkOnly = argv.includes("--check") || argv.includes("-c");
const dryRun = argv.includes("--dry-run");
const skipPreflight = argv.includes("--skip-preflight");

function argValue(name) {
  const i = argv.indexOf(name);
  return i >= 0 ? argv[i + 1] : undefined;
}

const arch = (argValue("--arch") ?? "x64").toLowerCase();
// --host 仅用于调试：在 Linux 上模拟 Windows 分支，验证命令拼装
const forcedHost = argValue("--host");
const isWindowsHost =
  forcedHost === "windows"
    ? true
    : forcedHost === "linux" || forcedHost === "macos"
      ? false
      : process.platform === "win32";
const hostTarget = process.arch === "arm64" ? "aarch64-pc-windows-msvc" : "x86_64-pc-windows-msvc";
const target = arch === "arm64" ? "aarch64-pc-windows-msvc" : "x86_64-pc-windows-msvc";
const cross = !isWindowsHost;

// MSI 只能在 Windows 上打包（WiX 仅支持 Windows）
const bundles = argValue("--bundles") ?? (isWindowsHost ? "nsis,msi" : "nsis");

const C = {
  dim: (s) => `\x1b[2m${s}\x1b[0m`,
  bold: (s) => `\x1b[1m${s}\x1b[0m`,
  green: (s) => `\x1b[32m${s}\x1b[0m`,
  red: (s) => `\x1b[31m${s}\x1b[0m`,
  yellow: (s) => `\x1b[33m${s}\x1b[0m`,
  cyan: (s) => `\x1b[36m${s}\x1b[0m`,
};

function log(msg = "") {
  console.log(msg);
}

/**
 * 用 Node 直接执行 Tauri CLI 的入口脚本，避免依赖 .bin/tauri.cmd 或 shell 拼接参数。
 */
const TAURI_CLI = join(root, "node_modules", "@tauri-apps", "cli", "tauri.js");

function hasTauriCli() {
  return existsSync(TAURI_CLI);
}

function rel(p) {
  return p.startsWith(root) ? p.slice(root.length + 1) : p;
}

/**
 * 在 PATH 与常见安装目录里查找可执行文件，返回绝对路径或 null。
 * 比只跑一遍 `cmd --version` 可靠：不依赖退出码，也能找到没进 PATH 的安装。
 */
function findExec(name, extraDirs = []) {
  const exts = process.platform === "win32" ? ["", ".exe", ".cmd", ".bat", ".com"] : [""];
  const local = [];
  for (const dir of extraDirs) for (const e of exts) local.push(join(dir, name + e));

  const finder = process.platform === "win32" ? "where" : "which";
  const res = spawnSync(finder, [name], { encoding: "utf8" });
  if (res.status === 0) {
    for (const line of res.stdout.split(/\r?\n/).map((s) => s.trim()).filter(Boolean)) {
      if (existsSync(line)) return line;
    }
  }
  return local.find((p) => existsSync(p)) ?? null;
}

/** 工具可能被装在但没加入 PATH 的目录（含 WSL 下可见的 Windows 路径）。 */
function toolDirs() {
  const dirs = [];
  if (process.platform === "win32") {
    const pf = process.env["ProgramFiles"] ?? "C:\\Program Files";
    const pf86 = process.env["ProgramFiles(x86)"] ?? "C:\\Program Files (x86)";
    const local = process.env["LOCALAPPDATA"] ?? "";
    dirs.push(join(pf86, "NSIS"), join(pf, "NSIS"));
    if (local) dirs.push(join(local, "tauri", "NSIS"));
  } else {
    dirs.push("/usr/bin", "/usr/local/bin", "/snap/bin", "/opt/local/bin", "/opt/homebrew/bin");
    // WSL：Windows 侧装的 NSIS 在这里也能看到
    dirs.push("/mnt/c/Program Files (x86)/NSIS", "/mnt/c/Program Files/NSIS");
  }
  if (process.env.NSIS_PATH) dirs.unshift(process.env.NSIS_PATH);
  return dirs;
}

function findNsis() {
  return findExec("makensis", toolDirs());
}

/** cargo-xwin 既可能是独立可执行文件，也可能是 cargo 子命令。 */
function findCargoXwin() {
  const direct = findExec("cargo-xwin");
  if (direct) return rel(direct);
  const probe = spawnSync("cargo", ["xwin", "--version"], { stdio: "ignore" });
  return probe.status === 0 ? "cargo xwin" : null;
}

/** 找出可用的 llvm-rc（cargo-xwin 链接 Windows 资源需要）。 */
function findLlvmRc() {
  const dirs = ["/opt/homebrew/opt/llvm/bin", "/usr/local/opt/llvm/bin", "/usr/lib/llvm-18/bin"];
  const names = ["llvm-rc", ...["20", "19", "18", "17", "16", "15"].map((v) => `llvm-rc-${v}`)];
  for (const n of names) {
    const hit = findExec(n, dirs);
    if (hit) return hit;
  }
  return null;
}

function installedRustTargets() {
  const out = spawnSync("rustup", ["target", "list", "--installed"], { encoding: "utf8" });
  return out.status === 0 ? out.stdout.split("\n").map((s) => s.trim()).filter(Boolean) : [];
}

function preflight() {
  log(C.bold(cross ? "检查 Linux/macOS → Windows 交叉编译环境…" : "检查 Windows 本机构建环境…"));
  log(C.dim(`  宿主系统: ${process.platform}   目标平台: ${target}   安装包类型: ${bundles}`));
  log("");

  const rows = [];
  const hints = [];
  let blocked = false;

  const add = (label, ok, detail, opts = {}) => {
    rows.push([label, ok, detail, opts]);
    if (!ok && opts.required) {
      blocked = true;
      if (opts.hint) hints.push(opts.hint);
    }
  };

  const cliOk = hasTauriCli();
  add("Tauri CLI", cliOk, cliOk ? rel(TAURI_CLI) : "未找到 @tauri-apps/cli", {
    required: true,
    hint: "npm install",
  });

  const cargo = findExec("cargo");
  add("cargo", Boolean(cargo), cargo ? rel(cargo) : "未找到", {
    required: true,
    hint: "先安装 Rust：https://rustup.rs",
  });

  if (cross) {
    const xwin = findCargoXwin();
    add("cargo-xwin", Boolean(xwin), xwin ?? "未找到", {
      required: true,
      hint: "cargo install cargo-xwin",
    });

    const rc = findLlvmRc();
    add("LLVM (llvm-rc)", Boolean(rc), rc ?? "未找到", {
      required: true,
      hint: 'Ubuntu/Debian: sudo apt install llvm clang lld\nmacOS: brew install llvm && export PATH="$(brew --prefix llvm)/bin:$PATH"',
    });

    const nsis = findNsis();
    add(
      "NSIS (makensis)",
      Boolean(nsis),
      nsis ?? "未找到 —— 打包阶段会失败，需要安装 NSIS",
      {
        // 标记为非必需：避免因为找不到就完全拦住构建，
        // 打包阶段若真的用到 makensis，Tauri 会给出明确报错，再按提示安装即可
        required: false,
        hint: "Ubuntu/Debian: sudo apt install nsis\nmacOS: brew install nsis\nWindows: https://nsis.sourceforge.io/Download",
      },
    );

    const targets = installedRustTargets();
    const hasTarget = targets.includes(target);
    add(`Rust 目标 ${target}`, hasTarget, hasTarget ? "已安装" : "未安装", {
      required: true,
      hint: `rustup target add ${target}`,
    });
  }

  for (const [label, ok, detail, opts] of rows) {
    const mark = ok ? C.green("✓") : opts.required ? C.red("✗") : C.yellow("!");
    log(`  ${mark} ${label.padEnd(22)} ${C.dim(detail)}`);
  }
  log("");

  if (blocked) {
    log(C.red("缺少必需的环境，已停止构建。"));
    log("");
    log(C.bold("请执行："));
    for (const h of hints) for (const line of h.split("\n")) log(C.cyan(`  ${line}`));
    log("");
    return false;
  }

  if (rows.some(([, ok]) => !ok)) {
    log(C.yellow("有可选组件未找到，继续构建；若构建失败请参考上面的提示。"));
  } else {
    log(C.green("环境检查通过。"));
  }
  return true;
}

function run(label, command, args) {
  log("");
  log(C.bold(`▶ ${label}`));
  log(C.dim(`  ${command} ${args.join(" ")}`));
  if (dryRun) {
    log(C.yellow("  (--dry-run：未实际执行)"));
    return;
  }
  const res = spawnSync(command, args, { stdio: "inherit", cwd: root });
  if (res.status !== 0) {
    log(C.red(`\n构建失败（退出码 ${res.status ?? "unknown"}）。`));
    process.exit(res.status ?? 1);
  }
}

function reportArtifacts() {
  const bundleRoot = join(root, "src-tauri", "target", target, "release", "bundle");
  if (!existsSync(bundleRoot)) return;
  log("");
  log(C.bold("构建产物："));
  const walk = (dir, depth = 0) => {
    for (const name of readdirSync(dir)) {
      const full = join(dir, name);
      const st = statSync(full);
      if (st.isDirectory()) {
        walk(full, depth + 1);
      } else if (/\.(exe|msi)$/i.test(name)) {
        const mb = (st.size / 1024 / 1024).toFixed(1);
        log(`  ${C.green("✔")} ${full}  ${C.dim(`(${mb} MB)`)}`);
      }
    }
  };
  walk(bundleRoot);
}

// ---------------------------------------------------------------------------

if (argv.includes("--help") || argv.includes("-h")) {
  log("用法: npm run build:win [-- 选项]");
  log("");
  log("选项:");
  log("  --arch <x64|arm64>      目标架构（默认 x64）");
  log("  --bundles <types>       安装包类型，默认 Windows 上 nsis,msi / 其它平台 nsis");
  log("  --check                 只检查环境，不执行构建");
  log("  --dry-run               只打印将要执行的命令");
  log("  --skip-preflight        跳过环境检查（自备工具链时使用）");
  log("  --host <windows|linux>  调试用：强制按某个宿主平台处理");
  log("  -h, --help              显示帮助");
  process.exit(0);
}

if (!skipPreflight && !preflight()) {
  process.exit(checkOnly ? 0 : 1);
}

if (checkOnly) {
  process.exit(0);
}

const buildArgs = [TAURI_CLI, "build", "--bundles", bundles];

// 交叉编译时必须让 cargo 走 cargo-xwin 作为 runner
if (cross) buildArgs.push("--runner", "cargo-xwin");
// 与本机架构一致时可以省略 --target
if (!isWindowsHost || target !== hostTarget) buildArgs.push("--target", target);

run(`构建 Windows 安装包（${target}）`, process.execPath, buildArgs);
reportArtifacts();

log("");
log(C.green("完成。"));
