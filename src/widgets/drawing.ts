const SVG_OPEN_TAG = /<svg\b[^>]*>/i;
const VIEW_BOX = /viewBox\s*=\s*["']([^"']+)["']/i;
const SVG_NAMESPACE = 'xmlns="http://www.w3.org/2000/svg"';
const XLINK_NAMESPACE = 'xmlns:xlink="http://www.w3.org/1999/xlink"';
const PAPER_CELL = 20;

const ROOT_DEFAULTS = `
:root:not([fill]) { fill: var(--text); }
:root:not([font-family]) { font-family: var(--font); }
:root { color: var(--text); }
`;

export interface ViewBox {
  x: number;
  y: number;
  width: number;
  height: number;
}

export function readViewBox(svg: string): ViewBox | null {
  const openTag = svg.match(SVG_OPEN_TAG)?.[0] ?? "";
  const numbers = openTag.match(VIEW_BOX)?.[1].trim().split(/[\s,]+/).map(Number) ?? [];
  if (numbers.length !== 4 || numbers.some((number) => !Number.isFinite(number))) {
    return null;
  }
  const [x, y, width, height] = numbers;
  return width > 0 && height > 0 ? { x, y, width, height } : null;
}

function withNamespaces(openTag: string, svg: string): string {
  let tag = openTag;
  if (!/\sxmlns\s*=/.test(tag)) {
    tag = tag.replace(/^<svg/i, `<svg ${SVG_NAMESPACE}`);
  }
  if (svg.includes("xlink:") && !/\sxmlns:xlink\s*=/.test(tag)) {
    tag = tag.replace(/^<svg/i, `<svg ${XLINK_NAMESPACE}`);
  }
  return tag;
}

function paperLayer(box: ViewBox): string {
  const pattern = `<pattern id="tarn-paper-cells" width="${PAPER_CELL}" height="${PAPER_CELL}" patternUnits="userSpaceOnUse"><path d="M${PAPER_CELL} 0H0V${PAPER_CELL}" fill="none" style="stroke: var(--paper-line)" stroke-width="1"/></pattern>`;
  const cover = `<rect x="${box.x}" y="${box.y}" width="${box.width}" height="${box.height}" fill="url(#tarn-paper-cells)"/>`;
  return `<defs>${pattern}</defs>${cover}`;
}

export function themedSvg(svg: string, tokensCss: string, paper: boolean): string {
  const openTag = svg.match(SVG_OPEN_TAG);
  if (!openTag || openTag.index === undefined) {
    return svg;
  }
  const box = readViewBox(svg);
  const style = `<style>:root {${tokensCss}}${ROOT_DEFAULTS}</style>`;
  const underlay = paper && box ? paperLayer(box) : "";
  const before = svg.slice(0, openTag.index);
  const after = svg.slice(openTag.index + openTag[0].length);
  return `${before}${withNamespaces(openTag[0], svg)}${style}${underlay}${after}`;
}

export function svgDataUrl(svg: string): string {
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}
