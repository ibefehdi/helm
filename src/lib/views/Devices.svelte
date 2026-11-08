<script lang="ts">
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { app } from '$lib/app.svelte';
  import { api } from '$lib/api';
  import Icon from '$lib/components/Icon.svelte';
  import Toggle from '$lib/components/Toggle.svelte';
  import type { DeviceApply, DevicesSnapshot, LogiDevice, PulsarApply, PulsarSnapshot } from '$lib/types';

  let snap = $state<DevicesSnapshot | null>(null);
  let selected = $state<string | null>(null);
  let applying = $state(false);

  let levels = $state<number[]>([]);
  let activeDpi = $state(0);
  let reportRate = $state<number | null>(null);
  let volume = $state(0);
  let muted = $state(false);
  let sidetone = $state(0);
  let sidetoneMuted = $state(false);
  let lightingOn = $state(true);
  let brightness = $state(100);
  let seededFor = $state<string | null>(null);

  const device = $derived(snap?.devices.find((item) => item.id === selected) ?? snap?.devices[0] ?? null);
  const dirty = $derived.by(() => {
    if (!device || seededFor !== device.id) return false;
    if (device.mouse && (activeDpi !== device.mouse.activeDpi || levels.join() !== device.mouse.levels.join())) return true;
    if (device.mouse?.reportRate != null && reportRate !== device.mouse.reportRate) return true;
    if (device.audio?.volume != null && (volume !== device.audio.volume || muted !== !!device.audio.muted)) return true;
    if (device.audio?.sidetone != null && (sidetone !== device.audio.sidetone || sidetoneMuted !== !!device.audio.sidetoneMuted)) return true;
    if (device.lighting?.on != null && lightingOn !== device.lighting.on) return true;
    if (device.lighting?.brightness != null && brightness !== device.lighting.brightness) return true;
    return false;
  });

  function seed(item: LogiDevice) {
    levels = item.mouse ? [...item.mouse.levels] : [];
    activeDpi = item.mouse?.activeDpi ?? 0;
    reportRate = item.mouse?.reportRate ?? null;
    volume = item.audio?.volume ?? 0;
    muted = !!item.audio?.muted;
    sidetone = item.audio?.sidetone ?? 0;
    sidetoneMuted = !!item.audio?.sidetoneMuted;
    lightingOn = item.lighting?.on ?? true;
    brightness = item.lighting?.brightness ?? 100;
    seededFor = item.id;
  }

  function seen(ms: number | null) {
    if (ms == null) return '';
    return new Date(ms).toLocaleString(undefined, { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' });
  }

  function connectionLabel(value: string) {
    const cleaned = value.replaceAll('_', ' ').toLowerCase();
    if (!cleaned || cleaned.startsWith('invalid')) return '';
    return cleaned.charAt(0).toUpperCase() + cleaned.slice(1);
  }

  function accept(value: DevicesSnapshot) {
    const nextId = selected ?? value.devices[0]?.id ?? null;
    const next = value.devices.find((item) => item.id === nextId) ?? null;
    const keepDraft = !!(next && seededFor === next.id && dirty);
    snap = value;
    if (!selected && next) selected = next.id;
    if (next && !keepDraft) seed(next);
  }

  async function apply() {
    if (!device || applying) return;
    applying = true;
    const change: DeviceApply = { id: device.id };
    if (device.mouse && (activeDpi !== device.mouse.activeDpi || levels.join() !== device.mouse.levels.join())) {
      change.activeDpi = activeDpi;
      change.dpiLevels = levels;
    }
    if (device.mouse?.reportRate != null && reportRate != null && reportRate !== device.mouse.reportRate) change.reportRate = reportRate;
    if (device.audio?.volume != null && (volume !== device.audio.volume || muted !== !!device.audio.muted)) {
      change.volume = volume;
      change.muted = muted;
    }
    if (device.audio?.sidetone != null && (sidetone !== device.audio.sidetone || sidetoneMuted !== !!device.audio.sidetoneMuted)) {
      change.sidetone = sidetone;
      change.sidetoneMuted = sidetoneMuted;
    }
    if (device.lighting?.on != null && lightingOn !== device.lighting.on) change.lightingOn = lightingOn;
    if (device.lighting?.brightness != null && brightness !== device.lighting.brightness) change.brightness = brightness;
    const next = await app.run(() => api.devicesApply(change));
    applying = false;
    if (!next) return;
    seededFor = null;
    accept(next);
    app.toast('success', 'Device settings applied');
  }

  // ---- Pulsar mouse -----------------------------------------------------------
  let pulsar = $state<PulsarSnapshot | null>(null);
  let pulsarApplying = $state(false);
  let stages = $state<number[]>([]);
  let activeStage = $state(1);
  let pulsarRate = $state<number | null>(null);
  let pulsarSeeded = $state(false);

  const pulsarDirty = $derived.by(() => {
    if (!pulsar?.present || !pulsarSeeded) return false;
    return (
      stages.join() !== pulsar.dpiStages.join() ||
      activeStage !== pulsar.activeStage ||
      (pulsar.reportRate != null && pulsarRate !== pulsar.reportRate)
    );
  });

  function acceptPulsar(value: PulsarSnapshot) {
    const keepDraft = pulsar?.present && pulsarSeeded && pulsarDirty;
    pulsar = value;
    if (value.present && !keepDraft) {
      stages = [...value.dpiStages];
      activeStage = value.activeStage;
      pulsarRate = value.reportRate;
      pulsarSeeded = true;
    }
  }

  async function applyPulsar() {
    if (!pulsar?.present || pulsarApplying) return;
    pulsarApplying = true;
    const change: PulsarApply = {};
    if (stages.join() !== pulsar.dpiStages.join()) change.dpiStages = stages;
    if (activeStage !== pulsar.activeStage) change.activeStage = activeStage;
    if (pulsar.reportRate != null && pulsarRate != null && pulsarRate !== pulsar.reportRate) change.reportRate = pulsarRate;
    const next = await app.run(() => api.pulsarApply(change));
    pulsarApplying = false;
    if (!next) return;
    pulsarSeeded = false;
    acceptPulsar(next);
    app.toast('success', 'Pulsar settings applied');
  }

  onMount(() => {
    let stop = () => {};
    let stopPulsar = () => {};
    void listen<DevicesSnapshot>('devices', (e) => accept(e.payload)).then((unlisten) => (stop = unlisten));
    void api.devicesGet().then((value) => {
      if (!snap?.agent) accept(value);
    });
    void listen<PulsarSnapshot>('pulsar', (e) => acceptPulsar(e.payload)).then((unlisten) => (stopPulsar = unlisten));
    void api.pulsarGet().then((value) => {
      if (!pulsar?.present) acceptPulsar(value);
    });
    return () => {
      stop();
      stopPulsar();
    };
  });
</script>

<header>
  <div>
    <h1>Devices</h1>
    <p class="muted">Mice, headsets, and keyboards through the G HUB agent already running on this PC.</p>
  </div>
</header>

{#if !snap}
  <div class="card empty"><p class="muted">Looking for the G HUB agent…</p></div>
{:else if !snap.agent}
  <div class="card empty">
    <Icon name="warn" size={24} />
    <p>G HUB isn't available</p>
    <p class="muted">{snap.message}</p>
  </div>
{:else}
  {#if snap.devices.length === 0}
    <div class="card empty">
      <p>No Logitech device is connected</p>
      <p class="muted">Plug one in, or wake a lightspeed receiver. Helm reads it through G HUB.</p>
    </div>
  {:else}
    {#if snap.devices.length > 1}
      <div class="picker">
        {#each snap.devices as item (item.id)}
          <button class={{ on: device?.id === item.id }} onclick={() => (selected = item.id)}>{item.name}</button>
        {/each}
      </div>
    {/if}

    {#if device}
      <section class="card identity">
        <div>
          <h2>{device.name}</h2>
          <p class="faint">
            {device.kind}
            {#if connectionLabel(device.connection)}
              · {connectionLabel(device.connection)}
            {/if}
            {#if device.state && !device.state.startsWith('INVALID')}
              · {device.state.toLowerCase()}
            {/if}
          </p>
        </div>
        {#if device.battery?.percent != null}
          <div class="battery">
            <span class="num">{device.battery.percent}%</span>
            <span class="faint">{device.battery.charging ? 'Charging' : 'Battery'}</span>
          </div>
        {/if}
      </section>

      {#if device.mouse}
        <section class="card block">
          <div class="block-head">
            <h2>DPI</h2>
            <p class="faint">The active step is what the mouse uses now.</p>
          </div>
          <div class="steps">
            {#each levels as level, index (index)}
              <label class={{ step: true, on: activeDpi === level }}>
                <input
                  class="input num"
                  type="number"
                  min="50"
                  max="45000"
                  value={level}
                  onchange={(e) => {
                    const next = Math.round(+e.currentTarget.value);
                    levels = levels.map((item, i) => (i === index ? next : item));
                    if (activeDpi === level) activeDpi = next;
                  }}
                />
                <button type="button" onclick={() => (activeDpi = level)}>Use</button>
              </label>
            {/each}
          </div>
          {#if device.mouse.reportRates.length}
            <div class="rates" role="radiogroup" aria-label="Report rate">
              {#each device.mouse.reportRates as rate (rate)}
                <button class={{ on: reportRate === rate }} role="radio" aria-checked={reportRate === rate} onclick={() => (reportRate = rate)}>
                  {rate >= 1000 ? `${rate / 1000} kHz` : `${rate} Hz`}
                </button>
              {/each}
            </div>
          {/if}
        </section>
      {/if}

      {#if device.audio}
        <section class="card block">
          <div class="block-head">
            <h2>Audio</h2>
            <p class="faint">Levels are sent to the headset as a fraction of full scale.</p>
          </div>
          {#if device.audio.volume != null}
            <label class="slider">
              <span>Volume</span>
              <input type="range" min="0" max="100" bind:value={volume} />
              <span class="num">{volume}%</span>
            </label>
            <label class="option">
              <span>Muted</span>
              <Toggle label="Muted" checked={muted} onchange={(value) => (muted = value)} />
            </label>
          {/if}
          {#if device.audio.sidetone != null}
            <label class="slider">
              <span>Sidetone</span>
              <input type="range" min="0" max="100" bind:value={sidetone} />
              <span class="num">{sidetone}%</span>
            </label>
            <label class="option">
              <span>Sidetone off</span>
              <Toggle label="Sidetone off" checked={sidetoneMuted} onchange={(value) => (sidetoneMuted = value)} />
            </label>
          {/if}
        </section>
      {/if}

      {#if device.lighting && (device.lighting.on != null || device.lighting.brightness != null)}
        <section class="card block">
          <div class="block-head">
            <h2>Lighting</h2>
            <p class="faint">On/off and brightness. Effects stay where G HUB saved them.</p>
          </div>
          {#if device.lighting.on != null}
            <label class="option">
              <span>Lighting</span>
              <Toggle label="Lighting" checked={lightingOn} onchange={(value) => (lightingOn = value)} />
            </label>
          {/if}
          {#if device.lighting.brightness != null}
            <label class="slider">
              <span>Brightness</span>
              <input type="range" min="0" max="100" bind:value={brightness} />
              <span class="num">{brightness}%</span>
            </label>
          {/if}
        </section>
      {/if}

      <div class={['applybar', { visible: dirty }]}>
        <span class="muted">{applying ? 'Applying…' : 'Unapplied device changes'}</span>
        <div class="spacer"></div>
        <button class="btn primary" disabled={applying} onclick={apply}>
          <Icon name="check" size={14} />Apply
        </button>
      </div>
    {/if}
  {/if}

  {#if snap.known.length}
    <section class="card block">
      <div class="block-head">
        <h2>Remembered</h2>
        <p class="faint">G HUB has seen these, and they aren't connected right now.</p>
      </div>
      <ul class="known">
        {#each snap.known as item (item.model + item.kind)}
          <li>
            <strong>{item.name}</strong>
            <span class="faint">{item.kind}{item.lastSeen ? ` · last seen ${seen(item.lastSeen)}` : ''}</span>
          </li>
        {/each}
      </ul>
    </section>
  {/if}
{/if}

<style>
  header {
    margin-bottom: 20px;
  }

  header p {
    margin-top: 4px;
  }

  .empty {
    padding: 40px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    text-align: center;
    color: var(--muted);
    margin-bottom: 14px;
  }

  .picker {
    display: flex;
    gap: 6px;
    margin-bottom: 12px;
  }

  .picker button,
  .rates button,
  .step button {
    height: 30px;
    padding: 0 12px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--line);
    background: transparent;
    color: var(--muted);
  }

  .picker button.on,
  .rates button.on,
  .step.on button {
    color: var(--text);
    border-color: color-mix(in srgb, var(--accent) 45%, transparent);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }

  .identity,
  .block {
    padding: 16px 18px;
    margin-bottom: 14px;
  }

  .identity {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 16px;
  }

  .battery {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
  }

  .battery .num {
    font-size: 22px;
    font-weight: 650;
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

  .steps,
  .rates {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .rates {
    margin-top: 12px;
  }

  .step {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .step .input {
    width: 84px;
  }

  .slider,
  .option {
    display: grid;
    grid-template-columns: 110px 1fr auto;
    align-items: center;
    gap: 10px;
    margin-bottom: 8px;
  }

  .known {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .known li {
    display: flex;
    justify-content: space-between;
    gap: 12px;
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
</style>
