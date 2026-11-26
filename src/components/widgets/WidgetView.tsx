import { Match, Switch, createMemo } from "solid-js";

import type { ToolCallPart } from "../../api";
import { TOOL_BOT_SIMULATOR } from "../../widgets/placement";
import { parseWidget, type WidgetSpec } from "../../widgets/types";
import { LiveBotWidget, MockBotWidget } from "./BotWidgets";
import { ChartWidget } from "./ChartWidget";
import { DrawingWidget } from "./DrawingWidget";
import { QuizWidget } from "./QuizWidget";
import { PageWidget, PhoneSiteWidget } from "./SandboxFrame";

function WidgetBody(props: { spec: WidgetSpec }) {
  const title = () => props.spec.title || "Виджет";
  return (
    <Switch>
      <Match when={props.spec.kind === "chart" && props.spec.chart}>{(chart) => <ChartWidget title={title()} spec={chart()} />}</Match>
      <Match when={props.spec.kind === "quiz" && props.spec.questions}>{(questions) => <QuizWidget title={title()} questions={questions()} />}</Match>
      <Match when={props.spec.kind === "drawing" && props.spec.svg}>
        {(svg) => <DrawingWidget title={title()} svg={svg()} paper={Boolean(props.spec.paper)} caption={props.spec.caption} />}
      </Match>
      <Match when={props.spec.kind === "page" && props.spec.html}>{(html) => <PageWidget title={title()} html={html()} />}</Match>
      <Match when={props.spec.kind === "phone" && props.spec.html}>{(html) => <PhoneSiteWidget title={title()} html={html()} />}</Match>
      <Match when={props.spec.kind === "bot" && props.spec.bot}>{(bot) => <MockBotWidget title={title()} spec={bot()} />}</Match>
    </Switch>
  );
}

export function InlineWidget(props: { part: ToolCallPart }) {
  const isSimulator = () => props.part.name === TOOL_BOT_SIMULATOR;
  const input = createMemo(() => props.part.input);
  const spec = createMemo(() => (isSimulator() ? null : parseWidget(input())));
  return (
    <Switch>
      <Match when={isSimulator()}>
        <LiveBotWidget />
      </Match>
      <Match when={spec()}>{(shown) => <WidgetBody spec={shown()} />}</Match>
    </Switch>
  );
}
