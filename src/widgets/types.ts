export type WidgetKind = "chart" | "quiz" | "drawing" | "page" | "phone" | "bot";
export type ChartType = "bar" | "line" | "parts";

export interface ChartSeries {
  name: string;
  values: number[];
}

export interface ChartSpec {
  type: ChartType;
  labels: string[];
  series: ChartSeries[];
  unit?: string;
  stacked?: boolean;
}

export interface QuizQuestion {
  question: string;
  options: string[];
  answer: number;
  explain?: string;
}

export interface BotButton {
  text: string;
  go?: string;
  edit?: boolean;
  url?: string;
}

export interface BotScreen {
  id: string;
  text: string;
  photo?: string;
  buttons?: BotButton[][];
  keyboard?: BotButton[][];
  next?: string;
}

export interface BotSpec {
  name: string;
  start: string;
  fallback?: string;
  screens: BotScreen[];
}

export interface WidgetSpec {
  kind: WidgetKind;
  title: string;
  chart?: ChartSpec;
  questions?: QuizQuestion[];
  svg?: string;
  paper?: boolean;
  caption?: string;
  html?: string;
  bot?: BotSpec;
}

const KINDS: WidgetKind[] = ["chart", "quiz", "drawing", "page", "phone", "bot"];

export function parseWidget(input: string): WidgetSpec | null {
  try {
    const parsed = JSON.parse(input) as Partial<WidgetSpec>;
    if (!parsed.kind || !KINDS.includes(parsed.kind)) {
      return null;
    }
    return { ...parsed, kind: parsed.kind, title: typeof parsed.title === "string" ? parsed.title : "" };
  } catch {
    return null;
  }
}
