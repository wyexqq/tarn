import type { MessagePart, ToolCallPart } from "../api";

export const TOOL_SHOW_WIDGET = "show_widget";
export const TOOL_BOT_SIMULATOR = "bot_simulator";

export type Piece = { kind: "text"; text: string } | { kind: "widget"; part: ToolCallPart };

function actionOf(part: ToolCallPart): string {
  try {
    const parsed = JSON.parse(part.input) as { action?: unknown };
    return typeof parsed.action === "string" ? parsed.action : "";
  } catch {
    return "";
  }
}

export function isShownInline(part: ToolCallPart): boolean {
  if (part.isError || part.status !== "done") {
    return false;
  }
  if (part.name === TOOL_SHOW_WIDGET) {
    return true;
  }
  return part.name === TOOL_BOT_SIMULATOR && actionOf(part) === "start";
}

export function isHiddenFromActions(part: ToolCallPart): boolean {
  return part.name === TOOL_SHOW_WIDGET && !part.isError;
}

export function inlineParts(parts: MessagePart[]): ToolCallPart[] {
  return parts.filter((part): part is ToolCallPart => part.type === "tool_call" && isShownInline(part));
}

export function splitByWidgets(content: string, widgets: ToolCallPart[]): Piece[] {
  const pieces: Piece[] = [];
  let cursor = 0;
  for (const part of widgets) {
    const wanted = part.textOffset ?? content.length;
    const offset = Math.min(Math.max(wanted, cursor), content.length);
    pieces.push({ kind: "text", text: content.slice(cursor, offset) });
    pieces.push({ kind: "widget", part });
    cursor = offset;
  }
  pieces.push({ kind: "text", text: content.slice(cursor) });
  return pieces;
}
