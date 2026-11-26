import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { chromium } from "playwright-core";

const DATA_DIR = process.env.TARN_DATA_DIR;
const SPEECH_SECONDS = 20;
const WORDS_SILENCE_LIMIT_SECONDS = 30;
const ALLOWED_DELAY_SECONDS = 8;

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
mkdirSync(join(DATA_DIR, "voice", "models"), { recursive: true });
writeFileSync(join(DATA_DIR, "voice", "models", "ggml-parakeet-tdt-0.6b-v3-q4_k.bin"), "заглушка");

await page.evaluate(() => {
  window.__playedNotes = 0;
  const createOscillator = AudioContext.prototype.createOscillator;
  AudioContext.prototype.createOscillator = function () {
    window.__playedNotes += 1;
    return createOscillator.call(this);
  };
});

const startedAt = Date.now();
await page.click(".voice-button");
await page.waitForSelector(".voice-status");
await page.waitForSelector(".voice-status", { state: "detached", timeout: (SPEECH_SECONDS + WORDS_SILENCE_LIMIT_SECONDS + ALLOWED_DELAY_SECONDS + 10) * 1000 });
const stoppedAfter = (Date.now() - startedAt) / 1000;
check(
  "microphone turns off 30 seconds after the last words",
  stoppedAfter >= WORDS_SILENCE_LIMIT_SECONDS + SPEECH_SECONDS - ALLOWED_DELAY_SECONDS && stoppedAfter <= WORDS_SILENCE_LIMIT_SECONDS + SPEECH_SECONDS + ALLOWED_DELAY_SECONDS,
  `${stoppedAfter.toFixed(1)} с`,
);
const text = await page.locator(".composer-input").inputValue();
check("speech before the silence stays in the field", text.length > 0, text);
check("automatic stop plays the sound too", (await page.evaluate(() => window.__playedNotes)) === 4, String(await page.evaluate(() => window.__playedNotes)));
check("no console errors", consoleErrors.length === 0, consoleErrors.slice(0, 5).join(" || "));

await browser.close();
console.log(failures.length === 0 ? "ALL PASSED" : `FAILED: ${failures.join(", ")}`);
process.exit(failures.length === 0 ? 0 : 1);
