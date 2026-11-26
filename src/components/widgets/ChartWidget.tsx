import { For, Index, Show, createMemo, createSignal } from "solid-js";

import { effectiveTheme } from "../../theme";
import { formatTick, formatValue, labelStep, niceScale, shares, type Scale } from "../../widgets/chartMath";
import { seriesColor } from "../../widgets/tokens";
import type { ChartSeries, ChartSpec } from "../../widgets/types";
import { ChartKindIcon, TableIcon } from "../icons";
import { WidgetAction, WidgetCard } from "./WidgetCard";

const WIDTH = 640;
const HEIGHT = 260;
const TOP = 14;
const RIGHT = 16;
const BOTTOM = 30;
const MAX_BAR_WIDTH = 24;
const BAR_RADIUS = 4;
const GROUP_SHARE = 0.72;
const MARKER_RADIUS = 4;
const MAX_POINTS_WITH_MARKERS = 24;
const MAX_PARTS = 8;
const OTHER_LABEL = "Другое";
const TICK_CHAR_WIDTH = 6.4;

interface TipRow {
  name: string;
  value: string;
  color: string;
}

interface Tip {
  x: number;
  y: number;
  title: string;
  rows: TipRow[];
}

interface BarMark {
  path: string;
  color: string;
}

interface Plot {
  left: number;
  scale: Scale;
  y: (value: number) => number;
  slotCenters: number[];
  slotWidth: number;
  shownLabels: number[];
}

function barShape(x: number, width: number, baseY: number, endY: number, round: boolean): string {
  const height = Math.abs(baseY - endY);
  const radius = round ? Math.min(BAR_RADIUS, width / 2, height) : 0;
  const direction = endY < baseY ? 1 : -1;
  const cornerY = endY + direction * radius;
  return `M${x} ${baseY}V${cornerY}Q${x} ${endY} ${x + radius} ${endY}H${x + width - radius}Q${x + width} ${endY} ${x + width} ${cornerY}V${baseY}Z`;
}

function valueAt(series: ChartSeries, index: number): number {
  const value = series.values[index];
  return typeof value === "number" && Number.isFinite(value) ? value : 0;
}

function stackTotals(series: ChartSeries[], count: number): number[] {
  const totals: number[] = [];
  for (let index = 0; index < count; index += 1) {
    const positive = series.reduce((sum, row) => sum + Math.max(0, valueAt(row, index)), 0);
    const negative = series.reduce((sum, row) => sum + Math.min(0, valueAt(row, index)), 0);
    totals.push(positive, negative);
  }
  return totals;
}

function buildPlot(spec: ChartSpec, visible: ChartSeries[]): Plot {
  const count = spec.labels.length;
  const isBar = spec.type === "bar";
  const values = isBar && spec.stacked ? stackTotals(visible, count) : visible.flatMap((row) => row.values.slice(0, count));
  const scale = niceScale(values, isBar);
  const longestTick = Math.max(...scale.ticks.map((tick) => formatTick(tick, spec.unit).length));
  const left = Math.min(96, Math.max(34, longestTick * TICK_CHAR_WIDTH + 12));
  const plotWidth = WIDTH - left - RIGHT;
  const plotHeight = HEIGHT - TOP - BOTTOM;
  const y = (value: number) => TOP + plotHeight - ((value - scale.min) / (scale.max - scale.min)) * plotHeight;
  const slotWidth = isBar ? plotWidth / count : count > 1 ? plotWidth / (count - 1) : plotWidth;
  const slotCenters = spec.labels.map((_, index) => {
    if (isBar) {
      return left + slotWidth * index + slotWidth / 2;
    }
    return count > 1 ? left + slotWidth * index : left + plotWidth / 2;
  });
  const step = labelStep(count, plotWidth);
  const shownLabels = spec.labels.map((_, index) => index).filter((index) => index % step === 0);
  return { left, scale, y, slotCenters, slotWidth, shownLabels };
}

function lastIndexWhere(values: number[], test: (value: number) => boolean): number {
  for (let index = values.length - 1; index >= 0; index -= 1) {
    if (test(values[index])) {
      return index;
    }
  }
  return -1;
}

function stackedBars(plot: Plot, visible: { row: ChartSeries; color: string }[], count: number): BarMark[] {
  const width = Math.min(MAX_BAR_WIDTH, plot.slotWidth * 0.6);
  const marks: BarMark[] = [];
  for (let index = 0; index < count; index += 1) {
    const x = plot.slotCenters[index] - width / 2;
    let positive = 0;
    let negative = 0;
    const values = visible.map(({ row }) => valueAt(row, index));
    const lastPositive = lastIndexWhere(values, (value) => value > 0);
    const lastNegative = lastIndexWhere(values, (value) => value < 0);
    visible.forEach(({ row, color }, seriesIndex) => {
      const value = valueAt(row, index);
      if (value === 0) {
        return;
      }
      const start = value > 0 ? positive : negative;
      const end = start + value;
      const isOutermost = seriesIndex === (value > 0 ? lastPositive : lastNegative);
      marks.push({ path: barShape(x, width, plot.y(start), plot.y(end), isOutermost), color });
      if (value > 0) {
        positive = end;
      } else {
        negative = end;
      }
    });
  }
  return marks;
}

function groupedBars(plot: Plot, visible: { row: ChartSeries; color: string }[], count: number): BarMark[] {
  const seriesCount = Math.max(1, visible.length);
  const width = Math.min(MAX_BAR_WIDTH, (plot.slotWidth * GROUP_SHARE) / seriesCount);
  const groupWidth = width * seriesCount;
  const baseline = plot.y(Math.max(plot.scale.min, Math.min(0, plot.scale.max)));
  const marks: BarMark[] = [];
  for (let index = 0; index < count; index += 1) {
    const groupLeft = plot.slotCenters[index] - groupWidth / 2;
    visible.forEach(({ row, color }, seriesIndex) => {
      const value = valueAt(row, index);
      if (value !== 0) {
        marks.push({ path: barShape(groupLeft + seriesIndex * width, width, baseline, plot.y(value), true), color });
      }
    });
  }
  return marks;
}

function linePath(plot: Plot, row: ChartSeries): string {
  return plot.slotCenters.map((x, index) => `${index === 0 ? "M" : "L"}${x} ${plot.y(valueAt(row, index))}`).join("");
}

function foldedParts(spec: ChartSpec): { label: string; value: number }[] {
  const row = spec.series[0] ?? { name: "", values: [] };
  const parts = spec.labels.map((label, index) => ({ label, value: Math.max(0, valueAt(row, index)) }));
  if (parts.length <= MAX_PARTS) {
    return parts;
  }
  const sorted = [...parts].sort((first, second) => second.value - first.value);
  const kept = sorted.slice(0, MAX_PARTS - 1);
  const rest = sorted.slice(MAX_PARTS - 1).reduce((sum, part) => sum + part.value, 0);
  return [...kept, { label: OTHER_LABEL, value: rest }];
}

function percent(share: number): string {
  return `${(share * 100).toLocaleString("ru-RU", { maximumFractionDigits: share < 0.1 ? 1 : 0 })}%`;
}

function PartsChart(props: { spec: ChartSpec; showTable: boolean }) {
  const parts = createMemo(() => foldedParts(props.spec));
  const partShares = createMemo(() => shares(parts().map((part) => part.value)));
  const color = (index: number) => seriesColor(index, effectiveTheme());
  return (
    <div class="chart-parts">
      <div class="parts-bar" role="img" aria-label={parts().map((part, index) => `${part.label} ${percent(partShares()[index])}`).join(", ")}>
        <Index each={parts()}>
          {(part, index) => (
            <span
              class="parts-segment"
              style={{ "flex-grow": String(partShares()[index]), background: color(index) }}
              title={`${part().label}: ${formatValue(part().value, props.spec.unit)}, ${percent(partShares()[index])}`}
            />
          )}
        </Index>
      </div>
      <Show when={!props.showTable}>
        <ul class="parts-list">
          <Index each={parts()}>
            {(part, index) => (
              <li>
                <i style={{ background: color(index) }} />
                <span class="parts-name">{part().label}</span>
                <span class="parts-value">{formatValue(part().value, props.spec.unit)}</span>
                <span class="parts-share">{percent(partShares()[index])}</span>
              </li>
            )}
          </Index>
        </ul>
      </Show>
    </div>
  );
}

function ChartTable(props: { spec: ChartSpec }) {
  const isParts = () => props.spec.type === "parts";
  const partShares = createMemo(() => shares(props.spec.series[0]?.values ?? []));
  return (
    <div class="chart-table-wrap">
      <table class="chart-table">
        <thead>
          <tr>
            <th />
            <Show when={!isParts()} fallback={<><th>{props.spec.series[0]?.name || "Значение"}</th><th>Доля</th></>}>
              <For each={props.spec.series}>{(row) => <th>{row.name}</th>}</For>
            </Show>
          </tr>
        </thead>
        <tbody>
          <Index each={props.spec.labels}>
            {(label, index) => (
              <tr>
                <th>{label()}</th>
                <For each={props.spec.series}>{(row) => <td>{formatValue(valueAt(row, index), props.spec.unit)}</td>}</For>
                <Show when={isParts()}>
                  <td>{percent(partShares()[index] ?? 0)}</td>
                </Show>
              </tr>
            )}
          </Index>
        </tbody>
      </table>
    </div>
  );
}

function AxisChart(props: { spec: ChartSpec }) {
  const [hidden, setHidden] = createSignal<ReadonlySet<number>>(new Set());
  const [tip, setTip] = createSignal<Tip | null>(null);
  const [hoverIndex, setHoverIndex] = createSignal<number | null>(null);
  let svgElement: SVGSVGElement | undefined;

  const count = () => props.spec.labels.length;
  const colored = createMemo(() => props.spec.series.map((row, index) => ({ row, index, color: seriesColor(index, effectiveTheme()) })));
  const visible = createMemo(() => colored().filter((entry) => !hidden().has(entry.index)));
  const plot = createMemo(() => buildPlot(props.spec, visible().map((entry) => entry.row)));
  const bars = createMemo(() => {
    if (props.spec.type !== "bar") {
      return [];
    }
    return props.spec.stacked ? stackedBars(plot(), visible(), count()) : groupedBars(plot(), visible(), count());
  });
  const showMarkers = () => count() <= MAX_POINTS_WITH_MARKERS;
  const ariaLabel = () => `${props.spec.type === "line" ? "Линейный" : "Столбчатый"} график: ${props.spec.series.map((row) => row.name).join(", ")}`;

  const toggle = (index: number) => {
    const next = new Set(hidden());
    if (next.has(index)) {
      next.delete(index);
    } else {
      next.add(index);
    }
    setHidden(next);
  };

  const tipFor = (index: number): Tip => {
    const rows = visible().map(({ row, color }) => ({ name: row.name, value: formatValue(valueAt(row, index), props.spec.unit), color }));
    const highest = Math.max(...visible().map(({ row }) => valueAt(row, index)), plot().scale.min);
    const total = visible().reduce((sum, { row }) => sum + valueAt(row, index), 0);
    const title = props.spec.stacked && rows.length > 1 ? `${props.spec.labels[index]} · ${formatValue(total, props.spec.unit)}` : props.spec.labels[index];
    const topY = props.spec.stacked ? plot().y(Math.max(0, total)) : plot().y(highest);
    return { x: plot().slotCenters[index], y: topY, title, rows };
  };

  const pixelsPerUnit = () => (svgElement ? svgElement.getBoundingClientRect().width / WIDTH : 1);

  const hoverLine = (event: MouseEvent) => {
    if (!svgElement || count() === 0) {
      return;
    }
    const bounds = svgElement.getBoundingClientRect();
    const viewX = ((event.clientX - bounds.left) / bounds.width) * WIDTH;
    const centers = plot().slotCenters;
    let nearest = 0;
    centers.forEach((center, index) => {
      if (Math.abs(center - viewX) < Math.abs(centers[nearest] - viewX)) {
        nearest = index;
      }
    });
    setHoverIndex(nearest);
    setTip(tipFor(nearest));
  };

  const leave = () => {
    setHoverIndex(null);
    setTip(null);
  };

  return (
    <>
      <Show when={props.spec.series.length >= 2}>
        <div class="chart-legend">
          <For each={colored()}>
            {(entry) => (
              <button class="legend-item" aria-pressed={!hidden().has(entry.index)} onClick={() => toggle(entry.index)}>
                <i style={{ background: entry.color }} />
                {entry.row.name}
              </button>
            )}
          </For>
        </div>
      </Show>
      <div class="chart-wrap" onMouseLeave={leave}>
        <svg ref={svgElement} viewBox={`0 0 ${WIDTH} ${HEIGHT}`} role="img" aria-label={ariaLabel()}>
          <For each={plot().scale.ticks}>
            {(tick) => (
              <>
                <line class="chart-grid" classList={{ "is-zero": tick === 0 }} x1={plot().left} x2={WIDTH - RIGHT} y1={plot().y(tick)} y2={plot().y(tick)} />
                <text class="chart-tick" x={plot().left - 8} y={plot().y(tick) + 4} text-anchor="end">{formatTick(tick, props.spec.unit)}</text>
              </>
            )}
          </For>
          <For each={plot().shownLabels}>
            {(index) => (
              <text class="chart-tick" x={plot().slotCenters[index]} y={HEIGHT - 10} text-anchor="middle">{props.spec.labels[index]}</text>
            )}
          </For>
          <Show when={props.spec.type === "bar"}>
            <Index each={count() > 0 ? props.spec.labels : []}>
              {(_, index) => (
                <rect
                  class="chart-hit"
                  classList={{ "is-hovered": hoverIndex() === index }}
                  x={plot().slotCenters[index] - plot().slotWidth / 2}
                  y={TOP}
                  width={plot().slotWidth}
                  height={HEIGHT - TOP - BOTTOM}
                  rx="6"
                  onMouseEnter={() => {
                    setHoverIndex(index);
                    setTip(tipFor(index));
                  }}
                />
              )}
            </Index>
            <For each={bars()}>{(bar) => <path class="chart-bar" d={bar.path} fill={bar.color} />}</For>
          </Show>
          <Show when={props.spec.type === "line"}>
            <Show when={hoverIndex() !== null}>
              <line class="chart-crosshair" x1={plot().slotCenters[hoverIndex() ?? 0]} x2={plot().slotCenters[hoverIndex() ?? 0]} y1={TOP} y2={HEIGHT - BOTTOM} />
            </Show>
            <For each={visible()}>
              {(entry) => (
                <>
                  <path class="chart-line" d={linePath(plot(), entry.row)} stroke={entry.color} />
                  <Index each={plot().slotCenters}>
                    {(x, index) => (
                      <Show when={showMarkers() || hoverIndex() === index}>
                        <circle class="chart-marker" cx={x()} cy={plot().y(valueAt(entry.row, index))} r={MARKER_RADIUS} fill={entry.color} />
                      </Show>
                    )}
                  </Index>
                </>
              )}
            </For>
            <rect class="chart-hover-area" x={plot().left} y={TOP} width={WIDTH - plot().left - RIGHT} height={HEIGHT - TOP - BOTTOM} onMouseMove={hoverLine} />
          </Show>
        </svg>
        <Show when={tip()}>
          {(shown) => (
            <div class="chart-tip" style={{ left: `${shown().x * pixelsPerUnit()}px`, top: `${Math.max(36, shown().y * pixelsPerUnit() - 8)}px` }}>
              <strong>{shown().title}</strong>
              <For each={shown().rows}>
                {(row) => (
                  <span>
                    <em><i style={{ background: row.color }} />{row.name}</em>
                    {row.value}
                  </span>
                )}
              </For>
            </div>
          )}
        </Show>
      </div>
    </>
  );
}

export function ChartWidget(props: { title: string; spec: ChartSpec }) {
  const [showTable, setShowTable] = createSignal(false);
  return (
    <WidgetCard
      icon={<ChartKindIcon />}
      title={props.title}
      actions={
        <WidgetAction label={showTable() ? "Показать график" : "Показать таблицей"} pressed={showTable()} onClick={() => setShowTable(!showTable())}>
          <TableIcon />
        </WidgetAction>
      }
    >
      <Show when={props.spec.type === "parts"} fallback={<Show when={!showTable()} fallback={<ChartTable spec={props.spec} />}><AxisChart spec={props.spec} /></Show>}>
        <PartsChart spec={props.spec} showTable={showTable()} />
        <Show when={showTable()}>
          <ChartTable spec={props.spec} />
        </Show>
      </Show>
    </WidgetCard>
  );
}
