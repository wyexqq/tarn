import { Show } from "solid-js";

import type { ChatSummary } from "../api";

const LABELS = { claude_code: "CC", codex: "CX" } as const;
const TITLES = { claude_code: "Импортирован из Claude Code", codex: "Импортирован из Codex" } as const;

export function SourceBadge(props: { source: ChatSummary["source"] }) {
  return (
    <Show when={props.source !== "local"}>
      <span class={`source-badge source-${props.source}`} title={TITLES[props.source as keyof typeof TITLES]}>
        {LABELS[props.source as keyof typeof LABELS]}
      </span>
    </Show>
  );
}
