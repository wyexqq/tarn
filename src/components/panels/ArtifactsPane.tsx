import { For, Match, Show, Switch, createMemo, createResource } from "solid-js";
import { save } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

import { api, type Artifact } from "../../api";
import { renderMarkdown } from "../../markdown";
import { chooseArtifact, describeArtifact, openPanel, panels } from "../../panels";
import { showToast } from "../../store";
import { FileIcon } from "../icons";

const EXTENSIONS: Record<string, string> = { html: "html", markdown: "md", text: "txt", csv: "csv", svg: "svg", json: "json", code: "txt" };
const PAGE_FILE = /\.(html?|svg|pdf)$/i;

function separatorOf(text: string): string {
  const firstLine = text.split("\n", 1)[0];
  const semicolons = firstLine.split(";").length;
  const commas = firstLine.split(",").length;
  return semicolons > commas ? ";" : ",";
}

export function parseCsv(text: string): string[][] {
  const separator = separatorOf(text);
  const rows: string[][] = [];
  let row: string[] = [];
  let cell = "";
  let insideQuotes = false;
  for (let index = 0; index < text.length; index += 1) {
    const letter = text[index];
    if (insideQuotes) {
      if (letter === '"' && text[index + 1] === '"') {
        cell += '"';
        index += 1;
      } else if (letter === '"') {
        insideQuotes = false;
      } else {
        cell += letter;
      }
    } else if (letter === '"') {
      insideQuotes = true;
    } else if (letter === separator) {
      row.push(cell);
      cell = "";
    } else if (letter === "\n") {
      row.push(cell.replace(/\r$/, ""));
      rows.push(row);
      row = [];
      cell = "";
    } else {
      cell += letter;
    }
  }
  if (cell !== "" || row.length > 0) {
    row.push(cell.replace(/\r$/, ""));
    rows.push(row);
  }
  return rows;
}

function canOpenAsPage(artifact: Artifact): boolean {
  return artifact.kind === "html" || artifact.kind === "svg" || (artifact.kind === "file" && PAGE_FILE.test(artifact.path));
}

function Preview(props: { artifact: Artifact }) {
  const [text] = createResource(() => props.artifact.id, (id) => api.artifactText(id).catch((error) => ({ problem: String(error) })));
  const content = () => {
    const loaded = text();
    return typeof loaded === "string" ? loaded : null;
  };
  const problem = () => {
    const loaded = text();
    return loaded && typeof loaded === "object" ? loaded.problem : null;
  };
  const tableRows = createMemo(() => (props.artifact.kind === "csv" && content() ? parseCsv(content() ?? "").slice(0, 500) : []));

  return (
    <Show when={content() !== null} fallback={<p class="side-empty">{problem() ?? "Загружаю…"}</p>}>
      <Switch fallback={<pre class="code-view">{content()}</pre>}>
        <Match when={props.artifact.kind === "markdown"}>
          <div class="markdown artifact-document" innerHTML={renderMarkdown(content() ?? "")} />
        </Match>
        <Match when={props.artifact.kind === "csv"}>
          <div class="artifact-table-wrap">
            <table class="artifact-table">
              <For each={tableRows()}>
                {(row, rowIndex) => (
                  <tr>
                    <For each={row}>{(cell) => (rowIndex() === 0 ? <th>{cell}</th> : <td>{cell}</td>)}</For>
                  </tr>
                )}
              </For>
            </table>
          </div>
        </Match>
        <Match when={props.artifact.kind === "html" || props.artifact.kind === "svg"}>
          <iframe class="artifact-frame" sandbox="" srcdoc={content() ?? ""} title={props.artifact.title} />
        </Match>
      </Switch>
    </Show>
  );
}

export function ArtifactsPane() {
  const selected = createMemo(() => {
    const listed = panels.artifacts();
    return listed.find((artifact) => artifact.id === panels.selectedArtifactId()) ?? listed[listed.length - 1] ?? null;
  });

  const saveCopy = async (artifact: Artifact) => {
    const extension = EXTENSIONS[artifact.kind];
    const safeTitle = artifact.title.replace(/[\\/:*?"<>|]/g, " ").trim() || "артефакт";
    const target = await save({ title: "Сохранить как файл", defaultPath: extension ? `${safeTitle}.${extension}` : safeTitle });
    if (!target) {
      return;
    }
    try {
      await api.saveArtifactCopy(artifact.id, target);
      showToast("Файл сохранён");
    } catch (error) {
      showToast(String(error), "error");
    }
  };

  const openAsPage = async (artifact: Artifact) => {
    try {
      openPanel("browser");
      await api.openArtifactInBrowser(artifact.id);
    } catch (error) {
      showToast(String(error), "error");
    }
  };

  return (
    <section class="side-pane side-pane-fill">
      <Show when={panels.artifacts().length > 0} fallback={<p class="side-empty">Артефактов пока нет</p>}>
        <div class="artifact-list">
          <For each={panels.artifacts()}>
            {(artifact) => (
              <button class="artifact-row" classList={{ "is-active": selected()?.id === artifact.id }} onClick={() => chooseArtifact(artifact.id)}>
                <FileIcon />
                <span class="artifact-row-main">
                  <span class="artifact-row-title">{artifact.title}</span>
                  <span class="artifact-row-meta">{describeArtifact(artifact)}</span>
                </span>
              </button>
            )}
          </For>
        </div>
        <Show when={selected()} keyed>
          {(artifact) => (
            <div class="artifact-preview">
              <div class="artifact-actions">
                <button class="ghost-button" onClick={() => void saveCopy(artifact)}>Сохранить как файл</button>
                <Show when={canOpenAsPage(artifact)}>
                  <button class="ghost-button" onClick={() => void openAsPage(artifact)}>Открыть в браузере</button>
                </Show>
                <Show when={artifact.kind === "file"}>
                  <button class="ghost-button" onClick={() => void revealItemInDir(artifact.path)}>Показать в папке</button>
                </Show>
              </div>
              <Preview artifact={artifact} />
            </div>
          )}
        </Show>
      </Show>
    </section>
  );
}
