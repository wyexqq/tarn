import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const SHOTS_DIR = process.env.TARN_SHOTS_DIR ?? "e2e-shots";
const MOCK_URL = process.env.TARN_MOCK_URL ?? "http://127.0.0.1:18765/v1";
const WORKSPACE = process.env.TARN_WORKSPACE;
const OUTSIDE = process.env.TARN_OUTSIDE;
const DATA_DIR = process.env.TARN_DATA_DIR;
const FAKE_SPEECH_LOG = process.env.TARN_FAKE_SPEECH_LOG;
const MOCK_MCP_COMMAND = `node "${join(dirname(fileURLToPath(import.meta.url)), "mock-mcp.mjs")}"`;
const STEP_TIMEOUT_MS = 20000;
const MENU_ANIMATION_MS = 300;

mkdirSync(SHOTS_DIR, { recursive: true });
rmSync(WORKSPACE, { recursive: true, force: true });
rmSync(OUTSIDE, { recursive: true, force: true });
mkdirSync(WORKSPACE, { recursive: true });
mkdirSync(join(OUTSIDE, "skills", "demo"), { recursive: true });
writeFileSync(join(WORKSPACE, "a.txt"), "содержимое файла a");
writeFileSync(join(OUTSIDE, "secret.txt"), "секрет снаружи");
writeFileSync(join(OUTSIDE, "skills", "demo", "SKILL.md"), "---\nname: demo\ndescription: Показательный скилл\n---\nИнструкция скилла demo");

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

const invoke = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args), { command, args });
const lastRequest = async () => (await fetch(`${MOCK_URL}/last-request`)).json();

async function shot(name) {
  await page.waitForTimeout(300);
  await page.screenshot({ path: join(SHOTS_DIR, `${name}.png`) });
}

async function choose(selectId, optionText) {
  await page.click(`#${selectId}`);
  await page.locator(".select-option", { hasText: optionText }).first().click();
  await page.waitForSelector(".select-menu", { state: "detached" });
}

async function openSettingsTab(label) {
  if ((await page.locator(".settings").count()) === 0) {
    await page.keyboard.press("Control+,");
    await page.waitForSelector(".settings");
  }
  await page.locator(".settings-tab", { hasText: label }).click();
  await page.waitForTimeout(150);
}

async function closeSettings() {
  await page.keyboard.press("Escape");
  await page.waitForSelector(".settings", { state: "detached" });
}

async function addCustomProvider(name, protocol) {
  await openSettingsTab("Ключи");
  await page.click("text=Подключить свой адрес API");
  await page.fill("#custom-name", name);
  await page.fill("#custom-url", MOCK_URL);
  await choose("custom-protocol", protocol === "anthropic" ? "Формат Anthropic" : "Формат OpenAI");
  await page.click(".custom-form button[type=submit]");
  await page.waitForSelector(".custom-form", { state: "detached" });
  await closeSettings();
}

async function selectModel(searchText) {
  await page.click(".model-button");
  await page.fill("#model-search", searchText);
  await page.keyboard.press("Enter");
  await page.waitForSelector(".model-menu", { state: "detached" });
}

async function waitForAnswerDone() {
  await page.waitForFunction(() => document.querySelector(".send-button.is-stop") === null, null, { timeout: 40000 });
  await page.waitForTimeout(200);
}

async function send(text) {
  await page.fill(".composer-input", text);
  await page.press(".composer-input", "Enter");
  await page.waitForTimeout(150);
}

async function sendAndWait(text) {
  await send(text);
  await waitForAnswerDone();
}

const lastAnswerText = () => page.locator(".message-assistant").last().innerText();

async function chooseMode(title) {
  await page.click("#access-menu-chat");
  await page.locator(".composer-menu-option", { hasText: title }).first().click();
  await page.waitForSelector(".composer-menu", { state: "detached" });
  await page.waitForTimeout(150);
}

function boxesTouch(menu, button) {
  const verticalGap = Math.min(Math.abs(button.y - (menu.y + menu.height)), Math.abs(menu.y - (button.y + button.height)));
  const leftAligned = Math.abs(menu.x - button.x) <= 1;
  const rightAligned = Math.abs(menu.x + menu.width - (button.x + button.width)) <= 1;
  return verticalGap <= 6 && (leftAligned || rightAligned);
}

await page.waitForSelector(".composer-input");
await addCustomProvider("Mock OpenAI", "openai");
await addCustomProvider("Mock Anthropic", "anthropic");

await selectModel("Mock Effort Mock Anthropic");
check("effort button appears for a model with levels", (await page.locator("#effort-menu-start").count()) === 1);
await page.click("#effort-menu-start");
await page.waitForSelector(".composer-menu");
await page.waitForTimeout(MENU_ANIMATION_MS);
const effortOptions = await page.locator(".composer-menu-option strong").allInnerTexts();
check("effort lists only the levels of this model", effortOptions.join() === "Как у модели,Низкий,Высокий,Максимум", effortOptions.join());
const startButton = await page.locator("#effort-menu-start").boundingBox();
const startMenu = await page.locator(".composer-menu").boundingBox();
check("effort menu opens right from its button on start screen", boxesTouch(startMenu, startButton), JSON.stringify({ startMenu, startButton }));
await shot("50-effort-menu-start");
await page.keyboard.press("Escape");
await page.waitForSelector(".composer-menu", { state: "detached" });
check("escape returns focus to the menu button", await page.evaluate(() => document.activeElement?.id === "effort-menu-start"));

await sendAndWait("рабочий чат");
const defaultRequest = await lastRequest();
check("default effort sends nothing", defaultRequest.effort === null, JSON.stringify(defaultRequest.effort));

await page.click("#effort-menu-chat");
await page.waitForTimeout(MENU_ANIMATION_MS);
const chatButton = await page.locator("#effort-menu-chat").boundingBox();
const chatMenu = await page.locator(".composer-menu").boundingBox();
check("effort menu opens right from its button in chat", boxesTouch(chatMenu, chatButton) && chatMenu.y < chatButton.y, JSON.stringify({ chatMenu, chatButton }));
await shot("51-effort-menu-chat");
await page.locator(".composer-menu-option", { hasText: "Высокий" }).click();
await page.waitForSelector(".composer-menu", { state: "detached" });
check("effort button shows the chosen level", (await page.locator("#effort-menu-chat").innerText()).includes("Высокий"));
await sendAndWait("вопрос с усилием");
check("chosen effort reaches the model", (await lastRequest()).effort === "high", JSON.stringify((await lastRequest()).effort));
await page.click("#effort-menu-chat");
await page.keyboard.press("ArrowUp");
await page.keyboard.press("ArrowUp");
await page.keyboard.press("Enter");
await page.waitForTimeout(150);
check("keyboard chooses the default effort", (await page.locator("#effort-menu-chat").innerText()).includes("Как у модели"), await page.locator("#effort-menu-chat").innerText());
await selectModel("Mock Fast Mock OpenAI");
check("effort button hides for a model without levels", (await page.locator("#effort-menu-chat").count()) === 0);

await sendAndWait("инструменты: read_file=a.txt");
const chatModeRequest = await lastRequest();
check("chat mode gives the model no tools", chatModeRequest.toolNames.length === 0, chatModeRequest.toolNames.join());
check("chat mode runs no tools", (await page.locator(".tool-group").count()) === 0);

const chats = await invoke("list_chats");
const chatId = chats[0].id;
await invoke("set_chat_access", { chatId, accessMode: "plan", folder: WORKSPACE });
await page.evaluate(() => window.location.reload());
await page.waitForSelector(".composer-input");
await page.keyboard.press("Control+k");
await page.waitForSelector(".palette");
await page.fill(".palette-input", "рабочий чат");
await page.waitForTimeout(500);
await page.keyboard.press("Enter");
await page.waitForSelector(".message-assistant");

check("access button shows the chat mode", (await page.locator("#access-menu-chat").innerText()).includes("План"));
check("folder is shown next to the mode", (await page.locator(".folder-button").innerText()).length > 0);
await page.click("#access-menu-chat");
await page.waitForTimeout(MENU_ANIMATION_MS);
const accessButton = await page.locator("#access-menu-chat").boundingBox();
const accessMenu = await page.locator(".composer-menu").boundingBox();
check("access menu opens right from its button", boxesTouch(accessMenu, accessButton) && accessMenu.y < accessButton.y, JSON.stringify({ accessMenu, accessButton }));
check("access menu has six modes with numbers", (await page.locator(".composer-menu-key").allInnerTexts()).join("") === "123456");
await shot("52-access-menu");
await page.keyboard.press("3");
await page.waitForSelector(".composer-menu", { state: "detached" });
await page.waitForTimeout(150);
check("number key switches the mode", (await invoke("list_chats"))[0].accessMode === "ask");
await chooseMode("План");

await sendAndWait("инструменты: read_file=a.txt");
const planRequest = await lastRequest();
check("plan gives only reading tools", planRequest.toolNames.join() === "read_file,list_files,search_text", planRequest.toolNames.join());
check("plan tells the model the rules and folder", planRequest.system.includes(WORKSPACE) && planRequest.system.includes("это данные"), planRequest.system);
check("plan reads a file without asking", (await lastAnswerText()).includes("содержимое файла a"), await lastAnswerText());
check("tool activity is shown in the answer", (await page.locator(".message-assistant").last().locator(".tool-summary").count()) === 1);

await sendAndWait("инструменты: write_file=plan.txt");
check("plan refuses writing", !existsSync(join(WORKSPACE, "plan.txt")) && (await lastAnswerText()).includes("режим доступа запрещает"), await lastAnswerText());

await sendAndWait(`инструменты: read_file=${join(OUTSIDE, "secret.txt")}`);
check("reading outside the folder is refused", (await lastAnswerText()).includes("вне рабочей папки") && !(await lastAnswerText()).includes("секрет снаружи"), await lastAnswerText());

await chooseMode("Спрашивать");
await send("инструменты: write_file=denied.txt");
await page.waitForSelector(".approval-card");
const askTools = (await lastRequest()).toolNames.join();
check("ask offers all six tools", askTools === "read_file,list_files,search_text,write_file,edit_file,run_command", askTools);
check("approval shows the file and its text, not raw data", (await page.locator(".approval-card .tool-detail").innerText()).startsWith("denied.txt\n"), await page.locator(".approval-card .tool-detail").innerText());
const cardBottom = await page.evaluate(() => document.querySelector(".approval-card").getBoundingClientRect().bottom);
const composerTop = await page.evaluate(() => document.querySelector(".composer-input").closest("form, .composer").getBoundingClientRect().top);
check("approval card is above the input field and fully visible", cardBottom <= composerTop, `${cardBottom} ${composerTop}`);
const question = await page.locator(".approval-question").innerText();
check("approval names the action and folder", question.includes("изменить файл") && question.includes(WORKSPACE), question);
await shot("53-approval-card");
await page.locator(".approval-card button", { hasText: "Запретить" }).click();
await waitForAnswerDone();
check("denied write creates no file", !existsSync(join(WORKSPACE, "denied.txt")));
check("model learns about the refusal", (await lastAnswerText()).includes("запретил"), await lastAnswerText());

await send("инструменты: write_file=sub/allowed.txt");
await page.waitForSelector(".approval-card");
await page.locator(".approval-card button", { hasText: /^Разрешить$/ }).click();
await waitForAnswerDone();
check("allowed write creates the file", existsSync(join(WORKSPACE, "sub", "allowed.txt")) && readFileSync(join(WORKSPACE, "sub", "allowed.txt"), "utf8") === "записано моделью");

await send(`инструменты: run_command=${encodeURIComponent("Write-Output привет-команда")}`);
await page.waitForSelector(".approval-card");
check("command approval shows the command", (await page.locator(".approval-card .tool-detail").innerText()).includes("Write-Output"));
await page.locator(".approval-card button", { hasText: "Разрешить в этом чате" }).click();
await waitForAnswerDone();
check("command output returns to the model", (await lastAnswerText()).includes("привет-команда"), await lastAnswerText());
await sendAndWait(`инструменты: run_command=${encodeURIComponent("Write-Output второй-раз")}`);
check("allow in this chat skips the next question", (await lastAnswerText()).includes("второй-раз"), await lastAnswerText());

await chooseMode("Правки без вопросов");
await sendAndWait("инструменты: write_file=edits.txt");
check("accept edits writes without asking", existsSync(join(WORKSPACE, "edits.txt")));
await send(`инструменты: run_command=${encodeURIComponent("Write-Output новый-вопрос")}`);
await page.waitForSelector(".approval-card");
check("changing the mode forgets chat permissions", true);
await page.locator(".approval-card button", { hasText: "Запретить" }).click();
await waitForAnswerDone();

await chooseMode("Авто");
await sendAndWait(`инструменты: run_command=${encodeURIComponent("Get-ChildItem")}`);
check("auto runs an everyday command itself", (await lastAnswerText()).includes("a.txt"), await lastAnswerText());
await send(`инструменты: run_command=${encodeURIComponent("Remove-Item a.txt")}`);
await page.waitForSelector(".approval-card");
check("auto asks before a risky command", true);
await page.click(".send-button.is-stop");
await waitForAnswerDone();
check("stop while waiting runs nothing", existsSync(join(WORKSPACE, "a.txt")));
check("stop leaves no approval card", (await page.locator(".approval-card").count()) === 0);

await send(`инструменты: run_command=${encodeURIComponent("Get-ChildItem;Start-Sleep 60")}`);
await page.waitForSelector(".approval-card");
await page.locator(".approval-card button", { hasText: /^Разрешить$/ }).click();
await page.waitForSelector(".tool-activity");
await shot("54-tool-running");
const stoppedAt = Date.now();
await page.waitForTimeout(600);
await page.click(".send-button.is-stop");
await waitForAnswerDone();
check("stop ends a running command quickly", Date.now() - stoppedAt < 5000, `${Date.now() - stoppedAt} мс`);

await chooseMode("Полный доступ");
check("full access button is marked as dangerous", await page.locator("#access-menu-chat.is-danger").count() === 1);
await sendAndWait(`инструменты: read_file=${join(OUTSIDE, "secret.txt")}`);
check("full access reads outside the folder", (await lastAnswerText()).includes("секрет снаружи"), await lastAnswerText());

await openSettingsTab("Доступ");
await shot("55-settings-access");
await page.uncheck("#access-toolsCommand");
await closeSettings();
await chooseMode("Спрашивать");
await sendAndWait("без инструментов сейчас");
check("turned off commands disappear from tools", !(await lastRequest()).toolNames.includes("run_command"), (await lastRequest()).toolNames.join());
await openSettingsTab("Доступ");
await page.check("#access-toolsCommand");
await closeSettings();

await sendAndWait("инструменты: read_file=a.txt");
const savedThread = await invoke("get_thread", { chatId, limit: 200, beforeId: null, ensureId: null });
const savedTool = savedThread.messages.at(-1).parts.at(-1);
check("tool calls are saved with the message", savedTool.name === "read_file" && savedTool.result.includes("содержимое") && savedTool.status === "done", JSON.stringify(savedTool));
await page.evaluate(() => window.location.reload());
await page.waitForSelector(".composer-input");
await page.keyboard.press("Control+k");
await page.waitForSelector(".palette");
await page.fill(".palette-input", "рабочий чат");
await page.waitForTimeout(500);
await page.keyboard.press("Enter");
await page.waitForSelector(".tool-summary");
check("saved tool calls show after reopening", (await page.locator(".message-assistant").last().locator(".tool-summary").count()) === 1);
check("reopening keeps the mode", (await page.locator("#access-menu-chat").innerText()).includes("Спрашивать"));

await selectModel("Mock No Tools Mock OpenAI");
await sendAndWait("обычный вопрос");
check("model without tools still answers", (await lastAnswerText()).includes("Отвечает"), await lastAnswerText());
check("model without tools explains why", (await lastAnswerText()).includes("не умеет пользоваться инструментами"), await lastAnswerText());
await selectModel("Mock Fast Mock OpenAI");

const allModels = await invoke("list_models");
const openAiProvider = allModels.find((model) => model.id === "mock-fast" && model.provider.startsWith("custom")).provider;
const compared = await invoke("send_message", {
  request: { chatId, parentId: null, text: "инструменты: read_file=a.txt", attachments: [], modelRefs: [`${openAiProvider}/mock-fast`, `${openAiProvider}/mock-smart`], accessMode: null, folder: null },
});
await page.waitForTimeout(1500);
const comparedThread = await invoke("get_thread", { chatId, limit: 5, beforeId: compared.assistantMessages[0].id, ensureId: null });
const comparedMessage = (await invoke("get_thread", { chatId, limit: 2, beforeId: null, ensureId: null })).messages.at(-1);
check("compare mode gives no computer access", comparedMessage.parts.length === 0 && (comparedMessage.note ?? "").includes("сравнении"), `${JSON.stringify(comparedMessage.note)} ${comparedThread.messages.length}`);
await page.evaluate(() => window.location.reload());
await page.waitForSelector(".composer-input");

await invoke("add_skill_folder", { folder: join(OUTSIDE, "skills") });
await openSettingsTab("Скиллы");
await page.waitForSelector("#skills-list .settings-item");
check("skill from own folder is listed", (await page.locator("#skills-list").innerText()).includes("demo"), await page.locator("#skills-list").innerText());
await shot("56-settings-skills");
await closeSettings();
await selectModel("Mock Fast Mock OpenAI");
await sendAndWait("вопрос в новом чате");
const skillListed = await lastRequest();
check("new chat starts without computer access", (await page.locator("#access-menu-chat").innerText()).includes("Только чат"));
check("enabled skill is offered through read_skill only", skillListed.toolNames.join() === "read_skill", skillListed.toolNames.join());
await sendAndWait("инструменты: read_skill=demo");
check("model can read a skill", (await lastAnswerText()).includes("Инструкция скилла demo"), await lastAnswerText());
await sendAndWait("/demo сделай это");
check("slash command puts the skill into instructions", ((await lastRequest()).system ?? "").includes("Инструкция скилла demo"));
await openSettingsTab("Скиллы");
await page.locator("#skills-list input.toggle").first().uncheck();
await page.waitForTimeout(250);
await closeSettings();
await sendAndWait("вопрос без скилла");
check("turned off skill is not offered", (await lastRequest()).toolNames.length === 0, (await lastRequest()).toolNames.join());

await openSettingsTab("Плагины");
await page.click("text=Подключить сервер MCP");
await page.fill("#mcp-name", "notes");
await page.fill("#mcp-target", MOCK_MCP_COMMAND);
await page.click(".settings-inline-form button[type=submit]");
await page.waitForSelector("#mcp-list .settings-item");
await page.locator("#mcp-list .link-button", { hasText: "Проверить" }).click();
await page.waitForFunction(() => document.querySelector("#mcp-list")?.innerText.includes("Работает"));
check("mcp server connects and lists tools", (await page.locator("#mcp-list").innerText()).includes("2 инструмента"), await page.locator("#mcp-list").innerText());
await page.locator("text=Каталог плагинов").click();
await page.waitForSelector("#plugin-list .settings-item");
const pluginCount = await page.locator("#plugin-list .settings-item").count();
check("plugin catalog comes from Claude Code folder", pluginCount > 10, String(pluginCount));
await page.fill("#plugin-filter", "champion-kit");
await page.waitForTimeout(150);
await shot("57-settings-plugins");
await page.locator("#plugin-list input.toggle").first().check();
await page.waitForTimeout(300);
await openSettingsTab("Скиллы");
await page.waitForSelector("#skills-list .settings-item");
check("enabled plugin brings its skills", (await page.locator("#skills-list").innerText()).includes("Плагин champion-kit"));
await openSettingsTab("Плагины");
await page.locator("text=Каталог плагинов").click();
await page.fill("#plugin-filter", "champion-kit");
await page.waitForTimeout(150);
await page.locator("#plugin-list input.toggle").first().uncheck();
await page.waitForTimeout(300);
await closeSettings();

await invoke("set_chat_access", { chatId, accessMode: "plan", folder: null });
await sendAndWait("новый вопрос до смены режима");
const newestChatId = (await invoke("list_chats"))[0].id;
await invoke("set_chat_access", { chatId: newestChatId, accessMode: "ask", folder: WORKSPACE });
await page.evaluate(() => window.location.reload());
await page.waitForSelector(".composer-input");
await page.keyboard.press("Control+k");
await page.waitForSelector(".palette");
await page.fill(".palette-input", "вопрос в новом чате");
await page.waitForTimeout(500);
await page.keyboard.press("Enter");
await page.waitForSelector(".message-assistant");
await sendAndWait("инструменты: mcp__notes__find_note=поиск");
const mcpRequest = await lastRequest();
check("mcp tools are offered to the model", mcpRequest.toolNames.includes("mcp__notes__find_note") && mcpRequest.toolNames.includes("mcp__notes__save_note"), mcpRequest.toolNames.join());
check("read-only mcp tool runs without asking", (await lastAnswerText()).includes("Сервер выполнил find_note: поиск"), await lastAnswerText());
await send("инструменты: mcp__notes__save_note=запись");
await page.waitForSelector(".approval-card");
check("changing mcp tool asks first", (await page.locator(".approval-question").innerText()).includes("mcp__notes__save_note"));
await page.locator(".approval-card button", { hasText: /^Разрешить$/ }).click();
await waitForAnswerDone();
check("allowed mcp tool runs", (await lastAnswerText()).includes("Сервер выполнил save_note: запись"), await lastAnswerText());
await chooseMode("План");
await sendAndWait("вопрос в плане с сервером");
const mcpPlanRequest = await lastRequest();
check("plan keeps only read-only mcp tools", mcpPlanRequest.toolNames.includes("mcp__notes__find_note") && !mcpPlanRequest.toolNames.includes("mcp__notes__save_note"), mcpPlanRequest.toolNames.join());

await page.click(".voice-button");
await page.waitForSelector(".settings");
check("microphone without a model opens voice settings", (await page.locator(".settings-page h2").innerText()) === "Голос");
check("voice download names the model size", (await page.locator(".settings-page .primary-button").innerText()).includes("396 МБ"), await page.locator(".settings-page .primary-button").innerText());
await shot("58-settings-voice");
await closeSettings();
mkdirSync(join(DATA_DIR, "voice", "models"), { recursive: true });
writeFileSync(join(DATA_DIR, "voice", "models", "ggml-parakeet-tdt-0.6b-v3-q4_k.bin"), "заглушка");

const composerInput = page.locator(".composer-input");
const caretPosition = () => composerInput.evaluate((input) => input.selectionStart);
const waitForFieldChange = async () => {
  const seen = await composerInput.inputValue();
  await page.waitForFunction((value) => document.querySelector(".composer-input")?.value !== value, seen);
};
rmSync(FAKE_SPEECH_LOG, { force: true });
await page.evaluate(() => {
  window.__playedNotes = 0;
  const createOscillator = AudioContext.prototype.createOscillator;
  AudioContext.prototype.createOscillator = function () {
    window.__playedNotes += 1;
    return createOscillator.call(this);
  };
});
const playedNotes = () => page.evaluate(() => window.__playedNotes);
const waveColumns = () => page.locator(".voice-wave").evaluate((canvas) => {
  const { width, height } = canvas;
  const pixels = canvas.getContext("2d").getImageData(0, 0, width, height).data;
  const tallAt = window.devicePixelRatio * 5;
  let tall = 0;
  let filled = 0;
  for (let x = 0; x < width; x += 1) {
    let columnHeight = 0;
    for (let y = 0; y < height; y += 1) {
      columnHeight += pixels[(y * width + x) * 4 + 3] > 40 ? 1 : 0;
    }
    filled += columnHeight > 0 ? 1 : 0;
    tall += columnHeight > tallAt ? 1 : 0;
  }
  return { tall, filled, signature: pixels.join().length + pixels.reduce((sum, value, index) => sum + value * (index % 97), 0) };
});
await composerInput.fill("Вот что я скажу:");
await page.click(".voice-button");
await page.waitForSelector(".voice-status");
check("turning the microphone on plays a sound", (await playedNotes()) === 2, String(await playedNotes()));
check("field stays visible while recording", await composerInput.isVisible());
check("field can be edited while recording", await composerInput.evaluate((input) => !input.readOnly));
check("field has focus while recording", await composerInput.evaluate((input) => document.activeElement === input));
check("caret starts at the end of the text", (await caretPosition()) === (await composerInput.inputValue()).length);
await page.waitForFunction(() => document.querySelector(".composer-input")?.value.includes("Черновик"));
check("spoken text appears in the field while recording", (await composerInput.inputValue()).startsWith("Вот что я скажу: Черновик"), await composerInput.inputValue());
check("live text is marked as not final", await composerInput.evaluate((input) => input.classList.contains("is-dictating")));
check("caret follows the end of the growing text", (await caretPosition()) === (await composerInput.inputValue()).length);
check("sending works while recording", await page.locator(".send-button").isEnabled());
const caretColor = await composerInput.evaluate((input) => getComputedStyle(input).caretColor);
const textColor = await page.locator("body").evaluate((body) => getComputedStyle(body).color);
check("caret has the main text color while recording", caretColor === textColor, `${caretColor} / ${textColor}`);
await page.waitForTimeout(2500);
const waveBefore = await waveColumns();
await page.waitForTimeout(400);
const waveAfter = await waveColumns();
check("volume strip draws loud moments as lines", waveAfter.tall > 0, JSON.stringify(waveAfter));
check("volume strip draws quiet moments as dots", waveAfter.filled > waveAfter.tall, JSON.stringify(waveAfter));
check("volume strip keeps moving", waveBefore.signature !== waveAfter.signature);
await page.screenshot({ path: join(SHOTS_DIR, "59-voice-recording.png"), caret: "initial" });

await page.keyboard.press("Control+Home");
await page.keyboard.type("Начало. ");
check("typing at the start while recording works", (await composerInput.inputValue()).startsWith("Начало. Вот что я скажу: Черновик"), await composerInput.inputValue());
await waitForFieldChange();
check("caret stays where the user put it when speech arrives", (await caretPosition()) === 8, String(await caretPosition()));
check("typed text stays when speech arrives", (await composerInput.inputValue()).startsWith("Начало. Вот что я скажу: Черновик"), await composerInput.inputValue());

await page.keyboard.press("Control+End");
await waitForFieldChange();
check("caret at the end keeps following the text", (await caretPosition()) === (await composerInput.inputValue()).length);

await composerInput.evaluate((input) => {
  const start = input.value.indexOf("Черновик");
  input.setSelectionRange(start, start + "Черновик".length);
});
await page.keyboard.press("Delete");
const textAfterDeleting = await composerInput.inputValue();
const frozenStart = textAfterDeleting.slice(0, textAfterDeleting.indexOf(".", 10) + 1);
check("selecting and deleting a spoken word works while recording", frozenStart.startsWith("Начало. Вот что я скажу:") && !frozenStart.includes("Черновик"), frozenStart);
await composerInput.evaluate((input) => input.setSelectionRange(input.value.length, input.value.length));
await page.waitForFunction(() => (document.querySelector(".composer-input")?.value.match(/Черновик/g) ?? []).length >= 3, null, { timeout: 30000 });
check("edited speech stays as the user left it", (await composerInput.inputValue()).startsWith(frozenStart), await composerInput.inputValue());
check("new speech goes after the edited text", (await composerInput.inputValue()).indexOf("Черновик") > frozenStart.length - 1, await composerInput.inputValue());
await shot("62-voice-editing");

await page.click(".voice-button");
check("caret stays in the field while the text is being refined", await composerInput.evaluate((input) => document.activeElement === input));
check("turning the microphone off plays a sound", (await playedNotes()) === 4, String(await playedNotes()));
await page.waitForSelector(".voice-status", { state: "detached", timeout: 30000 });
const finalText = await composerInput.inputValue();
check("microphone off replaces the speech with refined text", finalText.includes("Точный текст"), finalText);
check("refined text keeps everything the user typed", finalText.startsWith(frozenStart), finalText);
check("field is visible after recording", await composerInput.isVisible());
const fieldHeight = (await composerInput.boundingBox()).height;
check("field has height after recording", fieldHeight > 15, String(fieldHeight));
check("field looks normal after recording", await composerInput.evaluate((input) => !input.classList.contains("is-dictating")));
check("caret stays in the field after recording", await composerInput.evaluate((input) => document.activeElement === input));
await shot("61-voice-finished");

const recognitionRuns = readFileSync(FAKE_SPEECH_LOG, "utf8").trim().split("\n").map((line) => JSON.parse(line));
const runsDoNotOverlap = (runs) => runs.every((run, index) => index === 0 || run.startedAt >= runs[index - 1].endedAt - 50);
check("background refining ran while speech was shown", recognitionRuns.some((run) => !run.isLive) && recognitionRuns.some((run) => run.isLive));
check("fast passes run one at a time", runsDoNotOverlap(recognitionRuns.filter((run) => run.isLive)));
check("refining passes run one at a time", runsDoNotOverlap(recognitionRuns.filter((run) => !run.isLive)));

await composerInput.fill("");
await page.click(".voice-button");
await page.waitForFunction(() => document.querySelector(".composer-input")?.value.includes("Черновик"));
await page.keyboard.press("Enter");
await page.waitForFunction(() => document.querySelector(".composer-input")?.value === "");
await page.waitForFunction(() => document.querySelector(".voice-status") === null);
check("sending with the microphone on plays the off sound", (await playedNotes()) === 8, String(await playedNotes()));
check("sending with the microphone on stops recording", !(await page.locator(".voice-button").evaluate((button) => button.classList.contains("is-recording"))));
const sentText = await page.locator(".message-user").last().innerText();
check("sending with the microphone on sends the fast text", sentText.includes("Черновик") && !sentText.includes("Точный"), sentText);
await waitForAnswerDone();
await page.waitForTimeout(1200);
check("nothing arrives in the field after sending", (await composerInput.inputValue()) === "", await composerInput.inputValue());

await composerInput.fill("");

await page.evaluate(() => window.location.reload());
await page.waitForSelector(".composer-input");
await openSettingsTab("Доступ");
await page.check("#access-remember");
await closeSettings();
await page.keyboard.press("Control+k");
await page.waitForSelector(".palette");
await page.fill(".palette-input", "вопрос в новом чате");
await page.waitForTimeout(500);
await page.keyboard.press("Enter");
await page.waitForSelector(".message-assistant");
await chooseMode("Спрашивать");
await page.keyboard.press("Control+n");
await page.waitForTimeout(400);
check("new chat remembers the mode when asked to", (await page.locator("#access-menu-start").innerText()).includes("Спрашивать"), await page.locator("#access-menu-start").innerText());
await shot("60-start-with-mode");

check("no console errors", consoleErrors.length === 0, consoleErrors.slice(0, 5).join(" || "));

await browser.close();
console.log(failures.length === 0 ? "ALL PASSED" : `FAILED: ${failures.join(", ")}`);
process.exit(failures.length === 0 ? 0 : 1);
