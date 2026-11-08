<script lang="ts" module>
  export interface ChartSeries {
    key: string;
    label: string;
    color: string;
    points: [number, number][];
  }
</script>

<script lang="ts">
  let {
    series,
    unit = '',
    height = 220,
    digits = 0,
  }: { series: ChartSeries[]; unit?: string; height?: number; digits?: number } = $props();

  const PAD = { l: 44, r: 12, t: 10, b: 22 };

  let width = $state(600);
  let hoverX = $state<number | null>(null);

  const bounds = $derived.by(() => {
    let t0 = Infinity;
    let t1 = -Infinity;
    let lo = Infinity;
    let hi = -Infinity;
    for (const s of series) {
      for (const [t, v] of s.points) {
        t0 = Math.min(t0, t);
        t1 = Math.max(t1, t);
        lo = Math.min(lo, v);
        hi = Math.max(hi, v);
      }
    }
    if (!Number.isFinite(t0)) return null;
    const pad = (hi - lo) * 0.1 || Math.max(Math.abs(hi) * 0.05, 1);
    return { t0, t1: t1 === t0 ? t0 + 1 : t1, lo: lo - pad, hi: hi + pad };
  });

  const innerW = $derived(Math.max(10, width - PAD.l - PAD.r));
  const innerH = $derived(height - PAD.t - PAD.b);

  const sx = (t: number) => (bounds ? PAD.l + ((t - bounds.t0) / (bounds.t1 - bounds.t0)) * innerW : 0);
  const sy = (v: number) => (bounds ? PAD.t + innerH - ((v - bounds.lo) / (bounds.hi - bounds.lo)) * innerH : 0);

  const paths = $derived(
    series.map((s) => ({
      ...s,
      d: s.points.map(([t, v], i) => `${i ? 'L' : 'M'}${sx(t).toFixed(1)},${sy(v).toFixed(1)}`).join(''),
    })),
  );

  const yTicks = $derived.by(() => {
    if (!bounds) return [];
    const step = niceStep((bounds.hi - bounds.lo) / 4);
    const ticks: number[] = [];
    for (let v = Math.ceil(bounds.lo / step) * step; v <= bounds.hi; v += step) ticks.push(v);
    return ticks;
  });

  const xTicks = $derived.by(() => {
    if (!bounds) return [];
    const count = Math.max(2, Math.floor(innerW / 110));
    return Array.from({ length: count + 1 }, (_, i) => bounds.t0 + ((bounds.t1 - bounds.t0) * i) / count);
  });

  const hover = $derived.by(() => {
    if (hoverX === null || !bounds) return null;
    const t = bounds.t0 + ((hoverX - PAD.l) / innerW) * (bounds.t1 - bounds.t0);
    const values = series.map((s) => {
      let best: [number, number] | null = null;
      for (const p of s.points) if (!best || Math.abs(p[0] - t) < Math.abs(best[0] - t)) best = p;
      return { label: s.label, color: s.color, point: best };
    });
    const anchor = values.find((v) => v.point)?.point;
    return anchor ? { x: sx(anchor[0]), t: anchor[0], values } : null;
  });

  function niceStep(raw: number) {
    const mag = 10 ** Math.floor(Math.log10(raw || 1));
    const n = raw / mag;
    return (n < 1.5 ? 1 : n < 3 ? 2 : n < 7 ? 5 : 10) * mag;
  }

  const clock = (t: number) => new Date(t).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });

  function onmove(e: PointerEvent) {
    const rect = (e.currentTarget as SVGElement).getBoundingClientRect();
    const x = e.clientX - rect.left;
    hoverX = x >= PAD.l && x <= width - PAD.r ? x : null;
  }
</script>

<div class="chart" bind:clientWidth={width}>
  <svg {width} {height} role="img" aria-label="History chart" onpointermove={onmove} onpointerleave={() => (hoverX = null)}>
    {#each yTicks as v (v)}
      <line x1={PAD.l} x2={width - PAD.r} y1={sy(v)} y2={sy(v)} class="grid" />
      <text x={PAD.l - 8} y={sy(v) + 4} text-anchor="end" class="tick">{v.toFixed(Math.max(0, digits))}</text>
    {/each}
    {#each xTicks as t, i (i)}
      <text x={sx(t)} y={height - 6} text-anchor={i === 0 ? 'start' : i === xTicks.length - 1 ? 'end' : 'middle'} class="tick">
        {clock(t)}
      </text>
    {/each}
    {#each paths as p (p.key)}
      <path d={p.d} stroke={p.color} fill="none" stroke-width="1.6" stroke-linejoin="round" />
    {/each}
    {#if hover}
      <line x1={hover.x} x2={hover.x} y1={PAD.t} y2={PAD.t + innerH} class="cursor" />
      {#each hover.values as v (v.label)}
        {#if v.point}
          <circle cx={sx(v.point[0])} cy={sy(v.point[1])} r="3" fill={v.color} />
        {/if}
      {/each}
    {/if}
  </svg>
  {#if hover}
    <div class="tip" style:left="{Math.min(hover.x + 12, width - 180)}px">
      <div class="faint num">{new Date(hover.t).toLocaleTimeString()}</div>
      {#each hover.values as v (v.label)}
        <div class="row">
          <span class="dot" style:background={v.color}></span>
          <span class="label">{v.label}</span>
          <span class="num">{v.point ? v.point[1].toFixed(digits) : '—'} {unit}</span>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .chart {
    position: relative;
    width: 100%;
  }

  svg {
    display: block;
  }

  .grid {
    stroke: var(--saqr-data-grid);
    stroke-width: 1;
  }

  .tick {
    fill: var(--saqr-data-axis);
    font-size: 10.5px;
    font-family: var(--mono);
  }

  .cursor {
    stroke: var(--line-strong);
    stroke-dasharray: 3 3;
  }

  .tip {
    position: absolute;
    top: 8px;
    min-width: 160px;
    padding: 8px 10px;
    border-radius: var(--saqr-radius-md);
    background: var(--saqr-data-tooltip-bg);
    color: var(--saqr-data-tooltip-content);
    border: 1px solid transparent;
    font-size: 12px;
    pointer-events: none;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .tip .label,
  .tip .faint {
    color: var(--saqr-data-tooltip-content);
  }

  .row {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .label {
    flex: 1;
    color: var(--muted);
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }
</style>
