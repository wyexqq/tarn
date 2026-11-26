import assert from "node:assert/strict";
import { test } from "node:test";

import { notRestoredText, rewindDoneText } from "../src/rewindText.ts";

test("only the chat was rewound", () => {
  assert.equal(rewindDoneText(0, [], false), "Чат откачен");
});

test("files came back with the chat", () => {
  assert.equal(rewindDoneText(2, [], false), "Чат и файлы откачены");
});

test("files that stayed are named", () => {
  assert.equal(rewindDoneText(1, ["a.txt"], false), "Чат и файлы откачены. Не возвращены: a.txt");
});

test("no file came back at all", () => {
  assert.equal(rewindDoneText(0, ["a.txt"], false), "Чат откачен. Не возвращены: a.txt");
});

test("commands are mentioned last", () => {
  assert.equal(rewindDoneText(1, ["a.txt"], true), "Чат и файлы откачены. Не возвращены: a.txt. Результат команд остался");
});

test("a long list of files is cut", () => {
  assert.equal(notRestoredText(["a", "b", "c", "d", "e"]), "Не возвращены: a, b, c и ещё 2");
});
