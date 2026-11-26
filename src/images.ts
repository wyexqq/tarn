import { api, type Attachment } from "./api";

const MAX_IMAGE_SIDE_PX = 2000;
const MAX_IMAGE_BYTES = 3_500_000;
const MAX_TEXT_FILE_BYTES = 2 * 1024 * 1024;
const JPEG_QUALITY = 0.9;
const MIMES_MODELS_ACCEPT = ["image/png", "image/jpeg", "image/webp", "image/gif"];
const DATA_URL_SEPARATOR = ";base64,";

export async function withImageData(attachment: Attachment): Promise<Attachment> {
  if (attachment.kind !== "image" || attachment.dataBase64 || !attachment.imageId) {
    return attachment;
  }
  const source = await api.readImage(attachment.imageId);
  return { ...attachment, dataBase64: source.slice(source.indexOf(",") + 1) };
}

export function imageDataUrl(attachment: Attachment): string {
  return `data:${attachment.mime};base64,${attachment.dataBase64}`;
}

function readAsDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(new Error(`Не удалось прочитать ${file.name}`));
    reader.readAsDataURL(file);
  });
}

function loadPicture(dataUrl: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const picture = new Image();
    picture.onload = () => resolve(picture);
    picture.onerror = () => reject(new Error("Не удалось открыть изображение"));
    picture.src = dataUrl;
  });
}

function base64ByteLength(encoded: string): number {
  return Math.floor((encoded.length * 3) / 4);
}

function splitDataUrl(dataUrl: string): { mime: string; encoded: string } {
  const separatorIndex = dataUrl.indexOf(DATA_URL_SEPARATOR);
  return {
    mime: dataUrl.slice("data:".length, separatorIndex),
    encoded: dataUrl.slice(separatorIndex + DATA_URL_SEPARATOR.length),
  };
}

function drawScaled(picture: HTMLImageElement, scale: number): HTMLCanvasElement {
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(picture.naturalWidth * scale));
  canvas.height = Math.max(1, Math.round(picture.naturalHeight * scale));
  canvas.getContext("2d")?.drawImage(picture, 0, 0, canvas.width, canvas.height);
  return canvas;
}

function encodeWithinLimit(canvas: HTMLCanvasElement): string {
  const lossless = canvas.toDataURL("image/png");
  if (base64ByteLength(splitDataUrl(lossless).encoded) <= MAX_IMAGE_BYTES) {
    return lossless;
  }
  return canvas.toDataURL("image/jpeg", JPEG_QUALITY);
}

async function fitForModels(name: string, dataUrl: string): Promise<Attachment> {
  const { mime, encoded } = splitDataUrl(dataUrl);
  const picture = await loadPicture(dataUrl);
  const longestSide = Math.max(picture.naturalWidth, picture.naturalHeight);
  const fitsAsIs = longestSide <= MAX_IMAGE_SIDE_PX && base64ByteLength(encoded) <= MAX_IMAGE_BYTES && MIMES_MODELS_ACCEPT.includes(mime);
  if (fitsAsIs) {
    return { name, mime, kind: "image", size: base64ByteLength(encoded), dataBase64: encoded };
  }
  const scale = Math.min(1, MAX_IMAGE_SIDE_PX / longestSide);
  const fitted = splitDataUrl(encodeWithinLimit(drawScaled(picture, scale)));
  return { name, mime: fitted.mime, kind: "image", size: base64ByteLength(fitted.encoded), dataBase64: fitted.encoded };
}

export function fitAttachmentForModels(attachment: Attachment): Promise<Attachment> {
  if (attachment.kind !== "image" || !attachment.dataBase64) {
    return Promise.resolve(attachment);
  }
  return fitForModels(attachment.name, imageDataUrl(attachment));
}

export async function attachmentFromFile(file: File): Promise<Attachment> {
  if (file.type.startsWith("image/")) {
    const name = file.name || "изображение";
    return fitForModels(name, await readAsDataUrl(file));
  }
  if (file.size > MAX_TEXT_FILE_BYTES) {
    throw new Error(`${file.name} больше 2 МБ`);
  }
  const text = await file.text();
  if (text.includes("\u0000")) {
    throw new Error(`${file.name} не текстовый файл. Пока подходят текст и картинки`);
  }
  return { name: file.name, mime: "text/plain", kind: "text", size: file.size, text };
}
