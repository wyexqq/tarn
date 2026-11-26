import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { chromium } from "playwright-core";

const SHOTS_DIR = process.env.TARN_SHOTS_DIR ?? "e2e-shots";
const MOCK_URL = process.env.TARN_MOCK_URL ?? "http://127.0.0.1:18765/v1";
const STEP_TIMEOUT_MS = 15000;

mkdirSync(SHOTS_DIR, { recursive: true });

const failures = [];

function check(name, condition, detail = "") {
  console.log(`${condition ? "ok  " : "FAIL"} ${name}${detail ? ` | ${detail}` : ""}`);
  if (!condition) {
    failures.push(name);
  }
}

const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
const page = browser.contexts()[0].pages()[0];
page.setDefaultTimeout(STEP_TIMEOUT_MS);

const consoleErrors = [];
page.on("console", (message) => {
  if (message.type() === "error") {
    consoleErrors.push(message.text());
  }
});
page.on("pageerror", (error) => consoleErrors.push(String(error)));

async function shot(name) {
  await page.waitForTimeout(350);
  await page.screenshot({ path: join(SHOTS_DIR, `${name}.png`) });
}

async function pressCtrl(key) {
  await page.keyboard.press(`Control+${key}`);
}

async function choose(selectId, optionText) {
  await page.click(`#${selectId}`);
  await page.locator(".select-option", { hasText: optionText }).first().click();
  await page.waitForSelector(".select-menu", { state: "detached" });
}

async function closeSettings() {
  await page.keyboard.press("Escape");
  await page.waitForSelector(".settings", { state: "detached" });
}

async function addCustomProvider(name, baseUrl, protocol) {
  await pressCtrl(",");
  await page.waitForSelector(".settings");
  await page.click("text=Подключить свой адрес API");
  await page.fill("#custom-name", name);
  await page.fill("#custom-url", baseUrl);
  await choose("custom-protocol", protocol === "anthropic" ? "Формат Anthropic" : "Формат OpenAI");
  await page.click(".custom-form button[type=submit]");
  await page.waitForSelector(".custom-form", { state: "detached" });
  await closeSettings();
}

async function waitForAnswerDone() {
  await page.waitForFunction(() => document.querySelector(".send-button.is-stop") === null, null, { timeout: 30000 });
}

async function send(text) {
  const input = page.locator(".composer-input");
  await input.fill(text);
  await input.press("Enter");
}

async function sendAndWaitForAnswer(text) {
  const answersBefore = await page.locator(".message-assistant").count();
  await send(text);
  await page.waitForFunction((count) => document.querySelectorAll(".message-assistant").length > count, answersBefore);
  await waitForAnswerDone();
  await page.waitForTimeout(300);
}

await page.waitForSelector(".app");
await shot("01-start-empty");
check("start heading", await page.isVisible(".start-title"));

await send("привет");
await page.waitForSelector(".settings");
check("sending without a key opens the key settings", await page.isVisible("#key-input"));
check("unsent text is kept", (await page.locator(".composer-input").inputValue()) === "привет");
await page.keyboard.press("Escape");
await page.waitForSelector(".settings", { state: "detached" });
await page.locator(".composer-input").fill("");

await pressCtrl(",");
await page.waitForSelector(".settings");
await page.click("text=Подключить свой адрес API");
await page.fill("#custom-name", "Mock Secure");
await page.fill("#custom-url", MOCK_URL.replace("/v1", "/secure/v1"));
await page.fill("#custom-key", "wrong-key");
await page.click(".custom-form button[type=submit]");
await page.waitForSelector(".custom-form .form-error");
check("wrong key is rejected with a reason", (await page.locator(".custom-form .form-error").innerText()).length > 0, await page.locator(".custom-form .form-error").innerText());
await page.fill("#custom-key", "good-key");
await page.click(".custom-form button[type=submit]");
await page.waitForSelector(".custom-form", { state: "detached" });
const secureRow = page.locator(".provider-row", { hasText: "Mock Secure" });
check("correct key connects the provider", (await secureRow.locator(".provider-status").innerText()).includes("модел"));
const keyStorageSafe = await page.evaluate(async () => {
  const providers = await window.__TAURI_INTERNALS__.invoke("list_providers");
  return !JSON.stringify(providers).includes("good-key");
});
check("key never reaches the interface", keyStorageSafe);
await secureRow.locator('[aria-label^="Удалить"]').click();
await page.waitForFunction(() => ![...document.querySelectorAll(".provider-name")].some((item) => item.textContent === "Mock Secure"));
await page.keyboard.press("Escape");
await page.waitForSelector(".settings", { state: "detached" });

await pressCtrl(",");
await page.waitForSelector(".settings");
await shot("02-settings-keys");
await page.keyboard.press("Escape");
await page.waitForSelector(".settings", { state: "detached" });
check("escape closes settings", true);

await addCustomProvider("Mock OpenAI", MOCK_URL, "openai");
await addCustomProvider("Mock Anthropic", MOCK_URL, "anthropic");
await page.waitForFunction(() => document.querySelector(".model-button-label")?.textContent !== "Выбери модель");
await shot("03-start-with-providers");

await page.click(".model-button");
await page.waitForSelector(".model-menu");
await shot("04-model-menu");
const embeddingVisible = await page.locator(".model-row", { hasText: "embedding" }).count();
check("embedding models are hidden", embeddingVisible === 0);
await page.keyboard.press("Escape");
await page.waitForSelector(".model-menu", { state: "detached" });
await page.click(".model-button");
await page.waitForSelector(".model-menu");
await page.fill("#model-search", "smart");
await page.keyboard.press("Enter");
await page.waitForSelector(".model-menu", { state: "detached" });
check("model picker works from the keyboard", (await page.locator(".model-button-label").innerText()) === "Mock Smart");

await page.locator(".composer-input").fill("sk-abcdefghijklmnopqrstuvwxyz123456");
await page.waitForSelector(".key-suggestion");
await shot("05-key-suggestion");
check("api key is detected in composer", true);
await page.locator(".composer-input").press("Enter");
check("api key is not sent", await page.isVisible(".start-title"));
check("key warning is shown once, without a toast", (await page.locator(".toast-error").count()) === 0);
await page.locator(".composer-input").fill("");

await send("Почему падает сборка на Windows?");
await page.waitForSelector(".chat");
await page.waitForTimeout(120);
await page.screenshot({ path: join(SHOTS_DIR, "06-streaming.png") });
check("stop button while streaming", await page.isVisible(".send-button.is-stop"));
await waitForAnswerDone();
await shot("07-answer-done");
const answerText = await page.locator(".message-assistant .markdown").last().innerText();
check("answer rendered", answerText.includes("Компоновщик"));
check("code block rendered", (await page.locator(".code-block").count()) > 0);
check("table rendered", (await page.locator(".markdown table").count()) > 0);

await send("Ещё раз, но длинный ответ");
await page.waitForTimeout(4000);
const scrollMetrics = () =>
  page.evaluate(() => {
    const element = document.querySelector(".chat-scroll");
    return { top: element.scrollTop, distance: element.scrollHeight - element.scrollTop - element.clientHeight };
  });
const whileFollowing = await scrollMetrics();
check("follows the answer while it is written", whileFollowing.distance < 40, `distance ${whileFollowing.distance}`);
const scrollBox = await page.locator(".chat-scroll").boundingBox();
await page.mouse.move(scrollBox.x + scrollBox.width / 2, scrollBox.y + scrollBox.height / 2);
await page.mouse.wheel(0, -40);
await page.waitForTimeout(300);
const afterWheel = await scrollMetrics();
await page.waitForTimeout(1500);
const afterWaiting = await scrollMetrics();
check("small scroll up during streaming is not undone", afterWaiting.top <= afterWheel.top + 1, `${afterWheel.top} -> ${afterWaiting.top}`);
check("distance from bottom grows while reading above", afterWaiting.distance > afterWheel.distance, `${afterWheel.distance} -> ${afterWaiting.distance}`);
check("streaming caret is gone", await page.evaluate(() => {
  const element = document.querySelector(".message-assistant:last-of-type .markdown > :last-child");
  return element === null || getComputedStyle(element, "::after").content === "none";
}));
await page.locator(".composer-input").focus();
await page.keyboard.press("Escape");
await waitForAnswerDone();
await shot("08-after-stop");
const stoppedStatus = await page.locator(".message-assistant").last().innerText();
check("stop keeps partial answer", stoppedStatus.length > 0);

await page.locator(".message-assistant").last().hover();
await page.locator(".message-assistant").last().locator('[aria-label="Ответить заново"]').click();
await page.waitForTimeout(150);
await waitForAnswerDone();
await page.waitForSelector(".branch-switcher");
await shot("09-branches");
check("regenerate creates branch", (await page.locator(".branch-switcher").count()) > 0);
check("regenerated answer has no error", (await page.locator(".message-assistant").last().locator(".message-error").count()) === 0);
const branchLabel = await page.locator(".branch-switcher span").last().innerText();
check("branch counter shows 2/2", branchLabel === "2/2", branchLabel);
await page.locator('[aria-label="Предыдущий вариант"]').last().click();
await page.waitForFunction(() => document.querySelector(".branch-switcher span")?.textContent === "1/2");
check("branch switch works", true);

const userMessage = page.locator(".message-user").first();
await userMessage.hover();
await userMessage.locator('[aria-label="Изменить сообщение"]').click();
await page.waitForSelector(".edit-box");
await shot("10-edit");
await page.fill(".edit-input", "Почему падает сборка на Linux?");
await page.click(".edit-actions .primary-button");
await page.waitForTimeout(150);
await waitForAnswerDone();
const firstBubble = await page.locator(".user-bubble").first().innerText();
check("edit updates thread", firstBubble.includes("Linux"), firstBubble);
check("edited branch has an answer", (await page.locator(".message-error").count()) === 0);

await page.click(".model-button");
await page.waitForSelector(".model-menu");
await page.locator(".model-group", { hasText: "Mock OpenAI" }).locator(".model-row", { hasText: "Mock Fast" }).click();
await send("Покажи ошибка на сервере");
await page.waitForSelector(".message-error");
const errorText = await page.locator(".message-error span").innerText();
check("provider error shows the provider's reason", errorText.includes("upstream exploded") || errorText.includes("500"), errorText);
await shot("11-error");
check("provider error is visible", await page.isVisible(".message-error"));
await waitForAnswerDone();

await page.evaluate(() => {
  const originalFetch = window.fetch;
  window.__delayedSends = 0;
  window.__restoreFetch = () => {
    window.fetch = originalFetch;
  };
  window.fetch = async (url, options) => {
    const response = await originalFetch(url, options);
    if (String(url).includes("send_message")) {
      window.__delayedSends += 1;
      await new Promise((resolve) => setTimeout(resolve, 800));
    }
    return response;
  };
});
const errorsBeforeFastFailure = await page.locator(".message-error").count();
await send("Ещё одна ошибка на сервере");
await page.waitForFunction(() => window.__delayedSends > 0);
const fastFailureShown = await page
  .waitForFunction((count) => document.querySelectorAll(".message-error").length > count, errorsBeforeFastFailure, { timeout: 5000 })
  .then(() => true)
  .catch(() => false);
check("failure arriving before the message is on screen is not lost", fastFailureShown);
await page.evaluate(() => window.__restoreFetch());
if (fastFailureShown) {
  await waitForAnswerDone();
}

await page.evaluate(() => {
  const element = document.querySelector(".chat-scroll");
  element.scrollTop = 0;
});
await pressCtrl("b");
await page.waitForSelector(".history.is-open");
await shot("12-history");
check("history lists the chat", (await page.locator(".chat-row").count()) >= 1);
await page.waitForTimeout(400);
const panelBox = await page.locator(".history").boundingBox();
const mainBox = await page.locator(".workspace-main").boundingBox();
check("panel moves the chat instead of covering it", mainBox.x >= panelBox.x + panelBox.width - 1);
const scrimVisible = await page.locator(".history-scrim").isVisible();
check("wide window has no dimming behind the panel", !scrimVisible);
await page.locator(".chat-row-open").first().click();
await page.waitForTimeout(300);
check("panel stays open after choosing a chat", await page.isVisible(".history.is-open"));
await page.setViewportSize({ width: 760, height: 560 });
await page.waitForTimeout(400);
await shot("12c-history-narrow");
check("narrow window dims behind the panel", await page.locator(".history-scrim.is-open").isVisible());
await page.keyboard.press("Escape");
await page.waitForFunction(() => document.querySelector(".history.is-open") === null);
check("escape closes the panel in a narrow window", true);
await pressCtrl("b");
await page.waitForSelector(".history.is-open");
await page.locator(".chat-row-open").first().click();
await page.waitForFunction(() => document.querySelector(".history.is-open") === null);
check("choosing a chat closes the panel in a narrow window", true);
await page.setViewportSize({ width: 1100, height: 740 });
await page.locator(".rail-button").nth(2).hover();
await page.waitForTimeout(500);
await shot("12d-rail-hint");
const hintOpacity = await page.locator(".rail-button").nth(2).evaluate((element) => getComputedStyle(element, "::after").opacity);
check("rail button shows a hint with its shortcut", hintOpacity === "1", hintOpacity);
await page.mouse.move(600, 300);
await page.locator(".chat-scroll").click({ position: { x: 5, y: 5 } });
await page.keyboard.press("Tab");
const firstFocus = await page.evaluate(() => document.activeElement?.tagName ?? "");
check("tab moves keyboard focus to a control", ["BUTTON", "A", "TEXTAREA", "INPUT"].includes(firstFocus), firstFocus);
const unnamedControls = await page.evaluate(() =>
  [...document.querySelectorAll("button")]
    .filter((button) => !button.textContent.trim() && !button.getAttribute("aria-label"))
    .map((button) => button.className),
);
check("every icon button has a name for screen readers", unnamedControls.length === 0, unnamedControls.join(", "));

await pressCtrl("k");
await page.waitForSelector(".palette");
await shot("13-palette");
await page.fill("#palette-input", "Компоновщик");
await page.waitForSelector(".palette-snippet");
await shot("14-palette-search");
check("full text search finds answer", (await page.locator(".palette-snippet mark").count()) > 0);
const snippetText = await page.locator(".palette-snippet").first().innerText();
check("snippet has no markdown syntax", !snippetText.includes("**"), snippetText);
const hitTitles = await page.locator(".palette-row .palette-title").allInnerTexts();
check("one result per chat", new Set(hitTitles).size === hitTitles.length, hitTitles.join(" | "));
await page.keyboard.press("Enter");
await page.waitForSelector(".palette", { state: "detached" });
check("search result opens chat", await page.isVisible(".chat"));

await pressCtrl("n");
await page.waitForSelector(".start-title");
await shot("15-start-with-recent");
check("recent chats on start", (await page.locator(".resume-row").count()) >= 1);

await page.click(".model-button");
await page.waitForSelector(".model-menu");
await page.click('[aria-label="Режим сравнения моделей"]');
await page.locator(".model-row", { hasText: "Mock Fast" }).first().click();
await page.locator(".model-row", { hasText: "Mock Smart" }).first().click();
await shot("16-compare-picker");
await page.keyboard.press("Escape");
await send("Сравни ответы");
await page.waitForSelector(".compare");
await waitForAnswerDone();
await shot("17-compare");
check("compare shows two columns", (await page.locator(".compare-column").count()) === 2);
check("compare answers have no errors", (await page.locator(".compare .message-error").count()) === 0);
await page.locator(".compare-foot button").first().click();
await page.waitForSelector(".compare", { state: "detached" });
await shot("18-compare-chosen");
check("compare choice continues chat", (await page.locator(".message-assistant").count()) >= 1);

await pressCtrl(",");
await page.waitForSelector(".settings");
await page.locator(".settings-tab", { hasText: "Импорт" }).click();
await shot("19-settings-import");
await page.locator(".settings-tab", { hasText: "Расходы" }).click();
await shot("20-settings-usage");
await page.locator(".settings-tab", { hasText: "Журнал" }).click();
await shot("21-settings-log");
check("request log has entries", (await page.locator(".data-table tbody tr").count()) > 0);
const logText = await page.locator(".data-table").innerText();
check("request log has no message text", !logText.includes("Почему падает"));
await page.locator(".settings-tab", { hasText: "Общие" }).click();
await shot("22-settings-general");
await page.click("#theme-select");
await page.waitForSelector(".select-menu");
await shot("22b-select-open");
await page.keyboard.press("Escape");
await page.waitForSelector(".select-menu", { state: "detached" });
check("escape closes only the dropdown", await page.isVisible(".settings"));
await choose("theme-select", "Светлая");
await shot("23-settings-light");
await page.keyboard.press("Escape");
await page.waitForSelector(".settings", { state: "detached" });
await shot("24-chat-light");
await pressCtrl("n");
await shot("25-start-light");

await pressCtrl(",");
await page.waitForSelector(".settings");
await page.locator(".settings-tab", { hasText: "Общие" }).click();
await choose("theme-select", "Тёмная");
await closeSettings();
await shot("26-start-dark");
await page.locator(".resume-row").nth(1).click();
await page.waitForSelector(".chat");
await shot("27-chat-dark");
const openedDistance = await page.evaluate(() => {
  const element = document.querySelector(".chat-scroll");
  return element.scrollHeight - element.scrollTop - element.clientHeight;
});
check("opened chat is scrolled to the end", openedDistance < 4, `distance ${openedDistance}`);

const textFile = join(SHOTS_DIR, "attachment.txt");
writeFileSync(textFile, "секретное слово в файле");
const exportFile = join(SHOTS_DIR, "export.md");
const attachmentResult = await page.evaluate(async ({ textFile, exportFile }) => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const attachment = await invoke("read_attachment", { path: textFile });
  const chats = await invoke("list_chats");
  await invoke("export_chat", { chatId: chats[0].id, path: exportFile });
  return attachment;
}, { textFile, exportFile });
check("text file is read as an attachment", attachmentResult.kind === "text" && attachmentResult.text.includes("секретное"));

const dropFiles = (paths) =>
  page.evaluate(
    (paths) =>
      window.__TAURI_INTERNALS__.invoke("plugin:event|emit", { event: "tauri://drag-drop", payload: { paths, position: { x: 200, y: 200 } } }),
    paths,
  );
await dropFiles([textFile, join(SHOTS_DIR, "01-start-empty.png")]);
await page.waitForFunction(() => document.querySelectorAll(".composer .attachment-chip, .composer .attachment-preview").length === 2);
await shot("29-dropped-files");
check("dropped image gets a preview", (await page.locator(".composer .attachment-preview-image").count()) === 1);
await page.locator(".composer .attachment-preview-remove").click();
await sendAndWaitForAnswer("Прочитай файл");
const lastLogEntry = await page.evaluate(async () => (await window.__TAURI_INTERNALS__.invoke("request_log"))[0]);
check("attached file is listed in the request log", lastLogEntry.files.includes("attachment.txt"), lastLogEntry.files);
await dropFiles([process.execPath]);
await page.waitForSelector(".toast-error");
await shot("30-binary-rejected");
check("binary file is refused with an explanation", (await page.locator(".composer .attachment-chip").count()) === 0, await page.locator(".toast-error").last().innerText());

await page.click(".model-button");
await page.waitForSelector(".model-menu");
await page.locator(".model-group", { hasText: "Mock OpenAI" }).locator(".model-row", { hasText: "Mock Small" }).click();
const paddedText = "Длинный контекст для маленькой модели. ".repeat(1200);
await sendAndWaitForAnswer(paddedText);
const tooLongError = await page.locator(".message-assistant").last().locator(".message-error").innerText();
check("message larger than the model gets a clear explanation", tooLongError.includes("не помещается"), tooLongError);
await sendAndWaitForAnswer("Короткий вопрос после длинного");
await shot("31-context-shrunk");
check("overflowing context is shortened automatically", (await page.locator(".message-assistant").last().locator(".message-error").count()) === 0);
const shrinkNote = await page.locator(".message-assistant").last().locator(".message-note").innerText().catch(() => "");
check("user is told the context was shortened", shrinkNote.includes("Модель получила только последние"), shrinkNote);
check("chat export writes a file", readFileSync(exportFile, "utf8").length > 50);
await page.evaluate(() => navigator.clipboard.writeText("")).catch(() => undefined);
await pressCtrl("k");
await page.waitForSelector(".palette");
await shot("28-palette-dark");
await page.keyboard.press("Escape");
await page.waitForSelector(".palette", { state: "detached" });
await pressCtrl("n");
await page.waitForSelector(".composer-input");

const focusIsInSettings = () => page.evaluate(() => document.querySelector(".settings").contains(document.activeElement));
const focusedClasses = () => page.evaluate(() => document.activeElement?.className ?? "");
await pressCtrl(",");
await page.waitForSelector(".settings");
await page.waitForTimeout(60);
check("settings takes keyboard focus when opened", await focusIsInSettings());
await page.keyboard.press("ArrowDown");
await page.waitForTimeout(60);
check("arrow keys move between settings tabs", (await focusedClasses()).includes("settings-tab") && (await page.locator(".settings-tab.is-active").innerText()) !== "Ключи");
await page.keyboard.press("ArrowUp");
let focusEscapes = 0;
for (let step = 0; step < 30; step += 1) {
  await page.keyboard.press("Tab");
  if (!(await focusIsInSettings())) {
    focusEscapes += 1;
  }
}
check("tab stays inside settings", focusEscapes === 0, `left ${focusEscapes} times`);
await closeSettings();
await page.waitForTimeout(220);
check("focus returns after settings close", (await page.evaluate(() => document.activeElement?.tagName)) !== "BODY");

await page.click(".model-button");
await page.waitForSelector("#model-search");
await page.waitForTimeout(60);
await page.keyboard.press("ArrowDown");
const announcedOption = await page.evaluate(() => {
  const id = document.querySelector("#model-search").getAttribute("aria-activedescendant");
  return document.getElementById(id)?.getAttribute("role") ?? "";
});
check("current model is exposed to screen readers", announcedOption === "option", announcedOption);
await page.keyboard.press("Enter");
await page.waitForSelector(".model-menu", { state: "detached" });
check("choosing a model returns focus to the input", (await focusedClasses()).includes("composer-input"), await focusedClasses());
await page.click(".model-button");
await page.waitForSelector("#model-search");
await page.waitForTimeout(60);
await page.keyboard.press("Escape");
await page.waitForSelector(".model-menu", { state: "detached" });
check("escape closes the model menu and returns focus to its button", (await focusedClasses()).includes("model-button"), await focusedClasses());

await pressCtrl("k");
await page.waitForSelector(".palette");
await page.waitForTimeout(60);
check("palette focuses its search field", (await focusedClasses()).includes("palette-input"));
await page.keyboard.press("Escape");
await page.waitForSelector(".palette", { state: "detached" });
await page.waitForTimeout(220);
check("focus returns after the palette closes", (await page.evaluate(() => document.activeElement?.tagName)) !== "BODY");

const pasteClipboardFile = (name, mime, base64) =>
  page.evaluate(({ name, mime, base64 }) => {
    const bytes = Uint8Array.from(atob(base64), (symbol) => symbol.charCodeAt(0));
    const clipboardData = new DataTransfer();
    clipboardData.items.add(new File([bytes], name, { type: mime }));
    const event = new ClipboardEvent("paste", { clipboardData, bubbles: true, cancelable: true });
    document.querySelector(".composer-input").dispatchEvent(event);
  }, { name, mime, base64 });
const TINY_PNG = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=";

await pressCtrl("n");
await page.waitForSelector(".composer-start");
await pasteClipboardFile("image.png", "image/png", TINY_PNG);
await page.waitForSelector(".attachment-preview-image");
check("pasted picture appears as a large preview", (await page.locator(".attachment-preview-image").count()) === 1);
await shot("30-pasted-image");
await page.click(".attachment-preview .image-open");
await page.waitForSelector(".image-viewer");
check("clicking a preview opens the picture", (await page.locator(".image-viewer-picture").count()) === 1);
await page.keyboard.press("Escape");
await page.waitForSelector(".image-viewer", { state: "detached" });
await page.click(".attachment-preview-remove");
check("preview can be removed", (await page.locator(".attachment-preview").count()) === 0);

const textBeforePaste = "обычный текст";
await page.evaluate((text) => {
  const clipboardData = new DataTransfer();
  clipboardData.setData("text/plain", text);
  document.querySelector(".composer-input").dispatchEvent(new ClipboardEvent("paste", { clipboardData, bubbles: true, cancelable: true }));
}, textBeforePaste);
check("plain text paste adds no attachment", (await page.locator(".attachment-preview, .attachment-chip").count()) === 0);

await pasteClipboardFile("image.png", "image/png", TINY_PNG);
await page.waitForSelector(".attachment-preview-image");
await page.click(".send-button.is-ready");
await page.waitForSelector(".message-image");
await waitForAnswerDone();
const requestWithPicture = await (await fetch(`${MOCK_URL}/last-request`)).json();
check("pasted picture reaches the model", requestWithPicture.imageCount === 1, JSON.stringify(requestWithPicture));
check("picture without text sends no empty text", requestWithPicture.emptyTextParts === 0);
const storedAttachment = await page.evaluate(() => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  return invoke("list_chats").then((chats) => invoke("get_thread", { chatId: chats[0].id, limit: 10, beforeId: null, ensureId: null })).then((thread) => thread.messages[0].attachments[0]);
});
check("sent picture is kept outside the message", Boolean(storedAttachment.imageId) && !storedAttachment.dataBase64, JSON.stringify(storedAttachment));
await shot("31-sent-image");

const windowWidth = () => page.evaluate(() => window.innerWidth);
const widthAtStart = await windowWidth();
await page.mouse.move(500, 300);
await page.keyboard.down("Control");
await page.mouse.wheel(0, -100);
await page.waitForTimeout(150);
await page.mouse.wheel(0, -100);
await page.keyboard.up("Control");
await page.waitForTimeout(250);
const widthZoomedIn = await windowWidth();
check("ctrl and wheel up enlarges the app", widthZoomedIn < widthAtStart * 0.9, `${widthAtStart} -> ${widthZoomedIn}`);
check("zoom shows its percent", (await page.locator(".zoom-badge").innerText()) === "120%", await page.locator(".zoom-badge").innerText());
await shot("32-zoomed-in");
await page.keyboard.down("Control");
await page.mouse.wheel(0, 100);
await page.keyboard.up("Control");
await page.waitForTimeout(200);
check("ctrl and wheel down shrinks the app", (await windowWidth()) > widthZoomedIn, String(await windowWidth()));
await page.waitForTimeout(500);
const savedZoom = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke("get_settings").then((settings) => settings.zoom));
check("zoom is remembered", savedZoom === "1.1", String(savedZoom));
await pressCtrl("0");
await page.waitForTimeout(200);
check("ctrl 0 resets zoom", Math.abs((await windowWidth()) - widthAtStart) <= 2, String(await windowWidth()));

await page.setViewportSize({ width: 660, height: 500 }).catch(() => undefined);

check("no console errors", consoleErrors.length === 0, consoleErrors.slice(0, 5).join(" || "));

await browser.close();
console.log(failures.length === 0 ? "ALL PASSED" : `FAILED: ${failures.join(", ")}`);
process.exit(failures.length === 0 ? 0 : 1);
