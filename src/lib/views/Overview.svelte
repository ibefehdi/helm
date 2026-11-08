<script lang="ts">
  import { app } from '$lib/app.svelte';
  import { pwmAt, pwmToPct, rpmAt } from '$lib/curve';
  import { fmt, heat } from '$lib/format';
  import Sparkline from '$lib/components/Sparkline.svelte';
  import Icon from '$lib/components/Icon.svelte';
  import type { Reading } from '$lib/types';

  const temps = $derived(app.sample?.readings.filter((r) => r.kind === 'Temperature') ?? []);
  const volts = $derived(app.sample?.readings.filter((r) => r.kind === 'Voltage') ?? []);
  const pinned = $derived(
    (app.settings?.pinnedSensors ?? []).map((k) => app.readings[k]).filter((r): r is Reading => !!r),
  );

  const cpu = $derived(app.readings['Temperature/CPU'] ?? temps[0] ?? null);
  const hottest = $derived(
    temps.reduce<Reading | null>((a, r) => (r.value !== null && (!a || r.value > (a.value ?? -Infinity)) ? r : a), null),
  );
  const totalRpm = $derived((app.sample?.headerRpms ?? []).reduce((a, b) => a + (b > 0 ? b : 0), 0));
  const spinning = $derived((app.sample?.headerRpms ?? []).filter((r) => r > 0).length);

  const fanCards = $derived(
    (app.info?.fans ?? []).map((h) => {
      const curve = app.fans[h.index];
      const temp = app.headerTemperature(h.index);
      const rpm = app.headerRpm(h.index);
      const forced = app.overrides.failsafe ? 100 : app.overrides.manual[String(h.index)];
      const target = forced ?? (curve && temp !== null ? pwmToPct(pwmAt(curve.points, temp)) : null);
      const cal = app.settings?.calibration[String(h.index)];
      return {
        index: h.index,
        title: app.fanTitle(h.index),
        source: app.sourceTitle(curve?.temperatureSource ?? h.temperatureSource),
        temp,
        rpm,
        target,
        forced: forced !== undefined,
        expected: target !== null ? rpmAt(cal, target) : null,
        curve,
      };
    }),
  );

  function miniPath(points: [number, number][]) {
    if (points.length < 2) return '';
    const pts = [[0, points[1][1]], ...points.slice(1), [100, points[points.length - 1][1]]];
    return pts.map(([t, pwm], i) => `${i ? 'L' : 'M'}${t},${40 - (pwm / 255) * 40}`).join('');
  }
</script>

<header>
  <div>
    <h1>Overview</h1>
    <p class="muted">Live readings from the onboard controller{app.info ? ` · ${app.info.fanCount} headers` : ''}</p>
  </div>
  {#if app.profiles.length}
    <div class="quick">
      {#each app.profiles.slice(0, 5) as p (p.id)}
        <button class={['btn sm', { primary: app.settings?.activeProfile === p.id }]} onclick={() => app.applyProfile(p.id)}>
          {p.name}
        </button>
      {/each}
    </div>
  {/if}
</header>

{#if Object.keys(app.activeAlerts).length}
  <div class="alert-strip">
    <Icon name="warn" />
    {#each Object.values(app.activeAlerts) as a (a.id)}
      <span><b>{a.sensor.split('/')[1]}</b> {fmt(a.value, '')} — {a.action === 'fullSpeed' ? 'fans at full speed' : 'threshold crossed'}</span>
    {/each}
  </div>
{/if}

<section class="hero">
  <div class="card stat big">
    <span class="label">CPU</span>
    <div class="value num" style:color={heat(cpu?.value)}>
      {fmt(cpu?.value, '°C')}<small>°C</small>
    </div>
    <div class="range faint num">min {fmt(cpu?.min, '°C')} · max {fmt(cpu?.max, '°C')}</div>
    {#if cpu}
      <Sparkline points={app.series(cpu.key)} color={heat(cpu.value)} height={44} />
    {/if}
  </div>
  <div class="card stat">
    <span class="label">Hottest sensor</span>
    <div class="value num" style:color={heat(hottest?.value)}>{fmt(hottest?.value, '°C')}<small>°C</small></div>
    <div class="range muted">{hottest?.title ?? '—'}</div>
  </div>
  <div class="card stat">
    <span class="label">Airflow</span>
    <div class="value num">{fmt(totalRpm, 'RPM')}<small>rpm</small></div>
    <div class="range muted">{spinning} of {app.info?.fanCount ?? 0} headers spinning</div>
  </div>
  {#each pinned as r (r.key)}
    <div class="card stat">
      <span class="label">{r.title}</span>
      <div class="value num" style:color={r.kind === 'Temperature' ? heat(r.value) : undefined}>
        {fmt(r.value, r.unit)}<small>{r.unit}</small>
      </div>
      <Sparkline points={app.series(r.key)} height={28} />
    </div>
  {/each}
</section>

<h2>Fans</h2>
<section class="fans">
  {#each fanCards as f (f.index)}
    <button class="card fan" onclick={() => app.openFan(f.index)}>
      <div class="fan-head">
        <span class="title">{f.title}</span>
        {#if f.forced}
          <span class="chip warn">{app.overrides.failsafe ? 'failsafe' : 'forced'}</span>
        {:else if f.rpm === 0}
          <span class="chip">idle</span>
        {/if}
      </div>
      <div class="fan-body">
        <div>
          <div class="rpm num">{fmt(f.rpm, 'RPM')}<small>rpm</small></div>
          <div class="sub muted">
            <span class="num">{f.target !== null ? `${Math.round(f.target)}%` : '—'}</span> duty
            {#if f.expected}<span class="faint">· expect ~{Math.round(f.expected)} rpm</span>{/if}
          </div>
        </div>
        {#if f.curve}
          <svg viewBox="0 0 100 40" preserveAspectRatio="none" class="mini" aria-hidden="true">
            <path d={miniPath(f.curve.points)} />
            {#if f.temp !== null && f.target !== null}
              <circle cx={Math.min(100, f.temp)} cy={40 - (f.target / 100) * 40} r="2.5" style:fill={heat(f.temp)} />
            {/if}
          </svg>
        {/if}
      </div>
      <div class="fan-foot">
        <span class="faint">{f.source}</span>
        <span class="num" style:color={heat(f.temp)}>{fmt(f.temp, '°C')}°C</span>
      </div>
    </button>
  {/each}
</section>

<div class="split">
  <section>
    <h2>Temperatures</h2>
    <div class="card list">
      {#each temps as r (r.key)}
        <div class="row">
          <span class="name">{r.title}</span>
          <div class="bar"><span style:width="{Math.min(100, r.value ?? 0)}%" style:background={heat(r.value)}></span></div>
          <span class="num val" style:color={heat(r.value)}>{fmt(r.value, '°C')}°</span>
        </div>
      {/each}
    </div>
  </section>
  <section>
    <h2>Voltages</h2>
    <div class="card list">
      {#each volts as r (r.key)}
        <div class="row">
          <span class="name">{r.title}</span>
          <span class="faint num range-v">{fmt(r.min, 'V')}–{fmt(r.max, 'V')}</span>
          <span class="num val">{fmt(r.value, 'V')} V</span>
        </div>
      {/each}
    </div>
  </section>
</div>

<style>
  header {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 20px;
  }

  .quick {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
    justify-content: flex-end;
  }

  h2 {
    margin: 24px 0 10px;
  }

  .alert-strip {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px 16px;
    padding: 10px 14px;
    margin-bottom: 16px;
    border-radius: var(--radius);
    color: var(--danger);
    background: color-mix(in srgb, var(--danger) 9%, transparent);
    border: 1px solid color-mix(in srgb, var(--danger) 30%, transparent);
    font-size: 13px;
  }

  .alert-strip span {
    color: var(--text);
  }

  .hero {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
    gap: 12px;
  }

  .stat {
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-height: 116px;
  }

  .stat.big {
    grid-column: span 2;
  }

  .label {
    font-size: 12px;
    color: var(--muted);
    font-weight: 500;
  }

  .value {
    font-size: 30px;
    font-weight: 600;
    line-height: 1.1;
    letter-spacing: -0.02em;
  }

  .big .value {
    font-size: 40px;
  }

  .value small,
  .rpm small {
    font-size: 0.42em;
    margin-left: 3px;
    color: var(--muted);
    font-weight: 500;
  }

  .range {
    font-size: 12px;
    margin-bottom: auto;
  }

  .fans {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(230px, 1fr));
    gap: 12px;
  }

  .fan {
    text-align: left;
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    transition: border-color 0.12s;
  }

  .fan:hover {
    border-color: var(--line-strong);
    background: var(--raised);
  }

  .fan-head,
  .fan-foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .title {
    font-weight: 600;
  }

  .fan-body {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 12px;
  }

  .rpm {
    font-size: 26px;
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  .sub {
    font-size: 12px;
  }

  .mini {
    width: 84px;
    height: 40px;
    flex: none;
    overflow: visible;
  }

  .mini path {
    fill: none;
    stroke: var(--accent);
    stroke-width: 1.5;
    vector-effect: non-scaling-stroke;
    opacity: 0.8;
  }

  .fan-foot {
    font-size: 12px;
    border-top: 1px solid var(--line);
    padding-top: 8px;
  }

  .split {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 16px;
  }

  .list {
    padding: 6px 0;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 7px 16px;
    font-size: 13px;
  }

  .row + .row {
    border-top: 1px solid var(--line);
  }

  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .bar {
    width: 38%;
    height: 4px;
    border-radius: 2px;
    background: var(--raised);
    overflow: hidden;
  }

  .bar span {
    display: block;
    height: 100%;
    border-radius: 2px;
    transition: width 0.4s;
  }

  .val {
    width: 72px;
    text-align: right;
  }

  .range-v {
    font-size: 11.5px;
  }

  @media (max-width: 1100px) {
    .split {
      grid-template-columns: 1fr;
    }
  }
</style>
