import type { Theme } from "../theme";

const LIGHT_TOKENS = `
  color-scheme: light;
  --bg: #ffffff;
  --surface: #f3f1ed;
  --line: #e0dcd5;
  --text: #23211e;
  --text-dim: #66615a;
  --accent: #c15f3c;
  --accent-soft: #f6e6df;
  --blue: #3f6ea8;
  --blue-soft: #e3ebf5;
  --green: #4a8c55;
  --green-soft: #e4f0e5;
  --red: #b5403a;
  --red-soft: #f8e7e5;
  --yellow: #b0822a;
  --yellow-soft: #f5ecd8;
  --purple: #6b5bb5;
  --purple-soft: #ebe7f6;
  --paper-line: #dfe6ee;
  --font: "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif;
`;

const DARK_TOKENS = `
  color-scheme: dark;
  --bg: #2a2826;
  --surface: #201f1d;
  --line: #383532;
  --text: #ece8e2;
  --text-dim: #aaa49c;
  --accent: #d97757;
  --accent-soft: #3a2a23;
  --blue: #86a9d6;
  --blue-soft: #232c38;
  --green: #86b98c;
  --green-soft: #223024;
  --red: #e58a82;
  --red-soft: #3a2422;
  --yellow: #d9ae5f;
  --yellow-soft: #3a3122;
  --purple: #a99bdc;
  --purple-soft: #2c2838;
  --paper-line: #2f3640;
`;

export function tokensFor(theme: Theme): string {
  return theme === "dark" ? LIGHT_TOKENS + DARK_TOKENS : LIGHT_TOKENS;
}

export function themedTokensCss(): string {
  return `:root {${LIGHT_TOKENS}}\n:root[data-theme="dark"] {${DARK_TOKENS}}`;
}

const LIGHT_SERIES = ["#2a78d6", "#eb6834", "#1baf7a", "#eda100", "#e87ba4", "#008300", "#6250d6", "#e34948"];
const DARK_SERIES = ["#3987e5", "#d95926", "#199e70", "#c98500", "#d55181", "#008300", "#9085e9", "#e66767"];

export function seriesColor(index: number, theme: Theme): string {
  const palette = theme === "dark" ? DARK_SERIES : LIGHT_SERIES;
  return palette[index % palette.length];
}
