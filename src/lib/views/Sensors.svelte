<script lang="ts">
  import { app } from '$lib/app.svelte';
  import { api } from '$lib/api';
  import { fmt, heat, kindColor } from '$lib/format';
  import Icon from '$lib/components/Icon.svelte';
  import LineChart from '$lib/components/LineChart.svelte';
  import Sparkline from '$lib/components/Sparkline.svelte';
  import type { Reading } from '$lib/types';

  const palette = [
    'var(--saqr-data-series-01)',
    'var(--saqr-data-series-02)',
    'var(--saqr-data-series-03)',
    'var(--saqr-data-series-04)',
    'var(--saqr-data-series-05)',
    'var(--saqr-data-series-06)',
    'var(--saqr-data-series-07)',
    'var(--saqr-data-series-08)',
  ];
  const kinds = [
    { kind: 'Temperature', label: 'Temperatures', unit: '°C' },
    { kind: 'Fan', label: 'Fans', unit: 'rpm' },
    { kind: 'Voltage', label: 'Voltages', unit: 'V' },
  ];

  let showHidden = $state(false);
  let chartKind = $state('Temperature');
  let selected = $state<string[]>(['Temperature/CPU']);

  const hidden = $derived(new Set(app.settings?.hiddenSensors ?? []));
  const pinned = $derived(new Set(app.settings?.pinnedSensors ?? []));
  const readings = $derived(app.sample?.readings ?? []);
  const groups = $derived(
    kinds.map((k) => ({
      ...k,
      rows: readings.filter((r) => r.kind === k.kind && (showHidden || !hidden.has(r.key))),
    })),
  );

  const chartKeys = $derived(selected.filter((k) => k.startsWith(chartKind + '/')));
  const chartSeries = $derived(
    chartKeys.map((key, i) => ({
      key,
      label: key.split('/').slice(1).join('/'),
      color: palette[i % palette.length],
      points: app.series(key),
    })),
  );
  const chartUnit = $derived(kinds.find((k) => k.kind === chartKind)?.unit ?? '');

  function toggleSelect(r: Reading) {
    if (r.kind !== chartKind) {
      chartKind = r.kind;
      selected = [r.key];
      return;
    }
    selected = selected.includes(r.key) ? selected.filter((k) => k !== r.key) : [...selected, r.key];
  }

  function toggleIn(list: 'hiddenSensors' | 'pinnedSensors', key: string) {
    const current = app.settings?.[list] ?? [];
    void app.saveSettings({ [list]: current.includes(key) ? current.filter((k) => k !== key) : [...current, key] });
  }

  async function resetStats() {
    await app.run(() => api.resetStats(), 'Min / max / average reset');
  }

  const digits = (r: Reading) => (r.kind === 'Voltage' ? 3 : 0);
</script>

<header>
  <div>
    <h1>Sensors</h1>
    <p class="muted">{readings.length} readings · polled every {app.settings?.pollMs ?? 1000} ms · {app.settings?.historyMinutes ?? 15} min history</p>
  </div>
  <div class="actions">
    <label class="check">
      <input type="checkbox" bind:checked={showHidden} /> Show hidden
    </label>
    <button class="btn" onclick={resetStats}><Icon name="refresh" size={14} />Reset stats</button>
  </div>
</header>

<div class="card chart-card">
  <div class="chart-head">
    <div class="seg">
      {#each kinds as k (k.kind)}
        <button
          class={{ on: chartKind === k.kind }}
          onclick={() => {
            chartKind = k.kind;
            if (!chartKeys.length) {
              const first = readings.find((r) => r.kind === k.kind);
              if (first) selected = [...selected, first.key];
            }
          }}>{k.label}</button
        >
      {/each}
    </div>
    <span class="faint">Click rows to add or remove them from the chart</span>
  </div>
  {#if chartSeries.length && chartSeries.some((s) => s.points.length > 1)}
    <LineChart series={chartSeries} unit={chartUnit} digits={chartKind === 'Voltage' ? 3 : 0} />
  {:else}
    <div class="chart-empty faint">Collecting data…</div>
  {/if}
</div>

{#each groups as g (g.kind)}
  {#if g.rows.length}
    <h2>{g.label}</h2>
    <div class="card table">
      <div class="thead">
        <span>Sensor</span>
        <span class="r">Now</span>
        <span class="r">Min</span>
        <span class="r">Avg</span>
        <span class="r">Max</span>
        <span>Trend</span>
        <span></span>
      </div>
      {#each g.rows as r (r.key)}
        {@const color = selected.includes(r.key) && r.kind === chartKind ? palette[chartKeys.indexOf(r.key) % palette.length] : null}
        <div class={['trow', { dim: hidden.has(r.key) }]}>
          <button class="name" onclick={() => toggleSelect(r)}>
            <span class="swatch" style:background={color ?? 'transparent'} style:border-color={color ?? 'var(--line-strong)'}></span>
            {r.title}
          </button>
          <span class="r num now" style:color={r.kind === 'Temperature' ? heat(r.value) : undefined}>{fmt(r.value, r.unit, digits(r))}</span>
          <span class="r num muted">{fmt(r.min, r.unit, digits(r))}</span>
          <span class="r num muted">{fmt(r.avg, r.unit, r.kind === 'Voltage' ? 3 : 1)}</span>
          <span class="r num muted">{fmt(r.max, r.unit, digits(r))}</span>
          <span class="spark"><Sparkline points={app.series(r.key).slice(-120)} color={kindColor(r.kind, r.value)} height={22} fill={false} /></span>
          <span class="row-actions">
            <button
              class={['btn ghost sm', { on: pinned.has(r.key) }]}
              aria-label={pinned.has(r.key) ? 'Unpin from overview' : 'Pin to overview'}
              title={pinned.has(r.key) ? 'Unpin from overview' : 'Pin to overview'}
              onclick={() => toggleIn('pinnedSensors', r.key)}><Icon name="pin" size={14} /></button
            >
            <button
              class="btn ghost sm"
              aria-label={hidden.has(r.key) ? 'Show sensor' : 'Hide sensor'}
              title={hidden.has(r.key) ? 'Show sensor' : 'Hide sensor'}
              onclick={() => toggleIn('hiddenSensors', r.key)}><Icon name={hidden.has(r.key) ? 'eye' : 'eyeOff'} size={14} /></button
            >
          </span>
        </div>
      {/each}
    </div>
  {/if}
{/each}

<style>
  header {
    display: flex;
    justify-content: space-between;
    align-items: flex-end;
    margin-bottom: 20px;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 14px;
  }

  .check {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    color: var(--muted);
  }

  .check input {
    accent-color: var(--accent);
  }

  h2 {
    margin: 24px 0 10px;
  }

  .chart-card {
    padding: 12px 14px;
  }

  .chart-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 8px;
    font-size: 12px;
  }

  .chart-empty {
    height: 220px;
    display: grid;
    place-items: center;
  }

  .seg {
    display: inline-flex;
    padding: 2px;
    border-radius: var(--radius-sm);
    background: var(--bg);
    border: 1px solid var(--line-strong);
  }

  .seg button {
    height: 26px;
    padding: 0 12px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--muted);
    font-size: 12.5px;
  }

  .seg button.on {
    background: var(--surface);
    color: var(--text);
    box-shadow: 0 1px 2px var(--saqr-shadow-key);
  }

  .table {
    font-size: 13px;
    overflow: hidden;
  }

  .thead,
  .trow {
    display: grid;
    grid-template-columns: minmax(160px, 1.6fr) 80px 80px 80px 80px minmax(120px, 1.2fr) 72px;
    align-items: center;
    gap: 12px;
    padding: 0 14px;
  }

  .thead {
    height: 32px;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--faint);
    border-bottom: 1px solid var(--line);
  }

  .trow {
    height: 40px;
  }

  .trow + .trow {
    border-top: 1px solid var(--line);
  }

  .trow:hover {
    background: var(--raised);
  }

  .trow.dim {
    opacity: 0.45;
  }

  .r {
    text-align: right;
  }

  .now {
    font-weight: 600;
  }

  .name {
    display: flex;
    align-items: center;
    gap: 10px;
    border: none;
    background: none;
    padding: 0;
    text-align: left;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .swatch {
    width: 10px;
    height: 10px;
    border-radius: 3px;
    border: 1.5px solid;
    flex: none;
  }

  .row-actions {
    display: flex;
    justify-content: flex-end;
    gap: 2px;
  }

  .row-actions .on {
    color: var(--accent);
  }
</style>
