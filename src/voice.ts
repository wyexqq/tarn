export const SAMPLE_RATE = 16000;

const PROCESSOR_BUFFER_SIZE = 1024;
const BYTES_PER_SAMPLE = 2;
const WAV_HEADER_BYTES = 44;
const MAX_RECORDING_MS = 10 * 60 * 1000;
const BASE64_CHUNK_BYTES = 0x8000;
const LEVEL_GAIN = 6;

export interface Recording {
  sampleCount: () => number;
  audioBetween: (startSample: number, endSample: number) => string;
  stop: () => void;
  cancel: () => void;
}

function writeAscii(view: DataView, offset: number, text: string): void {
  for (let index = 0; index < text.length; index += 1) {
    view.setUint8(offset + index, text.charCodeAt(index));
  }
}

function encodeWav(chunks: Float32Array[], startSample: number, endSample: number): Uint8Array {
  const dataBytes = (endSample - startSample) * BYTES_PER_SAMPLE;
  const buffer = new ArrayBuffer(WAV_HEADER_BYTES + dataBytes);
  const view = new DataView(buffer);
  writeAscii(view, 0, "RIFF");
  view.setUint32(4, 36 + dataBytes, true);
  writeAscii(view, 8, "WAVEfmt ");
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, SAMPLE_RATE, true);
  view.setUint32(28, SAMPLE_RATE * BYTES_PER_SAMPLE, true);
  view.setUint16(32, BYTES_PER_SAMPLE, true);
  view.setUint16(34, 16, true);
  writeAscii(view, 36, "data");
  view.setUint32(40, dataBytes, true);
  let offset = WAV_HEADER_BYTES;
  let chunkStart = 0;
  for (const chunk of chunks) {
    const chunkEnd = chunkStart + chunk.length;
    const firstIndex = Math.max(0, startSample - chunkStart);
    const lastIndex = Math.min(chunk.length, endSample - chunkStart);
    for (let index = firstIndex; index < lastIndex; index += 1) {
      const limited = Math.max(-1, Math.min(1, chunk[index]));
      view.setInt16(offset, limited * 0x7fff, true);
      offset += BYTES_PER_SAMPLE;
    }
    chunkStart = chunkEnd;
  }
  return new Uint8Array(buffer);
}

function toBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let start = 0; start < bytes.length; start += BASE64_CHUNK_BYTES) {
    binary += String.fromCharCode(...bytes.subarray(start, start + BASE64_CHUNK_BYTES));
  }
  return btoa(binary);
}

function loudnessOf(samples: Float32Array): number {
  let sum = 0;
  for (const sample of samples) {
    sum += sample * sample;
  }
  return Math.sqrt(sum / samples.length) * LEVEL_GAIN;
}

export function describeMicrophoneError(error: unknown): string {
  const name = error instanceof DOMException ? error.name : "";
  if (name === "NotAllowedError") {
    return "Нет доступа к микрофону. Разреши его в настройках Windows: Конфиденциальность → Микрофон";
  }
  if (name === "NotFoundError") {
    return "Микрофон не найден";
  }
  return `Не удалось включить микрофон: ${String(error)}`;
}

export async function startRecording(onLevel: (level: number, sampleCount: number) => void, onTimeLimit: () => void): Promise<Recording> {
  const stream = await navigator.mediaDevices.getUserMedia({
    audio: { channelCount: 1, echoCancellation: false, noiseSuppression: false, autoGainControl: false },
  });
  const context = new AudioContext({ sampleRate: SAMPLE_RATE });
  const source = context.createMediaStreamSource(stream);
  const processor = context.createScriptProcessor(PROCESSOR_BUFFER_SIZE, 1, 1);
  const chunks: Float32Array[] = [];
  let recordedSamples = 0;
  processor.onaudioprocess = (event) => {
    const samples = new Float32Array(event.inputBuffer.getChannelData(0));
    chunks.push(samples);
    recordedSamples += samples.length;
    onLevel(loudnessOf(samples), recordedSamples);
  };
  source.connect(processor);
  processor.connect(context.destination);
  const timeLimit = window.setTimeout(onTimeLimit, MAX_RECORDING_MS);

  const release = () => {
    window.clearTimeout(timeLimit);
    processor.onaudioprocess = null;
    processor.disconnect();
    source.disconnect();
    stream.getTracks().forEach((track) => track.stop());
    void context.close();
  };

  return {
    sampleCount: () => recordedSamples,
    audioBetween: (startSample, endSample) => toBase64(encodeWav(chunks, startSample, endSample)),
    stop: release,
    cancel: release,
  };
}
