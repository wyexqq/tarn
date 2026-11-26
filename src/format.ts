const numberFormatter = new Intl.NumberFormat("ru-RU");

export function formatTokens(count: number): string {
  return numberFormatter.format(count);
}

export function formatThousands(count: number): string {
  if (count < 1000) {
    return numberFormatter.format(count);
  }
  const thousands = count / 1000;
  const shown = thousands < 10 ? Math.round(thousands * 10) / 10 : Math.round(thousands);
  return `${numberFormatter.format(shown)} тыс.`;
}

export function formatCost(cost: number): string {
  if (cost === 0) {
    return "$0";
  }
  if (cost < 0.01) {
    return `$${cost.toFixed(4).replace(".", ",")}`;
  }
  return `$${cost.toFixed(2).replace(".", ",")}`;
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} Б`;
  }
  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1).replace(".", ",")} КБ`;
  }
  if (bytes < 1024 * 1024 * 1024) {
    return `${(bytes / (1024 * 1024)).toFixed(1).replace(".", ",")} МБ`;
  }
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1).replace(".", ",")} ГБ`;
}

function startOfDay(moment: Date): number {
  return new Date(moment.getFullYear(), moment.getMonth(), moment.getDate()).getTime();
}

export function formatClock(timestamp: number): string {
  return new Date(timestamp).toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" });
}

export function formatMessageTime(timestamp: number): string {
  const moment = new Date(timestamp);
  const now = new Date();
  if (startOfDay(moment) === startOfDay(now)) {
    return formatClock(timestamp);
  }
  const sameYear = moment.getFullYear() === now.getFullYear();
  const day = moment.toLocaleDateString("ru-RU", sameYear ? { day: "numeric", month: "short" } : { day: "numeric", month: "short", year: "numeric" });
  return `${day}, ${formatClock(timestamp)}`;
}

export function formatFullMoment(timestamp: number): string {
  return new Date(timestamp).toLocaleString("ru-RU", { dateStyle: "long", timeStyle: "medium" });
}

export function formatRelativeDay(timestamp: number): string {
  const now = new Date();
  const dayDifference = Math.round((startOfDay(now) - startOfDay(new Date(timestamp))) / 86_400_000);
  if (dayDifference <= 0) {
    return formatClock(timestamp);
  }
  if (dayDifference === 1) {
    return "вчера";
  }
  const moment = new Date(timestamp);
  const sameYear = moment.getFullYear() === now.getFullYear();
  return moment.toLocaleDateString("ru-RU", sameYear ? { day: "numeric", month: "short" } : { day: "numeric", month: "short", year: "numeric" });
}

export function dayGroupLabel(timestamp: number): string {
  const dayDifference = Math.round((startOfDay(new Date()) - startOfDay(new Date(timestamp))) / 86_400_000);
  if (dayDifference <= 0) {
    return "Сегодня";
  }
  if (dayDifference === 1) {
    return "Вчера";
  }
  if (dayDifference < 7) {
    return "На этой неделе";
  }
  if (dayDifference < 30) {
    return "В этом месяце";
  }
  return "Раньше";
}

export function pluralRu(count: number, one: string, few: string, many: string): string {
  const lastTwo = count % 100;
  const last = count % 10;
  if (lastTwo >= 11 && lastTwo <= 14) {
    return many;
  }
  if (last === 1) {
    return one;
  }
  if (last >= 2 && last <= 4) {
    return few;
  }
  return many;
}

export function shortModelName(model: string | null | undefined): string {
  if (!model) {
    return "модель";
  }
  const lastSegment = model.includes("/") ? model.slice(model.lastIndexOf("/") + 1) : model;
  return lastSegment;
}

const KEY_PREFIXES = ["sk-", "AIza", "gsk_", "xai-"];

export function looksLikeApiKey(text: string): boolean {
  const trimmed = text.trim();
  if (trimmed.length < 20 || /\s/.test(trimmed)) {
    return false;
  }
  return KEY_PREFIXES.some((prefix) => trimmed.startsWith(prefix));
}

export function estimateTokens(text: string): number {
  return Math.ceil(text.length / 3.5);
}
