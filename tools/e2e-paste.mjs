import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { chromium } from "playwright-core";

const SHOTS_DIR = process.env.TARN_SHOTS_DIR ?? "e2e-shots";
const DATA_DIR = process.env.TARN_DATA_DIR;
const SET_CLIPBOARD_SCRIPT = process.env.TARN_SET_CLIPBOARD_SCRIPT;

const failures = [];

function check(name, condition, detail = "") {
  console.log(`${condition ? "ok  " : "FAIL"} ${name}${detail ? ` | ${detail}` : ""}`);
  if (!condition) {
    failures.push(name);
  }
}

function setClipboard(kind) {
  execFileSync("powershell", ["-NoProfile", "-STA", "-ExecutionPolicy", "Bypass", "-File", SET_CLIPBOARD_SCRIPT, kind], { stdio: "pipe" });
}

mkdirSync(SHOTS_DIR, { recursive: true });
const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
const page = browser.contexts()[0].pages()[0];
const consoleErrors = [];
page.on("console", (message) => {
  if (message.type() === "error") {
    consoleErrors.push(message.text());
  }
});
page.on("pageerror", (error) => consoleErrors.push(String(error)));

await page.waitForSelector(".composer-input");
const composerInput = page.locator(".composer-input");
const previews = page.locator(".attachment-preview");

await composerInput.focus();
setClipboard("image");
await page.keyboard.press("Control+V");
await page.waitForSelector(".attachment-preview", { timeout: 8000 });
check("an image from the real clipboard is attached", (await previews.count()) === 1);
const previewSource = await page.locator(".attachment-preview-image").first().getAttribute("src");
check("the attached image can be shown", previewSource.startsWith("data:image/"), previewSource.slice(0, 30));
check("pasting an image adds no text", (await composerInput.inputValue()) === "");
await page.screenshot({ path: join(SHOTS_DIR, "80-pasted-image.png") });

setClipboard("text");
await composerInput.focus();
await page.keyboard.press("Control+V");
await page.waitForFunction(() => document.querySelector(".composer-input")?.value.includes("обычный текст"));
check("text from the real clipboard stays text", (await previews.count()) === 1, await composerInput.inputValue());
await composerInput.fill("");

mkdirSync(join(DATA_DIR, "voice", "models"), { recursive: true });
writeFileSync(join(DATA_DIR, "voice", "models", "ggml-parakeet-tdt-0.6b-v3-q4_k.bin"), "заглушка");
await page.click(".voice-button");
await page.waitForSelector(".voice-status");
await page.waitForFunction(() => document.querySelector(".composer-input")?.value.includes("Черновик"), null, { timeout: 15000 });
setClipboard("image");
await page.keyboard.press("Control+V");
await page.waitForFunction(() => document.querySelectorAll(".attachment-preview").length === 2, null, { timeout: 8000 });
check("an image can be pasted while recording", (await previews.count()) === 2);
const recordingText = await composerInput.inputValue();
await page.waitForFunction((seen) => document.querySelector(".composer-input")?.value !== seen, recordingText, { timeout: 15000 });
check("recording goes on after pasting an image", (await page.locator(".voice-status").count()) === 1);
await page.screenshot({ path: join(SHOTS_DIR, "81-pasted-while-recording.png") });
await page.click(".voice-button");
await page.waitForSelector(".voice-status", { state: "detached", timeout: 20000 });
check("no console errors", consoleErrors.length === 0, consoleErrors.slice(0, 5).join(" || "));

await browser.close();
console.log(failures.length === 0 ? "ALL PASSED" : `FAILED: ${failures.join(", ")}`);
process.exit(failures.length === 0 ? 0 : 1);
