import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { chromium } from "playwright-core";

const SHOTS_DIR = process.env.TARN_SHOTS_DIR ?? "e2e-shots";
const MOCK_URL = process.env.TARN_MOCK_URL ?? "http://127.0.0.1:18765/v1";
const STEP_TIMEOUT_MS = 20000;
const FORK_FROM_HERE = '[aria-label="Форк отсюда"]';
const REWIND_HERE = '[aria-label="Откатить сюда"]';

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

const invoke = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args), { command, args });
const composerInput = page.locator(".composer-input");
const messages = page.locator(".message");
const currentTitle = () => page.locator(".chat-title").innerText();

async function waitForAnswerDone() {
  await page.waitForFunction(() => document.querySelector(".send-button.is-stop") === null, null, { timeout: 40000 });
  await page.waitForTimeout(200);
}

async function sendAndWait(text) {
  await composerInput.fill(text);
  await composerInput.press("Enter");
  await page.waitForTimeout(150);
  await waitForAnswerDone();
}

async function waitForMessageCount(count) {
  await page.waitForFunction((expected) => document.querySelectorAll(".message").length === expected, count);
}

async function clickOnMessage(messageIndex, selector) {
  const message = messages.nth(messageIndex);
  await message.hover();
  await message.locator(selector).click();
}

async function clickToastAction() {
  await page.locator(".toast-action").last().click();
}

await page.waitForSelector(".composer-input");
await page.keyboard.press("Control+,");
await page.waitForSelector(".settings");
await page.click("text=Подключить свой адрес API");
await page.fill("#custom-name", "Mock OpenAI");
await page.fill("#custom-url", MOCK_URL);
await page.click(".custom-form button[type=submit]");
await page.waitForSelector(".custom-form", { state: "detached" });
for (const tab of ["Доступ", "Скиллы", "Плагины", "Голос", "Импорт", "Расходы", "Журнал", "Общие"]) {
  await page.locator(".settings-tab", { hasText: tab }).click();
  await page.waitForTimeout(100);
  check(`${tab}: no explanation paragraph`, (await page.locator(".settings-lead").count()) === 0);
}
await page.locator(".settings-tab", { hasText: "Голос" }).click();
await page.waitForTimeout(250);
await page.screenshot({ path: join(SHOTS_DIR, "90-voice-settings-clean.png") });
await page.keyboard.press("Escape");
await page.waitForSelector(".settings", { state: "detached" });

await page.click(".model-button");
await page.fill("#model-search", "Mock Fast");
await page.keyboard.press("Enter");
await page.waitForSelector(".model-menu", { state: "detached" });

await sendAndWait("первый вопрос");
await sendAndWait("второй вопрос");
await waitForMessageCount(4);

const times = await page.locator(".message-time").allInnerTexts();
check("every message shows its time", times.length === 4 && times.every((time) => /^\d\d:\d\d$/.test(time)), times.join());
await page.mouse.move(2, 2);
await page.waitForTimeout(250);
const firstTimeOpacity = await page.locator(".message-time").first().evaluate((element) => getComputedStyle(element).opacity);
check("time is visible without hovering", firstTimeOpacity === "1", firstTimeOpacity);
const firstButtonOpacity = await messages.first().locator(".action-button").first().evaluate((element) => getComputedStyle(element).opacity);
check("message buttons stay hidden without hovering", firstButtonOpacity === "0", firstButtonOpacity);
await page.screenshot({ path: join(SHOTS_DIR, "91-message-times.png") });
await messages.nth(1).hover();
await page.waitForTimeout(250);
await page.screenshot({ path: join(SHOTS_DIR, "92-message-actions.png") });

const chatsBefore = await invoke("list_chats");
const original = chatsBefore[0];

await page.click('[aria-label="Форк чата"]');
await page.waitForFunction((title) => document.querySelector(".chat-title")?.textContent === title, `Форк: ${original.title}`);
await waitForMessageCount(4);
check("fork of a chat opens a copy of it", (await invoke("list_chats")).length === chatsBefore.length + 1);
const forkTimes = await page.locator(".message-time").allInnerTexts();
check("fork keeps the time of messages", forkTimes.join() === times.join(), forkTimes.join());
await sendAndWait("вопрос только в форке");
await waitForMessageCount(6);
const originalThread = await invoke("get_thread", { chatId: original.id, limit: 50, beforeId: null, ensureId: null });
check("writing in a fork does not touch the original", originalThread.messages.length === 4, String(originalThread.messages.length));

async function openOriginalChat() {
  await page.keyboard.press("Control+b");
  await page.waitForTimeout(250);
  await page.locator(".chat-row").filter({ hasNotText: "Форк:" }).first().click();
  await waitForMessageCount(4);
  await page.keyboard.press("Control+b");
  await page.waitForTimeout(250);
}

await openOriginalChat();
await clickOnMessage(1, FORK_FROM_HERE);
await page.waitForFunction(() => document.querySelector(".chat-title")?.textContent?.startsWith("Форк:"));
await waitForMessageCount(2);
check("fork from an answer ends on that answer", (await messages.last().innerText()).length > 0);
check("fork from an answer leaves the field empty", (await composerInput.inputValue()) === "");

await openOriginalChat();
await clickOnMessage(2, FORK_FROM_HERE);
await page.waitForFunction(() => document.querySelector(".composer-input")?.value === "второй вопрос");
await waitForMessageCount(2);
check("fork from a question ends before it and puts it into the field", (await currentTitle()).startsWith("Форк:"));
await composerInput.fill("");

await openOriginalChat();
await clickOnMessage(1, REWIND_HERE);
await waitForMessageCount(2);
check("rewind to an answer hides what came after it", (await composerInput.inputValue()) === "");
await page.screenshot({ path: join(SHOTS_DIR, "93-rewound.png") });
await clickToastAction();
await waitForMessageCount(4);
check("rewind can be undone", (await messages.last().locator(".message-time").count()) === 1);

await clickOnMessage(2, REWIND_HERE);
await waitForMessageCount(2);
check("rewind to a question puts it back into the field", (await composerInput.inputValue()) === "второй вопрос", await composerInput.inputValue());
await clickToastAction();
await waitForMessageCount(4);
await page.waitForFunction(() => document.querySelector(".composer-input")?.value === "");
check("undo takes the question back out of the field", true);

await clickOnMessage(2, REWIND_HERE);
await waitForMessageCount(2);
await sendAndWait("другой второй вопрос");
await waitForMessageCount(4);
check("sending after a rewind makes a branch", (await messages.nth(2).locator(".branch-switcher").innerText()).includes("2/2"));
await messages.nth(2).hover();
await messages.nth(2).locator('[aria-label="Предыдущий вариант"]').click();
await page.waitForFunction(() => document.querySelectorAll(".message")[2]?.innerText.includes("второй вопрос") && !document.querySelectorAll(".message")[2]?.innerText.includes("другой"));
check("old messages are still there after a rewind", true);

await clickOnMessage(0, REWIND_HERE);
await waitForMessageCount(0);
check("rewind to the first question empties the chat", (await composerInput.inputValue()) === "первый вопрос", await composerInput.inputValue());
await page.screenshot({ path: join(SHOTS_DIR, "94-rewound-to-start.png") });
await sendAndWait("новое начало");
await waitForMessageCount(2);
check("first question after a rewind offers the old start", (await messages.first().locator(".branch-switcher").innerText()).includes("2/2"));

check("no console errors", consoleErrors.length === 0, consoleErrors.slice(0, 5).join(" || "));
await browser.close();
console.log(failures.length === 0 ? "ALL PASSED" : `FAILED: ${failures.join(", ")}`);
process.exit(failures.length === 0 ? 0 : 1);
