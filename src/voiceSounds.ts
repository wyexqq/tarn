const NOTE_SECONDS = 0.075;
const NOTE_FADE_IN_SECONDS = 0.008;
const NOTE_RING_SECONDS = 0.11;
const NOTE_VOLUME = 0.09;
const SILENT_VOLUME = 0.0001;
const CONTEXT_LIFETIME_MS = 500;
const LOW_NOTE_HZ = 620;
const HIGH_NOTE_HZ = 880;

function playNote(context: AudioContext, frequency: number, startAt: number): void {
  const oscillator = context.createOscillator();
  const volume = context.createGain();
  oscillator.type = "sine";
  oscillator.frequency.value = frequency;
  volume.gain.setValueAtTime(SILENT_VOLUME, startAt);
  volume.gain.linearRampToValueAtTime(NOTE_VOLUME, startAt + NOTE_FADE_IN_SECONDS);
  volume.gain.exponentialRampToValueAtTime(SILENT_VOLUME, startAt + NOTE_RING_SECONDS);
  oscillator.connect(volume);
  volume.connect(context.destination);
  oscillator.start(startAt);
  oscillator.stop(startAt + NOTE_RING_SECONDS);
}

function playNotes(frequencies: number[]): void {
  const context = new AudioContext();
  frequencies.forEach((frequency, index) => playNote(context, frequency, context.currentTime + index * NOTE_SECONDS));
  window.setTimeout(() => void context.close(), CONTEXT_LIFETIME_MS);
}

export function playMicrophoneOn(): void {
  playNotes([LOW_NOTE_HZ, HIGH_NOTE_HZ]);
}

export function playMicrophoneOff(): void {
  playNotes([HIGH_NOTE_HZ, LOW_NOTE_HZ]);
}
