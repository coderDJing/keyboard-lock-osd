const { spawn, spawnSync } = require("node:child_process");

const isWindows = process.platform === "win32";
const viteArgs = process.argv.slice(2);
const command = isWindows ? "cmd.exe" : "pnpm";
const args = isWindows
  ? ["/d", "/s", "/c", ["pnpm", "exec", "vite", ...viteArgs].map(quoteWindowsArg).join(" ")]
  : ["exec", "vite", ...viteArgs];

const child = spawn(command, args, {
  env: process.env,
  stdio: "inherit",
  windowsHide: false,
});

let stopping = false;

function stop(signal) {
  if (stopping) {
    return;
  }

  stopping = true;
  if (child.pid && child.exitCode === null && child.signalCode === null) {
    if (isWindows) {
      spawnSync("taskkill", ["/pid", String(child.pid), "/t", "/f"], {
        stdio: "ignore",
      });
    } else {
      child.kill(signal);
    }
  }

  process.exit(0);
}

process.on("SIGINT", () => stop("SIGINT"));
process.on("SIGTERM", () => stop("SIGTERM"));

child.on("error", (error) => {
  if (!stopping) {
    console.error(error);
  }
  process.exit(stopping ? 0 : 1);
});

child.on("exit", (code, signal) => {
  if (stopping || signal === "SIGINT" || signal === "SIGTERM") {
    process.exit(0);
  }

  process.exit(code ?? 1);
});

function quoteWindowsArg(value) {
  if (!/[ \t"&|<>^]/.test(value)) {
    return value;
  }

  return `"${value.replace(/(["^])/g, "^$1")}"`;
}
