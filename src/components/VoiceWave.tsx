import { onCleanup, onMount } from "solid-js";

const LEVELS_PER_COLUMN = 2;
const COLUMN_DURATION_MS = 128;
const COLUMN_STEP = 5;
const STRIP_WIDTH = 150;
const STRIP_HEIGHT = 18;
const COLUMN_COUNT = Math.ceil(STRIP_WIDTH / COLUMN_STEP) + 2;
const DOT_RADIUS = 0.9;
const LINE_WIDTH = 1.6;
const SHORTEST_LINE = 4;

export class VoiceLevelColumns {
  columns: number[] = new Array(COLUMN_COUNT).fill(0);
  lastColumnAt = 0;
  private waitingLevels: number[] = [];

  clear(): void {
    this.columns = new Array(COLUMN_COUNT).fill(0);
    this.waitingLevels = [];
    this.lastColumnAt = 0;
  }

  add(level: number): void {
    this.waitingLevels.push(level);
    if (this.waitingLevels.length < LEVELS_PER_COLUMN) {
      return;
    }
    this.columns.push(Math.max(...this.waitingLevels));
    this.columns.shift();
    this.waitingLevels = [];
    this.lastColumnAt = performance.now();
  }
}

function drawColumn(drawing: CanvasRenderingContext2D, x: number, level: number): void {
  const middle = STRIP_HEIGHT / 2;
  drawing.beginPath();
  if (level === 0) {
    drawing.arc(x, middle, DOT_RADIUS, 0, Math.PI * 2);
    drawing.fill();
    return;
  }
  const height = SHORTEST_LINE + level * (STRIP_HEIGHT - SHORTEST_LINE);
  drawing.moveTo(x, middle - height / 2);
  drawing.lineTo(x, middle + height / 2);
  drawing.stroke();
}

export function VoiceWave(props: { levels: VoiceLevelColumns }) {
  let canvas: HTMLCanvasElement | undefined;
  let frame = 0;
  const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");

  const draw = () => {
    frame = requestAnimationFrame(draw);
    const drawing = canvas?.getContext("2d");
    if (!canvas || !drawing) {
      return;
    }
    const scale = window.devicePixelRatio;
    const pixelWidth = Math.round(STRIP_WIDTH * scale);
    if (canvas.width !== pixelWidth) {
      canvas.width = pixelWidth;
      canvas.height = Math.round(STRIP_HEIGHT * scale);
    }
    const color = getComputedStyle(canvas).color;
    drawing.setTransform(scale, 0, 0, scale, 0, 0);
    drawing.clearRect(0, 0, STRIP_WIDTH, STRIP_HEIGHT);
    drawing.fillStyle = color;
    drawing.strokeStyle = color;
    drawing.lineWidth = LINE_WIDTH;
    drawing.lineCap = "round";
    const elapsed = (performance.now() - props.levels.lastColumnAt) / COLUMN_DURATION_MS;
    const slide = reducedMotion.matches ? 1 : Math.min(1, elapsed);
    const columns = props.levels.columns;
    for (let age = 0; age < columns.length; age += 1) {
      const x = STRIP_WIDTH + COLUMN_STEP / 2 - (age + slide) * COLUMN_STEP;
      drawColumn(drawing, x, columns[columns.length - 1 - age]);
    }
  };

  onMount(() => {
    frame = requestAnimationFrame(draw);
  });
  onCleanup(() => cancelAnimationFrame(frame));

  return <canvas class="voice-wave" ref={canvas} style={{ width: `${STRIP_WIDTH}px`, height: `${STRIP_HEIGHT}px` }} aria-hidden="true" />;
}
