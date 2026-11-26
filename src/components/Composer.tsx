import { For, Show, createEffect, createMemo, createSignal, on, onCleanup, onMount, untrack } from "solid-js";
import { open } from "@tauri-apps/plugin-dialog";

import { api, type Attachment } from "../api";
import { estimateTokens, formatTokens, looksLikeApiKey } from "../format";
import { attachmentFromFile, fitAttachmentForModels, imageDataUrl } from "../images";
import { pendingDroppedFiles, takeDroppedFiles } from "./DropZone";
import {
  actions,
  chooseAccessFolder,
  chooseAccessMode,
  chooseEffort,
  currentAccess,
  isStreaming,
  refreshProviders,
  selectedEffort,
  selectedEffortLevels,
  selectModel,
  sendMessage,
  showToast,
  state,
  stopStreaming,
} from "../store";
import { startDictation, type Dictation } from "../dictation";
import { DictationText, mapCaret } from "../dictationText";
import { describeMicrophoneError } from "../voice";
import { playMicrophoneOff, playMicrophoneOn } from "../voiceSounds";
import { VoiceLevelColumns, VoiceWave } from "./VoiceWave";
import { ComposerMenu, type ComposerMenuOption } from "./ComposerMenu";
import { ArrowUpIcon, CloseIcon, FileIcon, FolderIcon, MicrophoneIcon, PlusIcon, StopIcon } from "./icons";
import { ContextMeter } from "./ContextMeter";
import { ModelPicker } from "./ModelPicker";

const MAX_TEXTAREA_HEIGHT = 240;
const KEY_SUGGESTION_NUDGE = [
  { transform: "translateX(0)" },
  { transform: "translateX(-5px)" },
  { transform: "translateX(4px)" },
  { transform: "translateX(0)" },
];

const ACCESS_OPTIONS: ComposerMenuOption[] = [
  { value: "chat", title: "Только чат", hint: "Модель не видит компьютер" },
  { value: "plan", title: "План", hint: "Читает файлы в папке, ничего не меняет" },
  { value: "ask", title: "Спрашивать", hint: "Читает сама, перед любым изменением спрашивает" },
  { value: "acceptEdits", title: "Правки без вопросов", hint: "Файлы в папке меняет сама, команды спрашивает" },
  { value: "auto", title: "Авто", hint: "Обычные команды запускает сама, рискованные спрашивает" },
  { value: "full", title: "Полный доступ", hint: "Всё без вопросов и за пределами папки", isRisky: true },
];

const DEFAULT_EFFORT_OPTION: ComposerMenuOption = { value: "", title: "Как у модели" };

const EFFORT_TITLES: Record<string, ComposerMenuOption> = {
  low: { value: "low", title: "Низкий" },
  medium: { value: "medium", title: "Средний" },
  high: { value: "high", title: "Высокий" },
  xhigh: { value: "xhigh", title: "Очень высокий" },
  max: { value: "max", title: "Максимум" },
};


function shortFolderName(folder: string): string {
  const pieces = folder.split(/[\\/]/).filter(Boolean);
  return pieces.slice(-2).join("\\") || folder;
}

export function Composer(props: { placement: "start" | "chat" }) {
  let textareaElement: HTMLTextAreaElement | undefined;
  const [text, setText] = createSignal("");
  let keySuggestionElement: HTMLDivElement | undefined;
  const [attachments, setAttachments] = createSignal<Attachment[]>([]);
  const [suggestedKey, setSuggestedKey] = createSignal<string | null>(null);
  const [isSavingKey, setIsSavingKey] = createSignal(false);
  const [voiceStage, setVoiceStage] = createSignal<"idle" | "recording" | "recognizing">("idle");
  const voiceLevels = new VoiceLevelColumns();
  let dictation: Dictation | null = null;
  let dictationText: DictationText | null = null;

  const effortOptions = createMemo(() => [DEFAULT_EFFORT_OPTION, ...selectedEffortLevels().map((level) => EFFORT_TITLES[level] ?? { value: level, title: level })]);
  const effortButtonText = createMemo(() => (selectedEffortLevels().includes(selectedEffort()) ? EFFORT_TITLES[selectedEffort()]?.title ?? selectedEffort() : DEFAULT_EFFORT_OPTION.title));
  const accessButtonText = createMemo(() => ACCESS_OPTIONS.find((option) => option.value === currentAccess().mode)?.title ?? "Только чат");

  const canSend = createMemo(() => (text().trim().length > 0 || attachments().length > 0) && !isStreaming());

  const inputEstimate = createMemo(() => {
    const length = text().trim().length;
    return length === 0 ? null : estimateTokens(text());
  });

  const resizeTextarea = () => {
    if (!textareaElement) {
      return;
    }
    textareaElement.style.height = "auto";
    textareaElement.style.height = `${Math.min(textareaElement.scrollHeight, MAX_TEXTAREA_HEIGHT)}px`;
  };

  const handleInput = (value: string) => {
    setText(value);
    setSuggestedKey(looksLikeApiKey(value) ? value.trim() : null);
    resizeTextarea();
  };

  const addAttachmentsFromPaths = async (paths: string[]) => {
    for (const path of paths) {
      try {
        const attachment = await fitAttachmentForModels(await api.readAttachment(path));
        setAttachments((current) => [...current, attachment]);
      } catch (error) {
        showToast(String(error), "error");
      }
    }
  };

  const addAttachmentsFromFiles = async (files: File[]) => {
    for (const file of files) {
      try {
        const attachment = await attachmentFromFile(file);
        setAttachments((current) => [...current, attachment]);
      } catch (error) {
        showToast(error instanceof Error ? error.message : String(error), "error");
      }
    }
    textareaElement?.focus();
  };

  const handlePaste = (event: ClipboardEvent) => {
    const anotherLayerIsOpen = state.settingsTab() !== null || state.paletteOpen() || state.viewedImage() !== null;
    const pastesIntoAnotherField = (event.target as HTMLElement).closest("input, textarea") !== null && event.target !== textareaElement;
    if (anotherLayerIsOpen || pastesIntoAnotherField || !event.clipboardData) {
      return;
    }
    const files = [...event.clipboardData.files];
    const images = files.filter((file) => file.type.startsWith("image/"));
    const hasPlainText = event.clipboardData.getData("text/plain").length > 0;
    const isCopiedTable = event.clipboardData.getData("text/html").includes("<table");
    const filesToAttach = isCopiedTable ? [] : hasPlainText ? images : files;
    if (filesToAttach.length === 0) {
      return;
    }
    event.preventDefault();
    void addAttachmentsFromFiles(filesToAttach);
  };

  const pickFiles = async () => {
    const selection = await open({ multiple: true, title: "Прикрепить файлы" });
    if (selection) {
      await addAttachmentsFromPaths(Array.isArray(selection) ? selection : [selection]);
    }
    textareaElement?.focus();
  };

  const removeAttachment = (target: Attachment) => {
    setAttachments((current) => current.filter((item) => item !== target));
  };

  const showDictationText = (spokenText: DictationText) => {
    if (!textareaElement) {
      return;
    }
    const previous = textareaElement.value;
    const next = spokenText.value();
    const caretFollowsEnd = textareaElement.selectionStart === previous.length;
    const selectionStart = mapCaret(previous, next, textareaElement.selectionStart);
    const selectionEnd = mapCaret(previous, next, textareaElement.selectionEnd);
    textareaElement.value = next;
    textareaElement.setSelectionRange(selectionStart, selectionEnd);
    handleInput(next);
    if (caretFollowsEnd) {
      textareaElement.scrollTop = textareaElement.scrollHeight;
    }
  };

  const handleTextareaInput = (value: string) => {
    if (dictationText) {
      dictation?.noteEditedSegments(dictationText.applyUserEdit(value));
      dictation?.noteUserTyping();
    }
    handleInput(value);
  };

  const stopDictationWithoutRefining = () => {
    if (dictation) {
      dictation.cancel();
      playMicrophoneOff();
    }
    if (voiceStage() !== "idle") {
      void api.releaseSpeech();
    }
    dictation = null;
    dictationText = null;
    setVoiceStage("idle");
  };

  const finishRecording = async () => {
    const active = dictation;
    const spokenText = dictationText;
    if (!active || !spokenText) {
      return;
    }
    dictation = null;
    setVoiceStage("recognizing");
    const finishing = active.finish();
    playMicrophoneOff();
    try {
      const refinedGroups = await finishing;
      if (dictationText === spokenText) {
        refinedGroups.forEach((group) => spokenText.applyRefinedGroup(group));
        showDictationText(spokenText);
        if (spokenText.value().trim().length === 0) {
          showToast("Не расслышал речь. Проверь микрофон", "error");
        }
      }
    } catch (error) {
      if (dictationText === spokenText) {
        showToast(String(error), "error");
      }
    } finally {
      void api.releaseSpeech();
      if (dictationText === spokenText) {
        dictationText = null;
        setVoiceStage("idle");
        textareaElement?.focus();
      }
    }
  };

  const beginRecording = async () => {
    const status = await api.voiceStatus();
    if (!status.isReady) {
      actions.setSettingsTab("voice");
      return;
    }
    const spokenText = new DictationText(text());
    voiceLevels.clear();
    playMicrophoneOn();
    void api.prepareSpeech().catch((error) => showToast(String(error), "error"));
    try {
      dictation = await startDictation({
        onLevel: (level) => voiceLevels.add(level),
        onSegmentText: (segmentId, spoken) => {
          if (dictationText === spokenText) {
            spokenText.setSegmentText(segmentId, spoken);
            showDictationText(spokenText);
          }
        },
        onShouldStop: () => void finishRecording(),
      });
      dictationText = spokenText;
      setVoiceStage("recording");
      textareaElement?.focus();
      textareaElement?.setSelectionRange(text().length, text().length);
    } catch (error) {
      void api.releaseSpeech();
      showToast(describeMicrophoneError(error), "error");
    }
  };

  const toggleRecording = () => {
    if (voiceStage() === "idle") {
      void beginRecording();
    } else if (voiceStage() === "recording") {
      void finishRecording();
    }
  };

  const saveSuggestedKey = async () => {
    const key = suggestedKey();
    if (!key) {
      return;
    }
    setIsSavingKey(true);
    try {
      const result = await api.addKey(key);
      await refreshProviders();
      setText("");
      setSuggestedKey(null);
      resizeTextarea();
      showToast(`${result.providerName} подключён: ${result.modelCount} ${result.modelCount === 1 ? "модель" : "моделей"}`);
    } catch (error) {
      showToast(String(error), "error");
    } finally {
      setIsSavingKey(false);
    }
  };

  const submit = async () => {
    if (!canSend()) {
      return;
    }
    if (suggestedKey()) {
      keySuggestionElement?.animate(KEY_SUGGESTION_NUDGE, { duration: 320, easing: "ease-out" });
      return;
    }
    const messageText = text().trim();
    const messageAttachments = attachments();
    stopDictationWithoutRefining();
    setText("");
    setAttachments([]);
    resizeTextarea();
    const sent = await sendMessage(messageText, messageAttachments);
    if (!sent) {
      setText(messageText);
      setAttachments(messageAttachments);
      resizeTextarea();
    }
    textareaElement?.focus();
  };

  const handleKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Enter" && !event.shiftKey && !event.isComposing) {
      event.preventDefault();
      void submit();
    }
  };

  createEffect(on(state.composerFocusTick, () => textareaElement?.focus(), { defer: true }));

  createEffect(() => {
    const prefill = state.composerPrefill();
    if (!prefill) {
      return;
    }
    actions.setComposerPrefill(null);
    untrack(() => {
      if (voiceStage() !== "idle") {
        return;
      }
      if (!prefill.append && text().trim() !== prefill.onlyIfTextIs.trim()) {
        return;
      }
      const existing = text().trimEnd();
      const nextText = prefill.append && existing ? `${existing}
${prefill.text}` : prefill.text;
      if (textareaElement) {
        textareaElement.value = nextText;
      }
      handleInput(nextText);
      if (!prefill.append) {
        setAttachments(prefill.attachments);
      }
      textareaElement?.focus();
      textareaElement?.setSelectionRange(nextText.length, nextText.length);
    });
  });

  createEffect(() => {
    if (pendingDroppedFiles().length > 0) {
      void addAttachmentsFromPaths(takeDroppedFiles());
    }
  });

  const focusOnWindowActivate = () => {
    if (!state.paletteOpen() && !state.settingsTab() && document.activeElement === document.body) {
      textareaElement?.focus();
    }
  };

  onMount(() => {
    textareaElement?.focus();
    window.addEventListener("focus", focusOnWindowActivate);
    document.addEventListener("paste", handlePaste);
  });
  onCleanup(() => {
    window.removeEventListener("focus", focusOnWindowActivate);
    document.removeEventListener("paste", handlePaste);
    if (dictation) {
      dictation.cancel();
      void api.releaseSpeech();
    }
  });

  const placeholderText = () => {
    if (voiceStage() === "recording") {
      return "Слушаю…";
    }
    if (voiceStage() === "recognizing") {
      return "Распознаю речь…";
    }
    return "Напиши сообщение";
  };

  return (
    <div class="composer" classList={{ "composer-start": props.placement === "start" }}>
      <Show when={suggestedKey()}>
        <div class="key-suggestion" ref={keySuggestionElement}>
          <span>Похоже на API-ключ</span>
          <button class="key-suggestion-save" disabled={isSavingKey()} onClick={() => void saveSuggestedKey()}>
            {isSavingKey() ? "Проверяю…" : "Сохранить ключ"}
          </button>
        </div>
      </Show>

      <Show when={attachments().length > 0}>
        <div class="attachment-list">
          <For each={attachments()}>
            {(attachment) => (
              <Show
                when={attachment.kind === "image"}
                fallback={
                  <span class="attachment-chip">
                    <FileIcon />
                    <span class="attachment-name">{attachment.name}</span>
                    <button class="attachment-remove" aria-label={`Убрать ${attachment.name}`} onClick={() => removeAttachment(attachment)}>
                      <CloseIcon />
                    </button>
                  </span>
                }
              >
                <span class="attachment-preview">
                  <button class="image-open" aria-label={`Открыть ${attachment.name}`} onClick={() => actions.setViewedImage(imageDataUrl(attachment))}>
                    <img class="attachment-preview-image" src={imageDataUrl(attachment)} alt={attachment.name} />
                  </button>
                  <button class="attachment-preview-remove" aria-label={`Убрать ${attachment.name}`} onClick={() => removeAttachment(attachment)}>
                    <CloseIcon />
                  </button>
                </span>
              </Show>
            )}
          </For>
        </div>
      </Show>

      <textarea
        ref={textareaElement}
        id={`composer-input-${props.placement}`}
        class="composer-input"
        classList={{ "is-dictating": voiceStage() !== "idle" }}
        rows={props.placement === "start" ? 2 : 1}
        placeholder={placeholderText()}
        value={text()}
        onInput={(event) => handleTextareaInput(event.currentTarget.value)}
        onKeyDown={handleKeyDown}
      />

      <div class="composer-row">
        <button class="icon-button" title="Прикрепить файл" aria-label="Прикрепить файл" onClick={() => void pickFiles()}>
          <PlusIcon />
        </button>
        <ComposerMenu
          id={`access-menu-${props.placement}`}
          title="Режим доступа"
          buttonText={accessButtonText()}
          value={currentAccess().mode}
          options={ACCESS_OPTIONS}
          side="left"
          showNumberKeys
          isDanger={currentAccess().mode === "full"}
          onChoose={(mode) => void chooseAccessMode(mode as Parameters<typeof chooseAccessMode>[0])}
        />
        <Show when={currentAccess().mode !== "chat" && currentAccess().folder}>
          {(folder) => (
            <button class="folder-button" title={folder()} aria-label={`Рабочая папка: ${folder()}. Выбрать другую`} onClick={() => void chooseAccessFolder()}>
              <FolderIcon />
              <span class="menu-button-label">{shortFolderName(folder())}</span>
            </button>
          )}
        </Show>
        <span class="composer-hint">
          <Show
            when={voiceStage() === "idle"}
            fallback={
              <span class="voice-status" role="status">
                <Show when={voiceStage() === "recording"} fallback={<span class="shimmer">Уточняю текст</span>}>
                  <VoiceWave levels={voiceLevels} />
                </Show>
              </span>
            }
          >
            <Show when={inputEstimate()}>{(estimate) => <span>≈ {formatTokens(estimate())} ток.</span>}</Show>
          </Show>
        </span>
        <Show when={selectedEffortLevels().length > 0}>
          <ComposerMenu
            id={`effort-menu-${props.placement}`}
            title="Сколько модель думает"
            buttonText={effortButtonText()}
            value={selectedEffortLevels().includes(selectedEffort()) ? selectedEffort() : ""}
            options={effortOptions()}
            side="right"
            onChoose={chooseEffort}
          />
        </Show>
        <Show when={props.placement === "chat"}>
          <ContextMeter />
        </Show>
        <ModelPicker onSelect={selectModel} />
        <button
          class="icon-button voice-button"
          classList={{ "is-recording": voiceStage() === "recording" }}
          disabled={voiceStage() === "recognizing"}
          title={voiceStage() === "recording" ? "Закончить запись" : "Голосовой ввод"}
          aria-label={voiceStage() === "recording" ? "Закончить запись" : "Голосовой ввод"}
          aria-pressed={voiceStage() === "recording"}
          onMouseDown={(event) => event.preventDefault()}
          onClick={toggleRecording}
        >
          <MicrophoneIcon />
        </button>
        <button
          class="send-button"
          classList={{ "is-stop": isStreaming(), "is-ready": canSend() }}
          disabled={!isStreaming() && !canSend()}
          title={isStreaming() ? "Остановить (Esc)" : "Отправить (Enter)"}
          aria-label={isStreaming() ? "Остановить ответ" : "Отправить"}
          onClick={() => (isStreaming() ? void stopStreaming() : void submit())}
        >
          <span class="send-glyph send-arrow"><ArrowUpIcon /></span>
          <span class="send-glyph send-stop"><StopIcon /></span>
        </button>
      </div>
    </div>
  );
}
