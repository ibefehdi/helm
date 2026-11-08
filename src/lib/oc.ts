import type { OcItem, OcRange } from './types';

const U32 = 2 ** 32;

/** Precision is a divisor: display = raw / precision. */
function precisionOf(item: OcItem): number {
  return item.ranges.find((r) => r.precision > 0)?.precision ?? 1;
}

export function decode(item: OcItem, raw: number): number {
  const p = precisionOf(item);
  if (item.encoding === 'biased') return (raw - item.bias) / p;
  if (item.encoding === 'signed') return (raw > 0x7fffffff ? raw - U32 : raw) / p;
  return raw / p;
}

export function encode(item: OcItem, value: number): number {
  const v = Math.round(value * precisionOf(item));
  if (item.encoding === 'biased') return item.bias + v;
  if (item.encoding === 'signed') return v < 0 ? v + U32 : v;
  return v;
}

function rangeBounds(item: OcItem, r: OcRange): [number, number] {
  const a = decode(item, r.base);
  const b = decode(item, r.max);
  return [Math.min(a, b), Math.max(a, b)];
}

/** Continuous bounds after Helm's own caps (which are stricter than the engine's). */
export function bounds(item: OcItem): { min: number; max: number; step: number } {
  let min = Infinity;
  let max = -Infinity;
  for (const r of item.ranges) {
    const [lo, hi] = rangeBounds(item, r);
    min = Math.min(min, lo);
    max = Math.max(max, hi);
  }
  const l = item.limits;
  if (l?.max != null) max = Math.min(max, l.max);
  if (l?.min != null) min = Math.max(min, l.min);
  if (l?.maxAbs != null) {
    min = Math.max(min, -l.maxAbs);
    max = Math.min(max, l.maxAbs);
  }
  if (l?.maxDeltaFromBoot != null) {
    const boot = decode(item, item.boot);
    min = Math.max(min, boot - l.maxDeltaFromBoot);
    max = Math.min(max, boot + l.maxDeltaFromBoot);
  }
  const r = item.ranges[0];
  const step = r && r.step > 0 ? r.step / precisionOf(item) : 1 / precisionOf(item);
  return { min, max, step };
}

/** Every allowed raw value when the engine exposes a small discrete set (e.g. memory speeds). */
export function discreteValues(item: OcItem): number[] | null {
  if (item.ranges.length < 2) return null;
  const out: number[] = [];
  for (const r of item.ranges) {
    if (r.step <= 0 || r.base === r.max) out.push(r.base);
    else for (let v = r.base; v <= r.max; v += r.step) out.push(v);
    if (out.length > 400) return null;
  }
  return out;
}

export function snap(item: OcItem, value: number): number {
  const { min, max, step } = bounds(item);
  const r = item.ranges[0];
  const origin = r ? decode(item, r.base) : 0;
  const snapped = origin + Math.round((value - origin) / step) * step;
  return Math.min(max, Math.max(min, Number(snapped.toFixed(6))));
}

export function decimals(item: OcItem): number {
  const { step } = bounds(item);
  const s = String(step);
  return s.includes('.') ? s.split('.')[1].length : 0;
}

export function display(item: OcItem, raw: number): string {
  const opt = item.options.find((o) => o.raw === raw);
  if (opt) return opt.label;
  if (item.key === 'memRatio') return `${raw} MT/s`;
  const v = decode(item, raw);
  const d = decimals(item);
  const sign = item.encoding !== 'unsigned' && v > 0 ? '+' : '';
  return `${sign}${v.toFixed(d)}${unitOf(item)}`;
}

export function unitOf(item: OcItem): string {
  if (item.key === 'ratio') return '×';
  if (item.key === 'bclk') return ' MHz';
  return item.unit ? ` ${item.unit}` : '';
}
