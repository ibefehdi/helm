<script lang="ts">
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { app } from '$lib/app.svelte';
  import { api, errorMessage } from '$lib/api';
  import { fmt, heat } from '$lib/format';
  import Icon from '$lib/components/Icon.svelte';
  import Sparkline from '$lib/components/Sparkline.svelte';
  import Toggle from '$lib/components/Toggle.svelte';
  import type { GpuControl, GpuDevice, GpuSnapshot } from '$lib/types';

  const ACK_KEY = 'helm.graphics.acknowledged';
  const CONFIRM_SECS = 20;

  let gpu = $state<GpuSnapshot | null>(null);
  let loadError = $state<string | null>(null);
  let acknowledged = $state(false);
  let applying = $state(false);
  let countdown = $state<number | null>(null);
  let seeded = $state(false);

  let core = $state(0);
  let memory = $state(0);
  let power = $state(0);
  let fanMode = $state<GpuControl['fanMode']>('auto');
  let fanPercent = $state(40);
  let fanCurve = $state<[number, number][]>([
    [45, 30],
    [60, 45],
    [72, 70],
    [82, 100],
  ]);
  let applyOnStart = $state(false);
  let osd = $state(false);
  let timer: ReturnType<typeof setInterval> | undefined;

  const dev = $derived(gpu?.devices[0] ?? null);
  const saved = $derived(app.settings?.gpu ?? null);
  const clocksSupported = $derived(dev?.coreOffset != null && dev?.memoryOffset != null);
  const powerSupported = $derived(dev?.powerMinW != null && dev?.powerMaxW != null && dev.powerMinW < dev.powerMaxW);
  const fanModes = ['auto', 'manual', 'curve'] as const;
  const dirty = $derived.by(() => {
    if (!dev || !saved) return false;
    const powerChanged = powerSupported && Math.round(power) !== Math.round(dev.powerLimitW ?? power);
    const fanChanged =
      fanMode !== saved.fanMode ||
      (fanMode === 'manual' && fanPercent !== saved.fanPercent) ||
      (fanMode === 'curve' && JSON.stringify(fanCurve) !== JSON.stringify(saved.fanCurve));
    return (
      core !== (dev.coreOffset ?? 0) ||
      memory !== (dev.memoryOffset ?? 0) ||
      powerChanged ||
      fanChanged ||
      applyOnStart !== saved.applyOnStart ||
      osd !== saved.osd
    );
  });
  const series = $derived.by(() => {
    const rows = gpu?.history ?? [];
    const pick = (key: 'temperature' | 'powerW' | 'utilization') =>
      rows.filter((p) => p[key] != null).map((p) => [p.t, p[key]!] as [number, number]);
    return { temp: pick('temperature'), power: pick('powerW'), util: pick('utilization') };
  });

  function seed(device: GpuDevice, control: GpuControl) {
    core = device.coreOffset ?? control.coreOffset;
    memory = device.memoryOffset ?? control.memoryOffset;
    power = device.powerLimitW ?? control.powerLimitW ?? device.powerDefaultW ?? 0;
    fanMode = control.fanMode;
    fanPercent = control.fanPercent;
    fanCurve = control.fanCurve.map((p) => [p[0], p[1]]);
    applyOnStart = control.applyOnStart;
    osd = control.osd;
    seeded = true;
  }

  function clampOffset(value: number, min: number | null, max: number | null) {
    const lo = min ?? value;
    const hi = max ?? value;
    return Math.round(Math.min(hi, Math.max(lo, value)));
  }

  function startCountdown(secs: number) {
    countdown = secs;
    clearInterval(timer);
    timer = setInterval(() => {
      if (countdown !== null && countdown > 0) countdown--;
    }, 1000);
  }

  function stopCountdown() {
    countdown = null;
    clearInterval(timer);
  }

  function acknowledge() {
    acknowledged = true;
    localStorage.setItem(ACK_KEY, '1');
  }

  async function apply() {
    if (!dev || applying) return;
    applying = true;
    const result = await app.run(() =>
      api.gpuApply({
        index: dev.index,
        coreOffset: core,
        memoryOffset: memory,
        powerLimitW: powerSupported ? power : null,
        tempLimitC: null,
        fanMode,
        fanPercent,
        fanCurve,
        applyOnStart,
        osd,
      }),
    );
    applying = false;
    if (!result) return;
    gpu = result;
    if (result.pendingConfirm) startCountdown(CONFIRM_SECS);
    else app.toast('success', 'Graphics settings applied');
  }

  async function keep() {
    const kept = await app.run(() => api.gpuConfirm());
    stopCountdown();
    if (kept) app.toast('success', 'Keeping the new clock offsets');
    else app.toast('warn', 'Those offsets were already reverted');
  }

  async function revert() {
    const snap = await app.run(() => api.gpuRestore());
    stopCountdown();
    if (snap?.devices[0] && saved) {
      core = snap.devices[0].coreOffset ?? core;
      memory = snap.devices[0].memoryOffset ?? memory;
    }
  }

  function setPoint(index: number, axis: 0 | 1, value: number) {
    fanCurve = fanCurve.map((point, i) => {
      if (i !== index) return point;
      const next: [number, number] = [point[0], point[1]];
      next[axis] = value;
      return next;
    });
  }

  function addPoint() {
    if (fanCurve.length >= 8) return;
    const last = fanCurve[fanCurve.length - 1] ?? [70, 70];
    fanCurve = [...fanCurve, [Math.min(110, last[0] + 5), last[1]]];
  }

  function removePoint(index: number) {
    if (fanCurve.length <= 2) return;
    fanCurve = fanCurve.filter((_, i) => i !== index);
  }

  function gib(bytes: number | null) {
    if (bytes == null) return '—';
    return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  }

  onMount(() => {
    acknowledged = localStorage.getItem(ACK_KEY) === '1';
    let stopGpu = () => {};
    let stopRevert = () => {};
    void listen<GpuSnapshot>('gpu', (e) => {
      gpu = e.payload;
      if (!seeded && e.payload.devices[0] && app.settings) seed(e.payload.devices[0], app.settings.gpu);
      if (e.payload.pendingConfirm && countdown === null) startCountdown(CONFIRM_SECS);
      if (!e.payload.pendingConfirm && countdown !== null) stopCountdown();
    }).then((unlisten) => (stopGpu = unlisten));
    void listen<{ ok: boolean }>('gpu-reverted', (e) => {
      stopCountdown();
      app.toast(e.payload.ok ? 'warn' : 'error', e.payload.ok ? 'Clock offsets were reverted' : 'Automatic revert failed');
    }).then((unlisten) => (stopRevert = unlisten));
    void api
      .gpuGet()
      .then((snap) => {
        gpu = snap;
        if (snap.devices[0] && app.settings) seed(snap.devices[0], app.settings.gpu);
        if (snap.pendingConfirm) startCountdown(CONFIRM_SECS);
      })
      .catch((e) => (loadError = errorMessage(e)));
    return () => {
      stopGpu();
      stopRevert();
      clearInterval(timer);
    };
  });
</script>

<header>
  <div>
    <h1>Graphics</h1>
    <p class="muted">NVIDIA clock offsets, power limit, fan control, and an overlay through the driver and RivaTuner.</p>
  </div>
</header>

{#if !acknowledged}
  <div class="card ack">
    <Icon name="warn" size={22} />
    <div>
      <h3>Before you change clocks</h3>
      <p class="muted">
        A clock offset the card cannot hold can crash the driver. Helm stays inside the range the driver reports, and puts unconfirmed
        offsets back after {CONFIRM_SECS} seconds. Power and fan changes apply immediately and are not raised past the driver's own limit.
      </p>
    </div>
    <button class="btn primary" onclick={acknowledge}>I understand</button>
  </div>
{:else if loadError}
  <div class="card empty">
    <Icon name="warn" size={24} />
    <p>Graphics tuning isn't available</p>
    <p class="muted">{loadError}</p>
  </div>
{:else if !dev}
  <div class="card empty">
    <p class="muted">{gpu?.message ?? 'Reading the GPU…'}</p>
  </div>
{:else}
  <section class="card stats">
    <div class="identity">
      <div>
        <h2>{dev.name}</h2>
        <p class="faint">
          {dev.performanceState != null ? `P${dev.performanceState}` : 'Performance state unknown'}
          {#if dev.limits.length}
            · {dev.limits.join(', ')}
          {/if}
        </p>
      </div>
      {#if gpu?.frames.rtss && gpu.frames.fps != null}
        <div class="fps">
          <span class="num">{gpu.frames.fps.toFixed(0)}</span>
          <span class="faint">fps{gpu.frames.app ? ` · ${gpu.frames.app}` : ''}</span>
        </div>
      {/if}
    </div>
    <div class="meters">
      <div class="meter">
        <span class="faint">Temperature</span>
        <strong style:color={heat(dev.temperature)}>{fmt(dev.temperature, '°C')}°</strong>
        <Sparkline points={series.temp} color={heat(dev.temperature)} height={28} />
      </div>
      <div class="meter">
        <span class="faint">Power</span>
        <strong class="num">{fmt(dev.powerW, 'W', 1)} W</strong>
        <Sparkline points={series.power} color="var(--violet)" height={28} />
      </div>
      <div class="meter">
        <span class="faint">Utilization</span>
        <strong class="num">{fmt(dev.utilization, '%')}%</strong>
        <Sparkline points={series.util} height={28} />
      </div>
      <div class="meter">
        <span class="faint">Graphics clock</span>
        <strong class="num">{fmt(dev.graphicsClock, 'MHz')} MHz</strong>
      </div>
      <div class="meter">
        <span class="faint">Memory clock</span>
        <strong class="num">{fmt(dev.memoryClock, 'MHz')} MHz</strong>
      </div>
      <div class="meter">
        <span class="faint">Fan</span>
        <strong class="num">
          {fmt(dev.fanPercent, '%')}%{dev.fanRpm != null ? ` · ${dev.fanRpm} rpm` : ''}
        </strong>
        <span class="faint">{dev.fanAuto ? 'Driver controlled' : 'Manual'}{#if dev.fanCount > 1} · {dev.fanCount} fans{/if}</span>
      </div>
      <div class="meter">
        <span class="faint">Memory</span>
        <strong class="num">{gib(dev.memoryUsed)} / {gib(dev.memoryTotal)}</strong>
        <span class="faint">Controller {fmt(dev.memoryController, '%')}%</span>
      </div>
      <div class="meter">
        <span class="faint">Limits</span>
        <strong class="num">{fmt(dev.powerLimitW, 'W', 0)} W</strong>
        <span class="faint">
          Target {fmt(dev.tempLimit, '°C')}°{#if dev.tempSlowdown != null} · slowdown {dev.tempSlowdown}°{/if}
        </span>
      </div>
    </div>
  </section>

  <section class="card block">
    <div class="block-head">
      <h2>Clocks</h2>
      <p class="faint">Offsets from the card's own boost curve. Unconfirmed changes revert after {CONFIRM_SECS} seconds.</p>
    </div>
    {#if clocksSupported}
      <label class="slider">
        <span>Core</span>
        <input
          type="range"
          min={dev.coreOffsetMin ?? -500}
          max={dev.coreOffsetMax ?? 500}
          step="15"
          value={core}
          oninput={(e) => (core = clampOffset(+e.currentTarget.value, dev.coreOffsetMin, dev.coreOffsetMax))}
        />
        <input
          class="input num"
          type="number"
          min={dev.coreOffsetMin ?? undefined}
          max={dev.coreOffsetMax ?? undefined}
          value={core}
          onchange={(e) => (core = clampOffset(+e.currentTarget.value, dev.coreOffsetMin, dev.coreOffsetMax))}
        />
        <span class="unit faint">MHz</span>
      </label>
      <label class="slider">
        <span>Memory</span>
        <input
          type="range"
          min={dev.memoryOffsetMin ?? -500}
          max={dev.memoryOffsetMax ?? 500}
          step="15"
          value={memory}
          oninput={(e) => (memory = clampOffset(+e.currentTarget.value, dev.memoryOffsetMin, dev.memoryOffsetMax))}
        />
        <input
          class="input num"
          type="number"
          min={dev.memoryOffsetMin ?? undefined}
          max={dev.memoryOffsetMax ?? undefined}
          value={memory}
          onchange={(e) => (memory = clampOffset(+e.currentTarget.value, dev.memoryOffsetMin, dev.memoryOffsetMax))}
        />
        <span class="unit faint">MHz</span>
      </label>
      <p class="faint range">
        Driver range {dev.coreOffsetMin} to {dev.coreOffsetMax} MHz core, {dev.memoryOffsetMin} to {dev.memoryOffsetMax} MHz memory.
      </p>
    {:else}
      <p class="muted">This driver does not expose clock offsets.</p>
    {/if}
  </section>

  {#if powerSupported}
    <section class="card block">
      <div class="block-head">
        <h2>Power limit</h2>
        <p class="faint">
          {Math.round(dev.powerMinW!)}–{Math.round(dev.powerMaxW!)} W · default {fmt(dev.powerDefaultW, 'W', 0)} W
        </p>
      </div>
      <label class="slider">
        <span>Limit</span>
        <input
          type="range"
          min={dev.powerMinW}
          max={dev.powerMaxW}
          step="1"
          value={power}
          oninput={(e) => (power = clampOffset(+e.currentTarget.value, dev.powerMinW, dev.powerMaxW))}
        />
        <input
          class="input num"
          type="number"
          min={dev.powerMinW}
          max={dev.powerMaxW}
          value={power}
          onchange={(e) => (power = clampOffset(+e.currentTarget.value, dev.powerMinW, dev.powerMaxW))}
        />
        <span class="unit faint">W</span>
      </label>
    </section>
  {/if}

  <section class="card block">
    <div class="block-head">
      <h2>Fan</h2>
      <p class="faint">A curve is applied by Helm. Fans return to automatic when Helm exits, and go to 100% near the slowdown temperature.</p>
    </div>
    <div class="modes" role="radiogroup" aria-label="Fan mode">
      {#each fanModes as mode (mode)}
        <button class={{ on: fanMode === mode }} role="radio" aria-checked={fanMode === mode} onclick={() => (fanMode = mode)}>
          {mode === 'auto' ? 'Automatic' : mode === 'manual' ? 'Fixed' : 'Curve'}
        </button>
      {/each}
    </div>
    {#if fanMode === 'manual'}
      <label class="slider">
        <span>Speed</span>
        <input type="range" min="0" max="100" step="1" bind:value={fanPercent} />
        <span class="num">{fanPercent}%</span>
      </label>
    {:else if fanMode === 'curve'}
      <div class="curve">
        {#each fanCurve as point, i (i)}
          <div class="point">
            <input class="input num" type="number" min="0" max="110" value={point[0]} onchange={(e) => setPoint(i, 0, +e.currentTarget.value)} />
            <span class="faint">°C</span>
            <input class="input num" type="number" min="0" max="100" value={point[1]} onchange={(e) => setPoint(i, 1, +e.currentTarget.value)} />
            <span class="faint">%</span>
            <button class="btn ghost sm" disabled={fanCurve.length <= 2} onclick={() => removePoint(i)} aria-label="Remove point">
              <Icon name="x" size={12} />
            </button>
          </div>
        {/each}
        <button class="btn ghost sm" disabled={fanCurve.length >= 8} onclick={addPoint}>Add point</button>
      </div>
    {/if}
  </section>

  <section class="card block options">
    <label class="option">
      <div>
        <strong>Re-apply on startup</strong>
        <p class="faint">Restores the saved offsets and power limit the next time Helm starts.</p>
      </div>
      <Toggle label="Re-apply on startup" checked={applyOnStart} onchange={(v) => (applyOnStart = v)} />
    </label>
    <label class="option">
      <div>
        <strong>On-screen display</strong>
        <p class="faint">
          {#if gpu?.frames.rtss}
            Writes a Helm line into RivaTuner Statistics Server.
          {:else}
            RivaTuner Statistics Server isn't running, so the overlay has nowhere to go yet.
          {/if}
        </p>
      </div>
      <Toggle label="On-screen display" checked={osd} onchange={(v) => (osd = v)} />
    </label>
  </section>

  <div class={['applybar', { visible: dirty }]}>
    <span class="muted">{applying ? 'Applying…' : 'Unapplied graphics changes'}</span>
    <div class="spacer"></div>
    <button class="btn primary" disabled={applying} onclick={apply}>
      <Icon name="check" size={14} />Apply
    </button>
  </div>
{/if}

{#if countdown !== null}
  <div class="scrim" role="presentation">
    <div class="card dialog" role="alertdialog" aria-modal="true" aria-labelledby="gpu-keep-title">
      <div class="ring" style:--p={countdown / CONFIRM_SECS}>
        <span class="num">{countdown}</span>
      </div>
      <h3 id="gpu-keep-title">Keep these clock offsets?</h3>
      <p class="muted">If the card is stable, keep them. Otherwise Helm puts the previous offsets back.</p>
      <div class="dialog-actions">
        <button class="btn" onclick={revert}>Revert now</button>
        <button class="btn primary" onclick={keep}>Keep</button>
      </div>
    </div>
  </div>
{/if}

<style>
  header {
    margin-bottom: 20px;
  }

  header p {
    margin-top: 4px;
  }

  .ack {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 18px 20px;
    color: var(--warn);
    border-color: color-mix(in srgb, var(--warn) 30%, transparent);
  }

  .ack div {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .ack h3 {
    color: var(--text);
  }

  .empty {
    padding: 40px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    text-align: center;
    color: var(--muted);
  }

  .stats,
  .block {
    padding: 16px 18px;
    margin-bottom: 14px;
  }

  .identity {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 16px;
    margin-bottom: 14px;
  }

  .fps {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
  }

  .fps .num {
    font-size: 28px;
    font-weight: 650;
    line-height: 1;
  }

  .meters {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 12px;
  }

  .meter {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .meter strong {
    font-size: 16px;
    font-weight: 620;
  }

  .block-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 12px;
  }

  .block-head p {
    font-size: 12px;
  }

  .slider {
    display: grid;
    grid-template-columns: 72px 1fr 84px 42px;
    align-items: center;
    gap: 10px;
    margin-bottom: 8px;
  }

  .range {
    font-size: 12px;
  }

  .modes {
    display: flex;
    gap: 6px;
    margin-bottom: 12px;
  }

  .modes button {
    height: 30px;
    padding: 0 12px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--line);
    background: transparent;
    color: var(--muted);
  }

  .modes button.on {
    color: var(--text);
    border-color: color-mix(in srgb, var(--accent) 45%, transparent);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }

  .curve {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
  }

  .point {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .options {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .option {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
  }

  .option p {
    font-size: 12px;
    margin-top: 2px;
  }

  .applybar {
    position: sticky;
    bottom: 16px;
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 8px;
    padding: 12px 14px;
    border-radius: var(--radius);
    border: 1px solid color-mix(in srgb, var(--accent) 40%, transparent);
    background: color-mix(in srgb, var(--accent) 8%, var(--surface));
    transform: translateY(140%);
    opacity: 0;
    pointer-events: none;
    transition:
      transform 0.18s,
      opacity 0.18s;
  }

  .applybar.visible {
    transform: none;
    opacity: 1;
    pointer-events: auto;
  }

  .spacer {
    flex: 1;
  }

  .scrim {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    background: rgb(0 0 0 / 55%);
    z-index: 50;
  }

  .dialog {
    width: 380px;
    padding: 24px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    text-align: center;
  }

  .ring {
    width: 72px;
    height: 72px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background:
      radial-gradient(closest-side, var(--surface) 82%, transparent 84%),
      conic-gradient(var(--accent) calc(var(--p) * 360deg), var(--line) 0);
    font-size: 22px;
    font-weight: 650;
    margin-bottom: 6px;
  }

  .dialog-actions {
    display: flex;
    gap: 8px;
    margin-top: 8px;
  }

  @media (max-width: 900px) {
    .meters {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }

    .slider {
      grid-template-columns: 64px 1fr 72px;
    }

    .slider .unit {
      display: none;
    }
  }
</style>
