import assert from "node:assert/strict";
import { test } from "node:test";

import { DictationText, mapCaret } from "../src/dictationText.ts";

function dictated(...texts) {
  const document = new DictationText("");
  texts.forEach((text, index) => document.setSegmentText(index + 1, text));
  return document;
}

test("speech is added after what was typed, with a space", () => {
  const document = new DictationText("Вот:");
  document.setSegmentText(1, "первое");
  document.setSegmentText(2, "второе");
  assert.equal(document.value(), "Вот: первое второе");
});

test("no double space when the typed text already ends with one", () => {
  const document = new DictationText("Вот ");
  document.setSegmentText(1, "слово");
  assert.equal(document.value(), "Вот слово");
});

test("a newer draft replaces the old draft of the same piece", () => {
  const document = dictated("при");
  document.setSegmentText(1, "привет");
  assert.equal(document.value(), "привет");
});

test("an empty recognition result does not erase the shown text", () => {
  const document = dictated("привет");
  document.setSegmentText(1, "");
  assert.equal(document.value(), "привет");
});

test("typing before the speech keeps it updating", () => {
  const document = dictated("при");
  assert.deepEqual(document.applyUserEdit("Эй при"), []);
  document.setSegmentText(1, "привет");
  assert.equal(document.value(), "Эй привет");
});

test("typing at the very end does not touch the speech and goes before later speech", () => {
  const document = dictated("привет");
  assert.deepEqual(document.applyUserEdit("привет!!"), []);
  document.setSegmentText(2, "дальше");
  assert.equal(document.value(), "привет!! дальше");
});

test("editing inside a piece freezes it against later drafts", () => {
  const document = dictated("плохой текст");
  assert.deepEqual(document.applyUserEdit("хороший текст"), [1]);
  document.setSegmentText(1, "плохой текст и ещё");
  assert.equal(document.value(), "хороший текст");
});

test("deleting a whole piece keeps the neighbours working", () => {
  const document = dictated("один", "два", "три");
  assert.deepEqual(document.applyUserEdit("один три"), [2]);
  document.setSegmentText(3, "три четыре");
  assert.equal(document.value(), "один три четыре");
});

test("selecting across two pieces and typing freezes both", () => {
  const document = dictated("один два", "три четыре");
  assert.deepEqual(document.applyUserEdit("один X четыре"), [1, 2]);
  assert.equal(document.value(), "один X четыре");
});

test("refined text replaces the untouched pieces of its group", () => {
  const document = dictated("при вет", "как дела");
  document.applyRefinedGroup({ segmentIds: [1, 2], text: "Привет, как дела?" });
  assert.equal(document.value(), "Привет, как дела?");
});

test("refined text keeps the space after typed text", () => {
  const document = new DictationText("Вот:");
  document.setSegmentText(1, "при вет");
  document.setSegmentText(2, "как дела");
  document.applyRefinedGroup({ segmentIds: [1, 2], text: "Привет, как дела?" });
  assert.equal(document.value(), "Вот: Привет, как дела?");
});

test("refined text never overwrites a group the user edited", () => {
  const document = dictated("при вет", "как дела");
  document.applyUserEdit("Привет как дела");
  document.applyRefinedGroup({ segmentIds: [1, 2], text: "Другой текст" });
  assert.equal(document.value(), "Привет как дела");
});

test("refined text does not swallow what the user typed between pieces", () => {
  const document = dictated("один", "два");
  document.applyUserEdit("один МОЁ два");
  document.applyRefinedGroup({ segmentIds: [1, 2], text: "Один два" });
  assert.ok(document.value().includes("МОЁ"));
});

test("refined text appears even when the draft never came", () => {
  const document = new DictationText("Вот:");
  document.applyRefinedGroup({ segmentIds: [5], text: "текст" });
  assert.equal(document.value(), "Вот: текст");
});

test("empty refined text keeps the draft", () => {
  const document = dictated("черновик");
  document.applyRefinedGroup({ segmentIds: [1], text: "" });
  assert.equal(document.value(), "черновик");
});

test("clearing the whole field removes everything", () => {
  const document = dictated("один", "два");
  assert.deepEqual(document.applyUserEdit(""), [1, 2]);
  assert.equal(document.value(), "");
  document.setSegmentText(3, "снова");
  assert.equal(document.value(), "снова");
});

test("value after an edit always equals what the user sees", () => {
  const document = dictated("один два", "три");
  for (const next of ["один два три", "один ддва три", "ддва три", "Яддва три", "Я"]) {
    document.applyUserEdit(next);
    assert.equal(document.value(), next);
  }
});

test("caret at the end follows the growing text", () => {
  assert.equal(mapCaret("привет", "привет мир", 6), 10);
});

test("caret before the changed part stays in place", () => {
  assert.equal(mapCaret("один два", "один два три", 4), 4);
});

test("caret after the changed part keeps its distance from the end", () => {
  assert.equal(mapCaret("один два конец", "один двадцать конец", 12), 17);
});

test("caret inside a replaced part moves to its start", () => {
  assert.equal(mapCaret("один два конец", "один ХХХ конец", 6), 5);
});
