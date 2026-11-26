import { appendFileSync } from "node:fs";

const HEADER_BYTES = 13;
const RECOGNITION_DELAY_MS = 300;
const LOG_PATH = process.env.TARN_FAKE_SPEECH_LOG;

let waitingBytes = Buffer.alloc(0);

function recognize(id, isLive, audioBytes) {
  const startedAt = Date.now();
  setTimeout(() => {
    const text = `${isLive ? "Черновик" : "Точный текст"} ${audioBytes + 44}.`;
    if (LOG_PATH) {
      appendFileSync(LOG_PATH, `${JSON.stringify({ isLive, audioBytes, startedAt, endedAt: Date.now() })}\n`);
    }
    process.stdout.write(`${JSON.stringify({ id, text, error: null })}\n`);
  }, RECOGNITION_DELAY_MS);
}

function handleWaitingRequests() {
  while (waitingBytes.length >= HEADER_BYTES) {
    const audioBytes = waitingBytes.readUInt32LE(9);
    if (waitingBytes.length < HEADER_BYTES + audioBytes) {
      return;
    }
    recognize(Number(waitingBytes.readBigUInt64LE(0)), waitingBytes[8] !== 0, audioBytes);
    waitingBytes = waitingBytes.subarray(HEADER_BYTES + audioBytes);
  }
}

process.stdin.on("data", (chunk) => {
  waitingBytes = Buffer.concat([waitingBytes, chunk]);
  handleWaitingRequests();
});
process.stdin.on("end", () => process.exit(0));
