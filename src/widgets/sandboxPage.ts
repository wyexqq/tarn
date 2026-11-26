export type SandboxKind = "page" | "phone";

const BASE_CSS = `
html { background: var(--bg); color: var(--text); font: 14px/1.5 var(--font); -webkit-text-size-adjust: 100%; }
body { margin: 0; display: flow-root; }
*, *::before, *::after { box-sizing: border-box; }
button, input, select, textarea { font: inherit; color: inherit; }
`;

const PHONE_CSS = `
html, body { min-height: 100%; }
html { overflow-x: hidden; }
`;

const BRIDGE_SCRIPT = `
(function () {
  var root = document.documentElement;
  var startTheme = new URLSearchParams(location.search).get("theme");
  if (startTheme) {
    root.dataset.theme = startTheme;
  }
  window.addEventListener("message", function (event) {
    if (event.source === parent && event.data && typeof event.data.tarnTheme === "string") {
      root.dataset.theme = event.data.tarnTheme;
    }
  });
  window.tarn = {
    send: function (text) {
      parent.postMessage({ tarnSend: String(text) }, "*");
    }
  };
  var lastHeight = -1;
  function reportHeight() {
    if (!document.body) {
      return;
    }
    var height = Math.ceil(document.body.getBoundingClientRect().height);
    if (height !== lastHeight) {
      lastHeight = height;
      parent.postMessage({ tarnHeight: height }, "*");
    }
  }
  function watchHeight() {
    new ResizeObserver(reportHeight).observe(document.body);
    reportHeight();
  }
  if (document.body) {
    watchHeight();
  } else {
    document.addEventListener("DOMContentLoaded", watchHeight);
  }
  window.addEventListener("load", reportHeight);
})();
`;

const DOCTYPE_PATTERN = /^\s*<!doctype[^>]*>/i;
const HEAD_PATTERN = /<head(\s[^>]*)?>/i;

function headBlock(kind: SandboxKind, tokensCss: string): string {
  const viewport = kind === "phone" ? "width=390, initial-scale=1" : "width=device-width, initial-scale=1";
  const extraCss = kind === "phone" ? PHONE_CSS : "";
  return [
    '<meta charset="utf-8">',
    `<meta name="viewport" content="${viewport}">`,
    `<style>${tokensCss}${BASE_CSS}${extraCss}</style>`,
    `<script>${BRIDGE_SCRIPT}</script>`,
  ].join("");
}

export function wrapPage(html: string, kind: SandboxKind, tokensCss: string): string {
  const body = html.replace(DOCTYPE_PATTERN, "");
  const head = headBlock(kind, tokensCss);
  const headTag = body.match(HEAD_PATTERN);
  if (headTag && headTag.index !== undefined) {
    const insertAt = headTag.index + headTag[0].length;
    return `<!doctype html>${body.slice(0, insertAt)}${head}${body.slice(insertAt)}`;
  }
  return `<!doctype html><html><head>${head}</head><body>${body}</body></html>`;
}

export interface FrameMessage {
  height?: number;
  send?: string;
}

export function readFrameMessage(data: unknown): FrameMessage {
  if (typeof data !== "object" || data === null) {
    return {};
  }
  const record = data as Record<string, unknown>;
  const message: FrameMessage = {};
  if (typeof record.tarnHeight === "number" && Number.isFinite(record.tarnHeight)) {
    message.height = record.tarnHeight;
  }
  if (typeof record.tarnSend === "string") {
    message.send = record.tarnSend;
  }
  return message;
}
