import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { chromium } from "playwright-core";

const SHOTS_DIR = process.env.TARN_SHOTS_DIR ?? "e2e-shots";
const SEARCH_TEXT = process.env.TARN_SEARCH_TEXT ?? "где ты увидел тут settings";
const POLL_INTERVAL_MS = 3000;
const MAX_IMPORT_MS = 40 * 60 * 1000;

mkdirSync(SHOTS_DIR, { recursive: true });

const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
const page = browser.contexts()[0].pages()[0];
page.setDefaultTimeout(30000);

const invokeInPage = (command, args = {}) =>
  page.evaluate(({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args), { command, args });

async function importIsRunning() {
  return page.evaluate(() =>
    window.__TAURI_INTERNALS__.invoke("start_import", { sources: [] }).then(
      () => false,
      (error) => String(error).includes("уже идёт"),
    ),
  );
}

const sources = await invokeInPage("scan_import_sources");
const claudeCode = sources.find((source) => source.source === "claude_code");
console.log(`файлов ${claudeCode.fileCount}, ${Math.round(claudeCode.totalBytes / 1024 / 1024)} МБ`);

const startedAt = Date.now();
await invokeInPage("start_import", { sources: [{ source: "claude_code", path: claudeCode.path }] });
await page.waitForTimeout(POLL_INTERVAL_MS);
while ((await importIsRunning()) && Date.now() - startedAt < MAX_IMPORT_MS) {
  await page.waitForTimeout(POLL_INTERVAL_MS);
}
console.log(`импорт занял ${Math.round((Date.now() - startedAt) / 1000)} с`);
const chats = await invokeInPage("list_chats");
console.log(`чатов ${chats.length}`);

const hits = await invokeInPage("search", { query: SEARCH_TEXT });
console.log(`найдено по тексту: ${hits.length}`);
const hit = hits.find((item) => item.messageId) ?? hits[0];
if (!hit) {
  console.log("чат не найден");
  await browser.close();
  process.exit(1);
}

const thread = await invokeInPage("get_thread", { chatId: hit.chatId, limit: 60, beforeId: null, ensureId: hit.messageId });
const target = thread.messages.find((message) => message.id === hit.messageId);
console.log(`в сообщении текста: ${JSON.stringify(target.content.slice(0, 60))}, картинок: ${target.attachments.length}`);

const loadStartedAt = Date.now();
const firstPicture = await invokeInPage("read_image", { imageId: target.attachments[0].imageId });
console.log(`первая картинка ${Math.round(firstPicture.length / 1024)} КБ, загружена за ${Date.now() - loadStartedAt} мс`);

await page.keyboard.press("Control+k");
await page.waitForSelector(".palette");
await page.fill(".palette-input", SEARCH_TEXT);
await page.waitForTimeout(600);
const openStartedAt = Date.now();
await page.keyboard.press("Enter");
await page.waitForSelector(".message-image");
console.log(`чат с картинками открылся за ${Date.now() - openStartedAt} мс`);
await page.waitForTimeout(1500);
await page.screenshot({ path: join(SHOTS_DIR, "40-real-chat-with-images.png") });
await browser.close();
