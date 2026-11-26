import type { JSX } from "solid-js";

export function WidgetCard(props: { icon: JSX.Element; title: string; actions?: JSX.Element; children: JSX.Element; flush?: boolean }) {
  return (
    <article class="widget">
      <header class="widget-head">
        <span class="widget-kind">{props.icon}</span>
        <span class="widget-title" title={props.title}>{props.title}</span>
        {props.actions}
      </header>
      <div class="widget-body" classList={{ "is-flush": props.flush }}>{props.children}</div>
    </article>
  );
}

export function WidgetAction(props: { label: string; pressed?: boolean; onClick: () => void; children: JSX.Element }) {
  return (
    <button class="widget-action" title={props.label} aria-label={props.label} aria-pressed={props.pressed} onClick={() => props.onClick()}>
      {props.children}
    </button>
  );
}
