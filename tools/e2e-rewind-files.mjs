import { execFile } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const SHOTS_DIR = process.env.TARN_SHOTS_DIR ?? "e2e-shots";
const MOCK_URL = process.env.TARN_MOCK_URL ?? "http://127.0.0.1:18765/v1";
const WORKSPACE = process.env.TARN_WORKSPACE;
const STEP_TIMEOUT_MS = 20000;
const REWIND_HERE = '[aria-label="Откатить сюда"]';
const PRESS_MENU_SCRIPT = join(dirname(fileURLToPath(import.meta.url)), "press-native-menu.ps1");

const MENU_CHAT_AND_FILES = 1;
const MENU_ONLY_CHAT = 2;
const RIGHT_CLICK_REWIND_ON_QUESTION = 3;

const EXISTING_FILE = join(WORKSPACE, "rewind-existing.txt");
const CREATED_FOLDER = join(WORKSPACE, "rewind-new");
const CREATED_FILE = join(CREATED_FOLDER, "created.txt");

mkdirSync(SHOTS_DIR, { recursive: true });
rmSync(CREATED_FOLDER, { recursive: true, force: true });
writeFileSync(EXISTING_FILE, "исходный");

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
const existingText = () => readFileSync(EXISTING_FILE, "utf8");
const lastToastText = () => page.locator(".toast").last().innerText();

function chooseInNativeMenu(itemNumber) {
  return new Promise((resolve, reject) => {
    execFile("powershell", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", PRESS_MENU_SCRIPT, "-ItemNumber", String(itemNumber)], (error) => {
      if (error) {
        reject(new Error(`системное меню не появилось: ${error.message}`));
      } else {
        resolve();
      }
    });
  });
}

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

async function clickRewind(messageIndex) {
  const message = messages.nth(messageIndex);
  await message.hover();
  await message.locator(REWIND_HERE).click();
}

async function rewindWithMenu(messageIndex, itemNumber) {
  await clickRewind(messageIndex);
  await chooseInNativeMenu(itemNumber);
  await page.waitForSelector(".toast-action");
  await page.waitForTimeout(150);
}

async function undoRewind(expectedCount) {
  await page.locator(".toast-action").last().click();
  await waitForMessageCount(expectedCount);
  await page.waitForTimeout(200);
}

await page.waitForSelector(".composer-input");
await page.keyboard.press("Control+,");
await page.waitForSelector(".settings");
await page.click("text=Подключить свой адрес API");
await page.fill("#custom-name", "Mock OpenAI");
await page.fill("#custom-url", MOCK_URL);
await page.click(".custom-form button[type=submit]");
await page.waitForSelector(".custom-form", { state: "detached" });
await page.keyboard.press("Escape");
await page.waitForSelector(".settings", { state: "detached" });

await page.click(".model-button");
await page.fill("#model-search", "Mock Fast");
await page.keyboard.press("Enter");
await page.waitForSelector(".model-menu", { state: "detached" });

await sendAndWait("начало проверки отката");
const chatId = (await invoke("list_chats"))[0].id;
await invoke("set_chat_access", { chatId, accessMode: "acceptEdits", folder: WORKSPACE });
await page.evaluate(() => window.location.reload());
await page.waitForSelector(".composer-input");
await page.keyboard.press("Control+k");
await page.waitForSelector(".palette");
await page.fill(".palette-input", "начало проверки");
await page.waitForTimeout(500);
await page.keyboard.press("Enter");
await waitForMessageCount(2);

await sendAndWait("инструменты: write_file=rewind-existing.txt=первая");
check("model changed the existing file", existingText() === "первая", existingText());
await sendAndWait("инструменты: edit_file=rewind-existing.txt=первая=вторая,write_file=rewind-new/created.txt=новый");
check("model edited the file and created another", existingText() === "вторая" && existsSync(CREATED_FILE), existingText());
await waitForMessageCount(6);
const thread = (await invoke("get_thread", { chatId, limit: 50, beforeId: null, ensureId: null })).messages;
const questionTwo = thread[2];
const questionThree = thread[4];
const answerThree = thread[5];

const previewOfLastQuestion = await invoke("rewind_preview", { messageId: questionThree.id });
check("preview counts files of the last answer", previewOfLastQuestion.changedFiles === 2, JSON.stringify(previewOfLastQuestion));
const previewOfEarlierQuestion = await invoke("rewind_preview", { messageId: questionTwo.id });
check("preview does not count one file twice", previewOfEarlierQuestion.changedFiles === 2, JSON.stringify(previewOfEarlierQuestion));
const previewOfLastAnswer = await invoke("rewind_preview", { messageId: answerThree.id });
check("nothing to return after the last answer", previewOfLastAnswer.changedFiles === 0, JSON.stringify(previewOfLastAnswer));

await rewindWithMenu(4, MENU_CHAT_AND_FILES);
await page.screenshot({ path: join(SHOTS_DIR, "95-rewind-with-files.png") });
check("menu item returns the chat", (await messages.count()) === 4, String(await messages.count()));
check("menu item returns the edited file", existingText() === "первая", existingText());
check("menu item removes the created file", !existsSync(CREATED_FILE));
check("toast says files came back", (await lastToastText()).startsWith("Чат и файлы откачены"), await lastToastText());
check("question is back in the field", (await composerInput.inputValue()).includes("edit_file"), await composerInput.inputValue());

await undoRewind(6);
check("undo returns the model text", existingText() === "вторая", existingText());
check("undo returns the created file", existsSync(CREATED_FILE) && readFileSync(CREATED_FILE, "utf8") === "новый");

await rewindWithMenu(4, MENU_ONLY_CHAT);
check("only chat returns the chat", (await messages.count()) === 4, String(await messages.count()));
check("only chat leaves the files", existingText() === "вторая" && existsSync(CREATED_FILE), existingText());
check("toast says only the chat", (await lastToastText()).startsWith("Чат откачен"), await lastToastText());
await undoRewind(6);
check("undo after only chat leaves the files", existingText() === "вторая" && existsSync(CREATED_FILE), existingText());

writeFileSync(EXISTING_FILE, "моя правка");
await rewindWithMenu(4, MENU_CHAT_AND_FILES);
check("file edited by the user stays", existingText() === "моя правка", existingText());
check("other file still comes back", !existsSync(CREATED_FILE));
check("toast names the file that stayed", (await lastToastText()).includes("Не возвращены: rewind-existing.txt"), await lastToastText());
await undoRewind(6);
check("undo leaves the edited file too", existingText() === "моя правка", existingText());
check("undo returns the other file", existsSync(CREATED_FILE));
writeFileSync(EXISTING_FILE, "вторая");

await messages.nth(4).click({ button: "right", position: { x: 20, y: 10 } });
await chooseInNativeMenu(RIGHT_CLICK_REWIND_ON_QUESTION);
await chooseInNativeMenu(MENU_CHAT_AND_FILES);
await page.waitForSelector(".toast-action");
await page.waitForTimeout(150);
check("second menu after right click works", existingText() === "первая" && (await messages.count()) === 4, existingText());
await undoRewind(6);

await composerInput.fill(`инструменты: run_command=${encodeURIComponent("Write-Output команда")}`);
await composerInput.press("Enter");
await page.waitForSelector(".approval-card");
await page.locator(".approval-card button", { hasText: /^Разрешить$/ }).click();
await waitForAnswerDone();
await waitForMessageCount(8);
await clickRewind(6);
await page.waitForSelector(".toast-action");
await page.waitForTimeout(150);
check("rewind without files opens no menu and warns about commands", (await lastToastText()).startsWith("Чат откачен. Результат команд остался"), await lastToastText());
await page.screenshot({ path: join(SHOTS_DIR, "96-rewind-command.png") });

check("no console errors", consoleErrors.length === 0, consoleErrors.slice(0, 5).join(" || "));

await browser.close();
console.log(failures.length === 0 ? "ALL PASSED" : `FAILED: ${failures.join(", ")}`);
process.exit(failures.length === 0 ? 0 : 1);
