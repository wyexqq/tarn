import { Show, createResource } from "solid-js";

import { api, type Attachment } from "../api";
import { createModalFocus } from "../focus";
import { imageDataUrl } from "../images";
import { actions, state } from "../store";
import { CloseIcon } from "./icons";

async function loadImageSource(attachment: Attachment): Promise<string> {
  if (attachment.dataBase64) {
    return imageDataUrl(attachment);
  }
  if (attachment.imageId) {
    return api.readImage(attachment.imageId);
  }
  throw new Error("Изображение не сохранилось");
}

export function ImageView(props: { attachment: Attachment }) {
  const [source] = createResource(() => props.attachment, loadImageSource);
  return (
    <Show
      when={!source.error}
      fallback={<span class="image-missing">Изображение недоступно</span>}
    >
      <Show when={source()} fallback={<span class="image-loading" aria-label="Изображение загружается" />}>
        {(loadedSource) => (
          <button class="image-open" aria-label={`Открыть ${props.attachment.name}`} onClick={() => actions.setViewedImage(loadedSource())}>
            <img class="message-image" src={loadedSource()} alt={props.attachment.name} />
          </button>
        )}
      </Show>
    </Show>
  );
}

export function ImageViewer() {
  let containerElement: HTMLDivElement | undefined;
  createModalFocus(() => state.viewedImage() !== null, () => containerElement);
  const close = () => actions.setViewedImage(null);
  return (
    <Show when={state.viewedImage()}>
      {(source) => (
        <div
          ref={containerElement}
          class="image-viewer"
          role="dialog"
          aria-modal="true"
          aria-label="Просмотр изображения"
          onClick={close}
        >
          <img class="image-viewer-picture" src={source()} alt="" />
          <button class="image-viewer-close icon-button" aria-label="Закрыть" data-initial-focus="true" onClick={close}>
            <CloseIcon />
          </button>
        </div>
      )}
    </Show>
  );
}
