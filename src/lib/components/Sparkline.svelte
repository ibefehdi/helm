<script lang="ts">
  let {
    points,
    color = 'var(--accent)',
    height = 36,
    fill = true,
  }: { points: [number, number][]; color?: string; height?: number; fill?: boolean } = $props();

  const W = 100;

  const shape = $derived.by(() => {
    if (points.length < 2) return null;
    const t0 = points[0][0];
    const t1 = points[points.length - 1][0];
    let lo = Infinity;
    let hi = -Infinity;
    for (const [, v] of points) {
      lo = Math.min(lo, v);
      hi = Math.max(hi, v);
    }
    const pad = (hi - lo) * 0.15 || Math.max(Math.abs(hi) * 0.05, 1);
    lo -= pad;
    hi += pad;
    const span = t1 - t0 || 1;
    const line = points
      .map(([t, v], i) => `${i ? 'L' : 'M'}${(((t - t0) / span) * W).toFixed(2)},${(height - ((v - lo) / (hi - lo)) * height).toFixed(2)}`)
      .join('');
    return { line, area: `${line}L${W},${height}L0,${height}Z` };
  });
</script>

<svg viewBox="0 0 {W} {height}" preserveAspectRatio="none" style:height="{height}px" aria-hidden="true">
  {#if shape}
    {#if fill}
      <path d={shape.area} fill={color} opacity="0.08" />
    {/if}
    <path d={shape.line} fill="none" stroke={color} stroke-width="1.5" vector-effect="non-scaling-stroke" />
  {/if}
</svg>

<style>
  svg {
    display: block;
    width: 100%;
    overflow: visible;
  }
</style>
