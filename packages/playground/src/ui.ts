import { diagLog } from "./diagnostics";

export function $(id: string): HTMLElement {
  const el = document.getElementById(id);
  if (!el) throw new Error(`Element #${id} not found`);
  return el;
}

export function $btn(id: string): HTMLButtonElement {
  return $(id) as HTMLButtonElement;
}

export function setStatus(elementId: string, connected: boolean | null): void {
  const el = $(elementId);
  el.className = `status-dot ${
    connected === null ? "status-unknown" : connected ? "status-online" : "status-offline"
  }`;
}

let logCount = 0;

export function resetLogCount(): void {
  logCount = 0;
}

export function appendLog(
  msg: string,
  level: "info" | "warn" | "error" | "success" = "info",
  url?: string,
): void {
  diagLog(msg, level);
  const log = $("log");
  const line = document.createElement("div");
  const time = new Date().toLocaleTimeString("en-US", { hour12: false });
  const prefix =
    level === "error" ? "ERR" : level === "warn" ? "WRN" : level === "success" ? " OK" : "   ";
  line.className = `log-${level}`;

  if (url) {
    line.textContent = `${time} ${prefix}  `;
    const link = document.createElement("a");
    link.href = url;
    link.target = "_blank";
    link.rel = "noopener noreferrer";
    link.className = "text-cyan-500/70 hover:text-cyan-400 underline";
    link.textContent = msg;
    line.appendChild(link);
  } else {
    line.textContent = `${time} ${prefix}  ${msg}`;
  }

  log.appendChild(line);
  while (log.childElementCount > 500) {
    log.firstElementChild?.remove();
  }
  log.scrollTop = log.scrollHeight;
  logCount++;
  $("log-count").textContent = `${logCount} ${logCount === 1 ? "entry" : "entries"}`;
}

export function formatDuration(ms: number): string {
  return `${(Math.max(0, ms) / 1000).toFixed(1)}s`;
}

export function startClock(): void {
  const update = () => {
    const now = new Date();
    $("clock").textContent = now.toLocaleTimeString("en-US", { hour12: false });
  };
  update();
  setInterval(update, 1000);
}
