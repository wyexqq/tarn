import { Show, createMemo } from "solid-js";

import { effectiveTheme } from "../../theme";
import { svgDataUrl, themedSvg } from "../../widgets/drawing";
import { savePicture } from "../../widgets/picture";
import { tokensFor } from "../../widgets/tokens";
import { DownloadIcon, DrawingKindIcon } from "../icons";
import { WidgetAction, WidgetCard } from "./WidgetCard";

export function DrawingWidget(props: { title: string; svg: string; paper: boolean; caption?: string }) {
  const shownUrl = createMemo(() => svgDataUrl(themedSvg(props.svg, tokensFor(effectiveTheme()), props.paper)));

  const saveAsPicture = () => {
    const printable = themedSvg(props.svg, tokensFor("light"), props.paper);
    void savePicture(props.title, svgDataUrl(printable), printable);
  };

  return (
    <WidgetCard
      icon={<DrawingKindIcon />}
      title={props.title}
      actions={
        <WidgetAction label="Сохранить картинкой" onClick={saveAsPicture}>
          <DownloadIcon />
        </WidgetAction>
      }
    >
      <figure class="drawing" classList={{ "is-paper": props.paper }}>
        <img src={shownUrl()} alt={props.caption || props.title} draggable={false} />
      </figure>
      <Show when={props.caption}>
        <p class="drawing-caption">{props.caption}</p>
      </Show>
    </WidgetCard>
  );
}
