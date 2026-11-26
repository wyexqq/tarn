import assert from "node:assert/strict";
import { test } from "node:test";

import { allReplacements, applyReplacements, parseUserReplacements } from "../src/voiceDictionary.ts";

const everyday = allReplacements("");

test("terms the model mishears become the right words", () => {
  assert.equal(applyReplacements("Открой код-код и проверь", everyday), "Открой Claude Code и проверь");
  assert.equal(applyReplacements("Код код уже запущен", everyday), "Claude Code уже запущен");
  assert.equal(applyReplacements("спроси клод", everyday), "спроси Claude");
  assert.equal(applyReplacements("Парсор упал, а парсёр нет", everyday), "Парсер упал, а парсер нет");
});

test("only whole words are replaced", () => {
  assert.equal(applyReplacements("исходный код и кодировка", everyday), "исходный код и кодировка");
  assert.equal(applyReplacements("пасерби", everyday), "пасерби");
});

test("own pairs are read line by line and win over built-in ones", () => {
  const own = parseUserReplacements("тарн = Tarn\nбез разделителя\nклод → Клод\n = пусто");
  assert.deepEqual(own, [
    { heard: "тарн", meant: "Tarn" },
    { heard: "клод", meant: "Клод" },
  ]);
  const replacements = allReplacements("тарн = Tarn\nклод = Клод");
  assert.equal(applyReplacements("тарн и клод", replacements), "Tarn и Клод");
});
