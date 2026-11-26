import { marked, Renderer, type Tokens } from "marked";
import hljs from "highlight.js/lib/core";
import bash from "highlight.js/lib/languages/bash";
import cpp from "highlight.js/lib/languages/cpp";
import csharp from "highlight.js/lib/languages/csharp";
import css from "highlight.js/lib/languages/css";
import go from "highlight.js/lib/languages/go";
import java from "highlight.js/lib/languages/java";
import javascript from "highlight.js/lib/languages/javascript";
import json from "highlight.js/lib/languages/json";
import powershell from "highlight.js/lib/languages/powershell";
import python from "highlight.js/lib/languages/python";
import rust from "highlight.js/lib/languages/rust";
import sql from "highlight.js/lib/languages/sql";
import typescript from "highlight.js/lib/languages/typescript";
import xml from "highlight.js/lib/languages/xml";
import yaml from "highlight.js/lib/languages/yaml";

const LANGUAGES = { bash, cpp, csharp, css, go, java, javascript, json, powershell, python, rust, sql, typescript, xml, yaml };

for (const [name, definition] of Object.entries(LANGUAGES)) {
  hljs.registerLanguage(name, definition);
}

hljs.registerAliases(["sh", "shell", "zsh", "console"], { languageName: "bash" });
hljs.registerAliases(["js", "jsx"], { languageName: "javascript" });
hljs.registerAliases(["ts", "tsx"], { languageName: "typescript" });
hljs.registerAliases(["py"], { languageName: "python" });
hljs.registerAliases(["rs"], { languageName: "rust" });
hljs.registerAliases(["html", "svg"], { languageName: "xml" });
hljs.registerAliases(["yml"], { languageName: "yaml" });
hljs.registerAliases(["ps1", "ps"], { languageName: "powershell" });
hljs.registerAliases(["c", "c++", "h"], { languageName: "cpp" });
hljs.registerAliases(["cs"], { languageName: "csharp" });

export function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function isSafeLink(href: string): boolean {
  return /^(https?:|mailto:)/i.test(href);
}

function highlightCode(code: string, language: string): string {
  if (language && hljs.getLanguage(language)) {
    return hljs.highlight(code, { language, ignoreIllegals: true }).value;
  }
  return escapeHtml(code);
}

const renderer = new Renderer();

renderer.html = (token: Tokens.HTML | Tokens.Tag) => escapeHtml(token.text);

renderer.code = (token: Tokens.Code) => {
  const language = (token.lang ?? "").trim().split(/\s+/)[0].toLowerCase();
  const label = escapeHtml(language || "текст");
  const body = highlightCode(token.text, language);
  return `<div class="code-block"><div class="code-head"><span>${label}</span><button class="code-copy" type="button" data-copy-code>Скопировать</button></div><pre><code>${body}</code></pre></div>`;
};

renderer.link = function (token: Tokens.Link) {
  const text = this.parser.parseInline(token.tokens);
  if (!isSafeLink(token.href)) {
    return text;
  }
  return `<a href="${escapeHtml(token.href)}" data-external-link title="${escapeHtml(token.href)}">${text}</a>`;
};

renderer.image = (token: Tokens.Image) => {
  const alt = escapeHtml(token.text || "изображение");
  if (!isSafeLink(token.href)) {
    return alt;
  }
  return `<a href="${escapeHtml(token.href)}" data-external-link>${alt}</a>`;
};

renderer.table = function (token: Tokens.Table) {
  const headerCells = token.header.map((cell) => `<th>${this.parser.parseInline(cell.tokens)}</th>`).join("");
  const bodyRows = token.rows
    .map((row) => `<tr>${row.map((cell) => `<td>${this.parser.parseInline(cell.tokens)}</td>`).join("")}</tr>`)
    .join("");
  return `<div class="table-scroll"><table><thead><tr>${headerCells}</tr></thead><tbody>${bodyRows}</tbody></table></div>`;
};

marked.use({ renderer, gfm: true, breaks: false, async: false });

export function renderMarkdown(source: string): string {
  return marked.parse(source, { async: false }) as string;
}
