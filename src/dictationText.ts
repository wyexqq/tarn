export interface RefinedGroup {
  segmentIds: number[];
  text: string;
}

interface TextPiece {
  separator: string;
  text: string;
  segmentId: number | null;
}

function commonPrefixLength(first: string, second: string): number {
  const limit = Math.min(first.length, second.length);
  let length = 0;
  while (length < limit && first[length] === second[length]) {
    length += 1;
  }
  return length;
}

function commonSuffixLength(first: string, second: string, prefixLength: number): number {
  const limit = Math.min(first.length, second.length) - prefixLength;
  let length = 0;
  while (length < limit && first[first.length - 1 - length] === second[second.length - 1 - length]) {
    length += 1;
  }
  return length;
}

function alignDeletionWithPieceEnd(current: string, start: number, end: number, inserted: string, ranges: { end: number }[]) {
  const pieceEnds = new Set(ranges.map((range) => range.end));
  if (inserted.length > 0 || start === end || pieceEnds.has(end)) {
    return { editStart: start, editEnd: end };
  }
  let shiftedStart = start;
  let shiftedEnd = end;
  while (shiftedStart > 0 && current[shiftedStart - 1] === current[shiftedEnd - 1]) {
    shiftedStart -= 1;
    shiftedEnd -= 1;
    if (pieceEnds.has(shiftedEnd)) {
      return { editStart: shiftedStart, editEnd: shiftedEnd };
    }
  }
  return { editStart: start, editEnd: end };
}

export function mapCaret(oldValue: string, newValue: string, caret: number): number {
  if (caret >= oldValue.length) {
    return newValue.length;
  }
  const prefix = commonPrefixLength(oldValue, newValue);
  if (caret <= prefix) {
    return caret;
  }
  const suffix = commonSuffixLength(oldValue, newValue, prefix);
  if (caret >= oldValue.length - suffix) {
    return newValue.length - (oldValue.length - caret);
  }
  return Math.min(prefix, newValue.length);
}

export class DictationText {
  private pieces: TextPiece[] = [];
  private editedSegments = new Set<number>();

  constructor(initialText: string) {
    if (initialText.length > 0) {
      this.pieces.push({ separator: "", text: initialText, segmentId: null });
    }
  }

  value(): string {
    return this.pieces.map((piece) => piece.separator + piece.text).join("");
  }

  setSegmentText(segmentId: number, spoken: string): void {
    if (spoken.length === 0 || this.editedSegments.has(segmentId)) {
      return;
    }
    const existing = this.pieces.find((piece) => piece.segmentId === segmentId);
    if (existing) {
      existing.text = spoken;
      return;
    }
    const current = this.value();
    const needsSpace = current.length > 0 && !/\s$/.test(current);
    this.pieces.push({ separator: needsSpace ? " " : "", text: spoken, segmentId });
  }

  applyUserEdit(nextValue: string): number[] {
    const current = this.value();
    const prefix = commonPrefixLength(current, nextValue);
    const suffix = commonSuffixLength(current, nextValue, prefix);
    const inserted = nextValue.slice(prefix, nextValue.length - suffix);
    const ranges = this.pieceRanges();
    const { editStart, editEnd } = alignDeletionWithPieceEnd(current, prefix, current.length - suffix, inserted, ranges);

    const isTouched = (start: number, end: number) => (editStart === editEnd ? start < editStart && editStart < end : editStart < end && editEnd > start);
    const touched = ranges.filter((range) => isTouched(range.start, range.end));

    if (touched.length === 0) {
      if (inserted.length > 0) {
        const position = ranges.findIndex((range) => range.start >= editStart);
        const insertAt = position === -1 ? this.pieces.length : position;
        this.pieces.splice(insertAt, 0, { separator: "", text: inserted, segmentId: null });
      }
      return [];
    }

    const first = touched[0];
    const last = touched[touched.length - 1];
    const mergedText = current.slice(first.start, editStart) + inserted + current.slice(editEnd, last.end);
    const editedIds = touched.flatMap((range) => (range.segmentId === null ? [] : [range.segmentId]));
    editedIds.forEach((id) => this.editedSegments.add(id));
    const replacement: TextPiece[] = mergedText.length > 0 ? [{ separator: "", text: mergedText, segmentId: null }] : [];
    this.pieces.splice(first.index, last.index - first.index + 1, ...replacement);
    return editedIds;
  }

  applyRefinedGroup(group: RefinedGroup): void {
    if (group.text.length === 0 || group.segmentIds.some((id) => this.editedSegments.has(id))) {
      return;
    }
    const indexes = this.pieces.flatMap((piece, index) => (piece.segmentId !== null && group.segmentIds.includes(piece.segmentId) ? [index] : []));
    if (indexes.length === 0) {
      this.setSegmentText(group.segmentIds[0], group.text);
      return;
    }
    const first = indexes[0];
    const isContinuous = indexes[indexes.length - 1] - first + 1 === indexes.length;
    if (!isContinuous) {
      return;
    }
    const replacement: TextPiece = { separator: this.pieces[first].separator, text: group.text, segmentId: group.segmentIds[0] };
    this.pieces.splice(first, indexes.length, replacement);
  }

  private pieceRanges() {
    let offset = 0;
    return this.pieces.map((piece, index) => {
      const start = offset;
      offset += piece.separator.length + piece.text.length;
      return { index, start, end: offset, segmentId: piece.segmentId };
    });
  }
}
