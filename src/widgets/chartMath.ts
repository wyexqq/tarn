export interface Scale {
  min: number;
  max: number;
  ticks: number[];
}

const NICE_FRACTIONS = [1, 2, 2.5, 5, 10];
const COMPACT_FROM = 10000;

function niceStep(range: number, targetTicks: number): number {
  const rough = range / targetTicks;
  const power = Math.pow(10, Math.floor(Math.log10(rough)));
  const fraction = rough / power;
  const nice = NICE_FRACTIONS.find((candidate) => fraction <= candidate) ?? 10;
  return nice * power;
}

function cleanFloat(value: number): number {
  return Number(value.toPrecision(12));
}

export function niceScale(values: number[], includeZero: boolean, targetTicks = 4): Scale {
  const finite = values.filter((value) => Number.isFinite(value));
  let low = finite.length > 0 ? Math.min(...finite) : 0;
  let high = finite.length > 0 ? Math.max(...finite) : 1;
  if (includeZero) {
    low = Math.min(0, low);
    high = Math.max(0, high);
  }
  if (high === low) {
    high = low + 1;
  }
  const step = niceStep(high - low, targetTicks);
  const min = cleanFloat(Math.floor(low / step) * step);
  const max = cleanFloat(Math.ceil(high / step) * step);
  const ticks: number[] = [];
  for (let tick = min; tick <= max + step / 2; tick += step) {
    ticks.push(cleanFloat(tick));
  }
  return { min, max, ticks };
}

function plainNumber(value: number): string {
  return value.toLocaleString("ru-RU", { maximumFractionDigits: 2 });
}

function compactNumber(value: number): string {
  if (Math.abs(value) < COMPACT_FROM) {
    return plainNumber(value);
  }
  return new Intl.NumberFormat("ru-RU", { notation: "compact", maximumFractionDigits: 1 }).format(value);
}

function withUnit(number: string, unit: string | undefined): string {
  const trimmed = unit?.trim() ?? "";
  if (!trimmed) {
    return number;
  }
  if (trimmed === "$") {
    return `$${number}`;
  }
  if (trimmed === "%") {
    return `${number}%`;
  }
  return `${number} ${trimmed}`;
}

export function formatValue(value: number, unit?: string): string {
  return withUnit(plainNumber(value), unit);
}

export function formatTick(value: number, unit?: string): string {
  return withUnit(compactNumber(value), unit);
}

export function labelStep(count: number, plotWidth: number, minLabelWidth = 56): number {
  const fits = Math.max(1, Math.floor(plotWidth / minLabelWidth));
  return Math.max(1, Math.ceil(count / fits));
}

export function shares(values: number[]): number[] {
  const total = values.reduce((sum, value) => sum + Math.max(0, value), 0);
  if (total === 0) {
    return values.map(() => 0);
  }
  return values.map((value) => Math.max(0, value) / total);
}
