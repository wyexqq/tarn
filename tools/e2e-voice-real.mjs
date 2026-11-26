import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { chromium } from "playwright-core";

const SHOTS_DIR = process.env.TARN_SHOTS_DIR ?? "e2e-shots";
const INSTALL_TIMEOUT_MS = 480000;
const SPEECH_DURATION_MS = 43000;
const POLL_INTERVAL_MS = 150;
const FINAL_TIMEOUT_MS = 60000;
const MAX_FIRST_TEXT_MS = 2000;
const MAX_QUIET_GAP_MS = 2000;
const MAX_FINAL_DELAY_MS = 10000;
const TYPED_TEXT = "Моя вставка. ";
const TYPING_MOMENT_MS = 18000;

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

const consoleErrors = [];
page.on("console", (message) => {
  if (message.type() === "error") {
    consoleErrors.push(message.text());
  }
});
page.on("pageerror", (error) => consoleErrors.push(String(error)));

await page.waitForSelector(".composer-input");
const composerInput = page.locator(".composer-input");

await page.click(".voice-button");
await page.waitForSelector(".settings");
check("microphone without a model opens voice settings", (await page.locator(".settings-page h2").innerText()) === "Голос");
const installStartedAt = Date.now();
await page.locator(".settings-page .primary-button").click();
await page.waitForSelector(".progress-block", { timeout: 30000 });
await page.waitForTimeout(800);
await page.screenshot({ path: join(SHOTS_DIR, "70-voice-downloading.png") });
await page.waitForFunction(() => document.querySelector(".settings-page")?.innerText.includes("Готово"), null, { timeout: INSTALL_TIMEOUT_MS });
check("real download and unpacking finish from the button", true, `${Math.round((Date.now() - installStartedAt) / 1000)} с`);
await page.screenshot({ path: join(SHOTS_DIR, "71-voice-installed.png") });
await page.keyboard.press("Escape");
await page.waitForSelector(".settings", { state: "detached" });

await page.click(".voice-button");
await page.waitForSelector(".voice-status");
const recordingStartedAt = Date.now();
const changes = [];
let lastValue = "";
let hasTyped = false;
while (Date.now() - recordingStartedAt < SPEECH_DURATION_MS) {
  if (!hasTyped && Date.now() - recordingStartedAt > TYPING_MOMENT_MS) {
    hasTyped = true;
    await page.keyboard.press("Control+Home");
    await page.keyboard.type(TYPED_TEXT);
    await page.screenshot({ path: join(SHOTS_DIR, "72-voice-real-live.png") });
    await page.keyboard.press("Control+End");
  }
  const value = await composerInput.inputValue();
  if (value !== lastValue) {
    lastValue = value;
    changes.push({ at: Date.now() - recordingStartedAt, length: value.length, tail: value.slice(-60) });
  }
  await page.waitForTimeout(POLL_INTERVAL_MS);
}
for (const change of changes) {
  console.log(`  ${(change.at / 1000).toFixed(1)} с: ${change.length} зн. …${change.tail}`);
}

const liveText = await composerInput.inputValue();
const stoppedAt = Date.now();
await page.click(".voice-button");
await page.waitForSelector(".voice-status", { state: "detached", timeout: FINAL_TIMEOUT_MS });
const finalDelay = Date.now() - stoppedAt;
const finalText = await composerInput.inputValue();
await page.screenshot({ path: join(SHOTS_DIR, "73-voice-real-final.png") });

const gaps = changes.slice(1).map((change, index) => change.at - changes[index].at);
check("first words appear quickly", changes.length > 0 && changes[0].at < MAX_FIRST_TEXT_MS, `${changes[0]?.at} мс`);
check("live text keeps coming without long pauses", gaps.every((gap) => gap < MAX_QUIET_GAP_MS), `обновлений ${changes.length}, самая долгая пауза ${Math.max(0, ...gaps)} мс`);
check("live text reached the end of the speech", /модел/.test(liveText), liveText);
check("final text keeps what was typed during recording", finalText.startsWith(TYPED_TEXT), finalText);
check("final text has the end of the speech", /модел/.test(finalText), finalText);
check("final text is not shorter than the speech", finalText.length >= 400, String(finalText.length));
check("final pass is quick after stopping", finalDelay < MAX_FINAL_DELAY_MS, `${finalDelay} мс`);
check("field is visible with height after recording", (await composerInput.boundingBox()).height > 40, String((await composerInput.boundingBox()).height));
check("no console errors", consoleErrors.length === 0, consoleErrors.slice(0, 5).join(" || "));
console.log(`черновик: ${liveText}`);
console.log(`итог: ${finalText}`);

await browser.close();
console.log(failures.length === 0 ? "ALL PASSED" : `FAILED: ${failures.join(", ")}`);
process.exit(failures.length === 0 ? 0 : 1);
