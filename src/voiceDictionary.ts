export interface Replacement {
  heard: string;
  meant: string;
}

export const BUILT_IN_REPLACEMENTS: Replacement[] = [
  { heard: "код-код", meant: "Claude Code" },
  { heard: "клод код", meant: "Claude Code" },
  { heard: "клауд код", meant: "Claude Code" },
  { heard: "клот код", meant: "Claude Code" },
  { heard: "cloud code", meant: "Claude Code" },
  { heard: "клод", meant: "Claude" },
  { heard: "клауд", meant: "Claude" },
  { heard: "гитхаб", meant: "GitHub" },
  { heard: "гит хаб", meant: "GitHub" },
  { heard: "опенай", meant: "OpenAI" },
  { heard: "опен эй ай", meant: "OpenAI" },
  { heard: "джемини", meant: "Gemini" },
  { heard: "тайпскрипт", meant: "TypeScript" },
  { heard: "джаваскрипт", meant: "JavaScript" },
  { heard: "пайтон", meant: "Python" },
  { heard: "парсор", meant: "парсер" },
  { heard: "парсёр", meant: "парсер" },
  { heard: "пасер", meant: "парсер" },
];

const SEPARATORS = ["=", "→", "->"];
const WORD_EDGE_BEFORE = "(?<![\\p{L}\\p{N}])";
const WORD_EDGE_AFTER = "(?![\\p{L}\\p{N}])";

function escapeForPattern(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function patternOf(heard: string): RegExp {
  const words = heard.trim().split(/[\s-]+/).map(escapeForPattern);
  return new RegExp(`${WORD_EDGE_BEFORE}${words.join("[\\s-]+")}${WORD_EDGE_AFTER}`, "giu");
}

function startsWithCapital(text: string): boolean {
  const first = text.charAt(0);
  return first !== first.toLowerCase();
}

function matchCase(found: string, meant: string): string {
  const meantIsLowerCase = meant === meant.toLowerCase();
  if (meantIsLowerCase && startsWithCapital(found)) {
    return meant.charAt(0).toUpperCase() + meant.slice(1);
  }
  return meant;
}

export function applyReplacements(text: string, replacements: Replacement[]): string {
  const longestFirst = [...replacements].filter((pair) => pair.heard.trim() !== "").sort((left, right) => right.heard.length - left.heard.length);
  let result = text;
  for (const pair of longestFirst) {
    result = result.replace(patternOf(pair.heard), (found) => matchCase(found, pair.meant));
  }
  return result;
}

export function parseUserReplacements(text: string): Replacement[] {
  const pairs: Replacement[] = [];
  for (const line of text.split("\n")) {
    const separator = SEPARATORS.find((candidate) => line.includes(candidate));
    if (!separator) {
      continue;
    }
    const at = line.indexOf(separator);
    const heard = line.slice(0, at).trim();
    const meant = line.slice(at + separator.length).trim();
    if (heard && meant) {
      pairs.push({ heard, meant });
    }
  }
  return pairs;
}

export function allReplacements(userText: string | undefined): Replacement[] {
  const userPairs = parseUserReplacements(userText ?? "");
  const userHeard = new Set(userPairs.map((pair) => pair.heard.toLowerCase()));
  const builtIn = BUILT_IN_REPLACEMENTS.filter((pair) => !userHeard.has(pair.heard.toLowerCase()));
  return [...userPairs, ...builtIn];
}
