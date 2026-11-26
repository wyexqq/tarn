import assert from "node:assert/strict";
import { test } from "node:test";

import { VoiceNoiseTracker } from "../src/voiceNoise.ts";

const LEVELS_PER_SECOND = 16;

function feed(tracker, levels) {
  const results = [];
  for (const level of levels) {
    tracker.add(level);
    const share = tracker.stripShareOfLastLevel();
    results.push({ share, onStrip: share > 0, forRecognition: tracker.mayBeVoiceForRecognition(level) });
  }
  return results;
}

function roomNoise(seconds, average) {
  return Array.from({ length: seconds * LEVELS_PER_SECOND }, (_, index) => average * (1 + 0.2 * Math.sin(index * 1.7)));
}

function speechWithGaps(seconds, loudness, gapLevel) {
  return Array.from({ length: seconds * LEVELS_PER_SECOND }, (_, index) => (index % 5 === 4 ? gapLevel : loudness * (1 + 0.3 * Math.sin(index))));
}

function shortWords(count, loudness, noise) {
  const levels = [];
  for (let word = 0; word < count; word += 1) {
    levels.push(loudness, loudness * 0.6, ...new Array(10).fill(noise));
  }
  return levels;
}

test("a quiet microphone without automatic gain still counts as speech", () => {
  const tracker = new VoiceNoiseTracker();
  feed(tracker, roomNoise(3, 0.004));
  const results = feed(tracker, [0.03, 0.05, 0.04]);
  assert.ok(results.every((result) => result.onStrip && result.forRecognition));
});

test("room noise above the old threshold is not drawn as speech", () => {
  const results = feed(new VoiceNoiseTracker(), roomNoise(10, 0.15));
  assert.equal(results.filter((result) => result.onStrip).length, 0);
});

test("loud noise raised by automatic gain is not drawn as speech", () => {
  const results = feed(new VoiceNoiseTracker(), roomNoise(10, 0.9));
  assert.equal(results.filter((result) => result.onStrip).length, 0);
});

test("room noise is not sent to recognition once the noise is known", () => {
  const results = feed(new VoiceNoiseTracker(), roomNoise(10, 0.15)).slice(LEVELS_PER_SECOND * 2);
  assert.equal(results.filter((result) => result.forRecognition).length, 0);
});

test("speech over room noise is drawn as lines", () => {
  const tracker = new VoiceNoiseTracker();
  feed(tracker, roomNoise(3, 0.1));
  const results = feed(tracker, speechWithGaps(4, 0.5, 0.1));
  const spokenFrames = results.filter((_, index) => index % 5 !== 4);
  assert.ok(spokenFrames.every((result) => result.onStrip));
  assert.ok(spokenFrames.every((result) => result.forRecognition));
});

test("speech only twice as loud as the noise is still drawn", () => {
  const tracker = new VoiceNoiseTracker();
  feed(tracker, roomNoise(3, 0.5));
  const results = feed(tracker, [1, 1, 1, 0.8]);
  assert.ok(results.every((result) => result.onStrip));
});

test("every short word over noise is drawn", () => {
  const tracker = new VoiceNoiseTracker();
  feed(tracker, roomNoise(3, 0.3));
  const results = feed(tracker, shortWords(6, 0.9, 0.3));
  for (let word = 0; word < 6; word += 1) {
    assert.ok(results[word * 12].onStrip, `слово ${word + 1}`);
  }
});

test("a word stays drawn while it fades", () => {
  const tracker = new VoiceNoiseTracker();
  feed(tracker, roomNoise(3, 0.2));
  const results = feed(tracker, [0.8, 0.5, 0.3]);
  assert.ok(results.every((result) => result.onStrip));
});

test("line height follows loudness instead of being the same", () => {
  const tracker = new VoiceNoiseTracker();
  feed(tracker, roomNoise(3, 0.1));
  const results = feed(tracker, [3, 0.3, 0.6, 1.5, 0.4, 0.25]);
  const shares = results.map((result) => result.share);
  assert.ok(new Set(shares).size === shares.length, shares.join());
  assert.ok(shares[0] > shares[3] && shares[3] > shares[2] && shares[2] > shares[1], shares.join());
  assert.equal(shares[0], 1);
});

test("the middle of the recent range is drawn about half high", () => {
  const tracker = new VoiceNoiseTracker();
  feed(tracker, roomNoise(3, 0.1));
  feed(tracker, [3]);
  const [result] = feed(tracker, [Math.sqrt(30) * 0.1]);
  assert.ok(result.share > 0.35 && result.share < 0.65, String(result.share));
});

test("barely audible speech is not drawn at full height", () => {
  const tracker = new VoiceNoiseTracker();
  feed(tracker, roomNoise(3, 0.1));
  const [result] = feed(tracker, [0.2]);
  assert.ok(result.onStrip && result.share < 0.6, String(result.share));
});

test("long speech stays visible and audible for recognition", () => {
  const tracker = new VoiceNoiseTracker();
  feed(tracker, roomNoise(2, 0.1));
  const results = feed(tracker, speechWithGaps(60, 0.5, 0.1)).slice(-LEVELS_PER_SECOND * 5);
  const spokenFrames = results.filter((_, index) => index % 5 !== 4);
  assert.ok(spokenFrames.every((result) => result.onStrip && result.forRecognition));
});

test("speech right after the microphone turns on reaches recognition", () => {
  const results = feed(new VoiceNoiseTracker(), new Array(LEVELS_PER_SECOND).fill(0.5));
  assert.ok(results.every((result) => result.forRecognition));
});

test("the very start draws dots until the noise is known", () => {
  const results = feed(new VoiceNoiseTracker(), [0.5, 0.5, 0.5, 0.5]);
  assert.ok(results.every((result) => !result.onStrip));
});

test("digital silence is neither speech nor voice", () => {
  const results = feed(new VoiceNoiseTracker(), new Array(40).fill(0));
  assert.ok(results.every((result) => !result.onStrip && !result.forRecognition));
});

test("speech over digital silence is drawn with a finite height", () => {
  const tracker = new VoiceNoiseTracker();
  feed(tracker, new Array(40).fill(0));
  const results = feed(tracker, [0.3, 0.6]);
  assert.ok(results.every((result) => result.share > 0 && result.share <= 1), results.map((result) => result.share).join());
});

test("a fan turned on during recording stops being speech within seconds", () => {
  const tracker = new VoiceNoiseTracker();
  feed(tracker, roomNoise(5, 0.02));
  const results = feed(tracker, roomNoise(10, 0.2));
  const lastSeconds = results.slice(-LEVELS_PER_SECOND * 3);
  assert.ok(lastSeconds.every((result) => !result.onStrip && !result.forRecognition));
});
