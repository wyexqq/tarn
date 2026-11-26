const MAX_NAMED_FILES = 3;

function namedFiles(names: string[]): string {
  const shown = names.slice(0, MAX_NAMED_FILES).join(", ");
  const hiddenCount = names.length - MAX_NAMED_FILES;
  return hiddenCount > 0 ? `${shown} и ещё ${hiddenCount}` : shown;
}

export function notRestoredText(names: string[]): string {
  return `Не возвращены: ${namedFiles(names)}`;
}

export function rewindDoneText(restoredFiles: number, notRestored: string[], mentionCommands: boolean): string {
  const sentences = [restoredFiles > 0 ? "Чат и файлы откачены" : "Чат откачен"];
  if (notRestored.length > 0) {
    sentences.push(notRestoredText(notRestored));
  }
  if (mentionCommands) {
    sentences.push("Результат команд остался");
  }
  return sentences.join(". ");
}
