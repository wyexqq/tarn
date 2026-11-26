import type { JSX } from "solid-js";

function clockNow(): string {
  return new Date().toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" });
}

export function Phone(props: { children: JSX.Element; label: string }) {
  return (
    <div class="phone" role="group" aria-label={props.label}>
      <div class="phone-screen">
        <div class="phone-island" />
        <div class="phone-status" aria-hidden="true">
          <span>{clockNow()}</span>
          <span class="phone-signal">
            <i />
            <i />
            <i />
          </span>
        </div>
        <div class="phone-content">{props.children}</div>
      </div>
    </div>
  );
}
