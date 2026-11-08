export function fmt(value: number | null | undefined, unit: string, digits?: number): string {
  if (value === null || value === undefined || Number.isNaN(value)) return '—';
  const d = digits ?? (unit === 'V' ? 3 : 0);
  return value.toFixed(d);
}

export function unitLabel(unit: string): string {
  return unit === 'RPM' ? 'rpm' : unit;
}

/** Colour for a temperature on a cool→hot scale. */
export function heat(celsius: number | null | undefined): string {
  if (celsius === null || celsius === undefined) return 'var(--muted)';
  const stops: [number, [number, number, number]][] = [
    [25, [78, 93, 72]],
    [50, [45, 202, 114]],
    [70, [255, 194, 51]],
    [82, [172, 113, 86]],
    [92, [239, 68, 68]],
  ];
  if (celsius <= stops[0][0]) return rgb(stops[0][1]);
  for (let i = 1; i < stops.length; i++) {
    const [t1, c1] = stops[i];
    if (celsius <= t1) {
      const [t0, c0] = stops[i - 1];
      const k = (celsius - t0) / (t1 - t0);
      return rgb([0, 1, 2].map((j) => c0[j] + (c1[j] - c0[j]) * k) as [number, number, number]);
    }
  }
  return rgb(stops[stops.length - 1][1]);
}

function rgb([r, g, b]: [number, number, number]) {
  return `rgb(${Math.round(r)} ${Math.round(g)} ${Math.round(b)})`;
}

export function kindColor(kind: string, value?: number | null): string {
  if (kind === 'Temperature') return heat(value);
  if (kind === 'Fan') return 'var(--accent)';
  return 'var(--violet)';
}

export function timeAgo(ms: number): string {
  const s = Math.round((Date.now() - ms) / 1000);
  if (s < 60) return 'just now';
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  if (h < 48) return `${h} h ago`;
  return new Date(ms).toLocaleDateString();
}
