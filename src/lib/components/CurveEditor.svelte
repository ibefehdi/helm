<script lang="ts">
  import { CURVE_MAX_POINTS, CURVE_MAX_TEMP, CURVE_MIN_POINTS, PWM_MAX, clamp, pctToPwm, pwmAt, pwmToPct, rpmAt } from '$lib/curve';
  import { heat } from '$lib/format';
  import type { CalibrationPoint, CurvePoint } from '$lib/types';

  let {
    points = $bindable(),
    liveTemp = null,
    calibration,
    stall = null,
    height = 340,
    readonly = false,
  }: {
    points: CurvePoint[];
    liveTemp?: number | null;
    calibration?: CalibrationPoint[];
    stall?: number | null;
    height?: number;
    readonly?: boolean;
  } = $props();

  const PAD = { l: 44, r: 52, t: 14, b: 30 };

  let width = $state(700);
  let dragging = $state<number | null>(null);
  let selected = $state<number | null>(null);
  let hover = $state<{ t: number; pct: number } | null>(null);

  const innerW = $derived(Math.max(10, width - PAD.l - PAD.r));
  const innerH = $derived(height - PAD.t - PAD.b);

  const sx = (t: number) => PAD.l + (t / CURVE_MAX_TEMP) * innerW;
  const sy = (pct: number) => PAD.t + innerH - (pct / 100) * innerH;
  const tx = (x: number) => ((x - PAD.l) / innerW) * CURVE_MAX_TEMP;
  const ty = (y: number) => ((PAD.t + innerH - y) / innerH) * 100;

  /** What the fan actually does: point 0 holds the start duty, then linear segments, then flat at 100%. */
  const effective = $derived.by(() => {
    if (points.length < 2) return [] as [number, number][];
    const out: [number, number][] = [[0, pwmToPct(points[1][1])]];
    out.push([points[0][0], pwmToPct(points[1][1])]);
    for (let i = 1; i < points.length; i++) out.push([points[i][0], pwmToPct(points[i][1])]);
    out.push([CURVE_MAX_TEMP, pwmToPct(points[points.length - 1][1])]);
    return out;
  });

  const line = $derived(effective.map(([t, p], i) => `${i ? 'L' : 'M'}${sx(t).toFixed(1)},${sy(p).toFixed(1)}`).join(''));
  const area = $derived(line ? `${line}L${sx(CURVE_MAX_TEMP)},${sy(0)}L${sx(0)},${sy(0)}Z` : '');

  const live = $derived.by(() => {
    if (liveTemp === null || points.length < 2) return null;
    const t = clamp(liveTemp, 0, CURVE_MAX_TEMP);
    const pct = pwmToPct(pwmAt(points, t));
    return { t, pct, rpm: rpmAt(calibration, pct) };
  });

  const rpmTicks = $derived.by(() => {
    if (!calibration || calibration.length < 2) return [];
    return [0, 25, 50, 75, 100]
      .map((pct) => ({ pct, rpm: rpmAt(calibration, pct) }))
      .filter((r): r is { pct: number; rpm: number } => r.rpm !== null);
  });

  /** Index 0 is the fan-off anchor (locked); the last point must stay at 100%. */
  const isAnchor = (i: number) => i === 0;
  const isLast = (i: number) => i === points.length - 1;
  const canRemove = (i: number) => !readonly && points.length > CURVE_MIN_POINTS && i > 1 && !isLast(i);

  /** Pointer position relative to the chart; handlers that call this are attached to the <svg> itself. */
  function toLocal(e: PointerEvent | MouseEvent) {
    const rect = (e.currentTarget as SVGSVGElement).getBoundingClientRect();
    return { x: e.clientX - rect.left, y: e.clientY - rect.top };
  }

  function move(i: number, t: number, pct: number) {
    const next = points.map((p) => [...p] as CurvePoint);
    const lo = i > 0 ? next[i - 1][0] + 1 : 0;
    const hi = i < next.length - 1 ? next[i + 1][0] - 1 : CURVE_MAX_TEMP;
    next[i][0] = Math.round(clamp(t, lo, hi));
    if (!isLast(i)) next[i][1] = clamp(pctToPwm(Math.round(clamp(pct, 0, 100))), 0, PWM_MAX);
    if (i === 1) next[0][1] = next[1][1];
    points = next;
  }

  function onpointerdown(e: PointerEvent, i: number) {
    if (readonly || isAnchor(i) || e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    selected = i;
    dragging = i;
    (e.currentTarget as SVGGElement).ownerSVGElement?.setPointerCapture(e.pointerId);
  }

  function onpointermove(e: PointerEvent) {
    const { x, y } = toLocal(e);
    const t = tx(x);
    const pct = ty(y);
    hover = t >= 0 && t <= CURVE_MAX_TEMP && pct >= 0 && pct <= 100 ? { t, pct } : null;
    if (dragging !== null) move(dragging, t, pct);
  }

  function onpointerup(e: PointerEvent) {
    if (dragging !== null) (e.currentTarget as SVGSVGElement).releasePointerCapture(e.pointerId);
    dragging = null;
  }

  function ondblclick(e: MouseEvent) {
    if (readonly || points.length >= CURVE_MAX_POINTS) return;
    const { x, y } = toLocal(e);
    const t = Math.round(tx(x));
    const pct = ty(y);
    // New points can only go between the start point and the full-speed point.
    const at = points.findIndex(([px]) => px > t);
    if (at <= 1 || points.some(([px]) => px === t)) return;
    const next = points.map((p) => [...p] as CurvePoint);
    next.splice(at, 0, [t, pctToPwm(clamp(Math.round(pct), 0, 100))]);
    points = next;
    selected = at;
  }

  function remove(i: number) {
    if (!canRemove(i)) return;
    points = points.filter((_, j) => j !== i);
    selected = null;
  }

  function onkeydown(e: KeyboardEvent, i: number) {
    if (readonly || isAnchor(i)) return;
    const step = e.shiftKey ? 5 : 1;
    const [t, pwm] = points[i];
    const pct = Math.round(pwmToPct(pwm));
    const moves: Record<string, [number, number]> = {
      ArrowLeft: [t - step, pct],
      ArrowRight: [t + step, pct],
      ArrowUp: [t, pct + step],
      ArrowDown: [t, pct - step],
    };
    if (moves[e.key]) {
      e.preventDefault();
      move(i, ...moves[e.key]);
    } else if (e.key === 'Delete' || e.key === 'Backspace') {
      e.preventDefault();
      remove(i);
    }
  }
</script>

<div class="editor" bind:clientWidth={width}>
  <svg
    {width}
    {height}
    role="application"
    aria-label="Fan curve editor. Double-click to add a point, drag to move, Delete to remove."
    {onpointermove}
    {onpointerup}
    onpointerleave={() => (hover = null)}
    {ondblclick}
    class={{ dragging: dragging !== null }}
  >
    <defs>
      <linearGradient id="heat" x1="0" x2="1" y1="0" y2="0">
        {#each [0, 25, 50, 70, 82, 92, 100] as t (t)}
          <stop offset="{t}%" stop-color={heat(t)} />
        {/each}
      </linearGradient>
    </defs>

    {#if stall !== null}
      <rect x={PAD.l} y={sy(stall)} width={innerW} height={sy(0) - sy(stall)} class="stall" />
      <text x={PAD.l + 6} y={sy(stall) - 5} class="zone-label">fan stops below {stall}%</text>
    {/if}

    {#each [0, 20, 40, 60, 80, 100] as pct (pct)}
      <line x1={PAD.l} x2={PAD.l + innerW} y1={sy(pct)} y2={sy(pct)} class="grid" />
      <text x={PAD.l - 8} y={sy(pct) + 4} text-anchor="end" class="tick">{pct}%</text>
    {/each}
    {#each [0, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100] as t (t)}
      <line x1={sx(t)} x2={sx(t)} y1={PAD.t} y2={PAD.t + innerH} class="grid v" />
      <text x={sx(t)} y={height - 10} text-anchor="middle" class="tick">{t}°</text>
    {/each}
    <rect x={PAD.l} y={PAD.t + innerH + 1} width={innerW} height="3" fill="url(#heat)" opacity="0.7" rx="1.5" />

    {#each rpmTicks as r (r.pct)}
      <text x={PAD.l + innerW + 8} y={sy(r.pct) + 4} class="tick rpm">{Math.round(r.rpm)}</text>
    {/each}
    {#if rpmTicks.length}
      <text x={PAD.l + innerW + 8} y={PAD.t - 2} class="tick rpm">rpm</text>
    {/if}

    <path d={area} class="area" />
    <path d={line} class="line" />

    {#if live}
      <line x1={sx(live.t)} x2={sx(live.t)} y1={PAD.t} y2={PAD.t + innerH} class="live-line" style:stroke={heat(live.t)} />
      <circle cx={sx(live.t)} cy={sy(live.pct)} r="9" class="live-halo" style:fill={heat(live.t)} />
      <circle cx={sx(live.t)} cy={sy(live.pct)} r="4" style:fill={heat(live.t)} />
    {/if}

    {#each points as [t, pwm], i (i)}
      {@const pct = pwmToPct(i === 0 && points.length > 1 ? points[1][1] : pwm)}
      {#if isAnchor(i)}
        <circle cx={sx(t)} cy={sy(pct)} r="3.5" class="anchor">
          <title>Fan-off limit: {t}°C (locked)</title>
        </circle>
      {:else}
        <g
          class={['pt', { selected: selected === i, 'locked-y': isLast(i) }]}
          role="slider"
          tabindex={readonly ? -1 : 0}
          aria-label="Point {i}: {t}°C at {Math.round(pct)}%"
          aria-valuenow={Math.round(pct)}
          onpointerdown={(e) => onpointerdown(e, i)}
          onkeydown={(e) => onkeydown(e, i)}
          onfocus={() => (selected = i)}
          oncontextmenu={(e) => {
            e.preventDefault();
            remove(i);
          }}
        >
          <circle cx={sx(t)} cy={sy(pct)} r="14" class="hit" />
          <circle cx={sx(t)} cy={sy(pct)} r="6" class="dot" />
        </g>
      {/if}
    {/each}

    {#if selected !== null && points[selected]}
      {@const [st, spwm] = points[selected]}
      {@const label = `${st}°C · ${Math.round(pwmToPct(spwm))}%`}
      {@const lx = clamp(sx(st), PAD.l + 40, PAD.l + innerW - 40)}
      <g class="badge" transform="translate({lx}, {Math.max(sy(pwmToPct(spwm)) - 22, PAD.t + 8)})">
        <rect x="-40" y="-11" width="80" height="20" rx="5" />
        <text text-anchor="middle" y="3">{label}</text>
      </g>
    {:else if hover && !readonly && dragging === null}
      <text x={PAD.l + innerW - 4} y={PAD.t + 12} text-anchor="end" class="tick">
        {Math.round(hover.t)}°C · {Math.round(hover.pct)}%
      </text>
    {/if}
  </svg>
</div>

<style>
  .editor {
    width: 100%;
    position: relative;
  }

  svg {
    display: block;
    touch-action: none;
  }

  svg.dragging {
    cursor: grabbing;
  }

  .grid {
    stroke: var(--line);
  }

  .grid.v {
    stroke-dasharray: 2 4;
  }

  .tick {
    fill: var(--faint);
    font-size: 10.5px;
    font-family: var(--mono);
  }

  .tick.rpm {
    fill: color-mix(in srgb, var(--accent) 70%, var(--faint));
  }

  .stall {
    fill: var(--danger);
    opacity: 0.06;
  }

  .zone-label {
    fill: var(--danger);
    opacity: 0.7;
    font-size: 10.5px;
  }

  .area {
    fill: var(--accent);
    opacity: 0.07;
  }

  .line {
    fill: none;
    stroke: var(--accent);
    stroke-width: 2;
    stroke-linejoin: round;
  }

  .live-line {
    stroke-width: 1;
    stroke-dasharray: 4 3;
    opacity: 0.7;
  }

  .live-halo {
    opacity: 0.2;
  }

  .anchor {
    fill: var(--bg);
    stroke: var(--faint);
    stroke-width: 1.5;
  }

  .pt {
    cursor: grab;
    outline: none;
  }

  .pt.locked-y {
    cursor: ew-resize;
  }

  .hit {
    fill: transparent;
  }

  .dot {
    fill: var(--bg);
    stroke: var(--accent);
    stroke-width: 2;
    transition: r 0.1s;
  }

  .pt:hover .dot,
  .pt.selected .dot {
    fill: var(--accent);
  }

  .pt:focus-visible .dot {
    stroke: var(--text);
  }

  .badge rect {
    fill: var(--raised);
    stroke: var(--line-strong);
  }

  .badge text {
    fill: var(--text);
    font-size: 11px;
    font-family: var(--mono);
  }
</style>
