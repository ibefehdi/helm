<script lang="ts">
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { app } from '$lib/app.svelte';
  import { api } from '$lib/api';
  import Icon from '$lib/components/Icon.svelte';
  import Toggle from '$lib/components/Toggle.svelte';
  import type { DeviceApply, DevicesSnapshot, LogiDevice, PulsarApply, PulsarSnapshot } from '$lib/types';

  let snap = $state<DevicesSnapshot | null>(null);
  let open = $state<{ source: 'logi'; id: string } | { source: 'pulsar' } | null>(null);
  let section = $state<string | null>(null);
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

  const device = $derived.by(() => {
    const current = open;
    if (current?.source !== 'logi') return null;
    return snap?.devices.find((item) => item.id === current.id) ?? null;
  });
  const logiSections = $derived.by(() => {
    if (!device) return [] as { id: string; label: string }[];
    const items: { id: string; label: string }[] = [];
    if (device.mouse) items.push({ id: 'pointer', label: 'Pointer' });
    if (device.audio) items.push({ id: 'audio', label: 'Audio' });
    if (device.lighting && (device.lighting.on != null || device.lighting.brightness != null)) items.push({ id: 'lighting', label: 'Lighting' });
    return items;
  });
  const logiSection = $derived(logiSections.some((item) => item.id === section) ? section : (logiSections[0]?.id ?? null));
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

  function caps(item: LogiDevice) {
    const parts: string[] = [];
    if (item.mouse) parts.push('Pointer');
    if (item.audio) parts.push('Audio');
    if (item.lighting && (item.lighting.on != null || item.lighting.brightness != null)) parts.push('Lighting');
    return parts;
  }

  function statusLine(item: LogiDevice) {
    const parts = [item.kind];
    const connection = connectionLabel(item.connection);
    if (connection) parts.push(connection);
    if (item.state && !item.state.startsWith('INVALID')) parts.push(item.state.toLowerCase());
    const groups = caps(item);
    if (groups.length) parts.push(groups.join(', '));
    return parts.join(' · ');
  }

  function openLogi(id: string) {
    open = { source: 'logi', id };
    section = null;
    const item = snap?.devices.find((entry) => entry.id === id);
    if (item && seededFor !== id) seed(item);
  }

  function accept(value: DevicesSnapshot) {
    const openId = open?.source === 'logi' ? open.id : null;
    const next = openId ? (value.devices.find((item) => item.id === openId) ?? null) : null;
    const keepDraft = !!(next && seededFor === next.id && dirty);
    snap = value;
    if (open?.source === 'logi' && !next) {
      open = null;
      return;
    }
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
  let motionSync = $state(false);
  let ripple = $state(false);
  let angleSnap = $state(false);
  let turbo = $state(false);
  let lowPower = $state(false);
  let debounce = $state(0);
  let lod = $state(1);
  let sleep = $state(0);
  let longDistance = $state(false);
  let longDistanceTouched = $state(false);
  let pulsarSeeded = $state(false);

  const pulsarSections = $derived.by(() => {
    if (!pulsar?.present) return [] as { id: string; label: string }[];
    const items: { id: string; label: string }[] = [];
    if (pulsar.dpiStages.length) items.push({ id: 'dpi', label: 'DPI' });
    if (pulsar.reportRate != null && pulsar.reportRates.length) items.push({ id: 'polling', label: 'Polling' });
    items.push({ id: 'performance', label: 'Performance' }, { id: 'power', label: 'Power' });
    return items;
  });
  const pulsarSection = $derived(pulsarSections.some((item) => item.id === section) ? section : (pulsarSections[0]?.id ?? null));
  const pulsarCaps = $derived(pulsarSections.map((item) => item.label));

  const pulsarDirty = $derived.by(() => {
    if (!pulsar?.present || !pulsarSeeded) return false;
    return (
      stages.join() !== pulsar.dpiStages.join() ||
      activeStage !== pulsar.activeStage ||
      (pulsar.reportRate != null && pulsarRate !== pulsar.reportRate) ||
      motionSync !== pulsar.motionSync ||
      ripple !== pulsar.ripple ||
      angleSnap !== pulsar.angleSnap ||
      turbo !== pulsar.turbo ||
      lowPower !== pulsar.lowPower ||
      debounce !== pulsar.debounceMs ||
      lod !== pulsar.lod ||
      sleep !== pulsar.sleepMinutes ||
      longDistanceTouched
    );
  });

  function seedPulsar(value: PulsarSnapshot) {
    stages = [...value.dpiStages];
    activeStage = value.activeStage;
    pulsarRate = value.reportRate;
    motionSync = value.motionSync;
    ripple = value.ripple;
    angleSnap = value.angleSnap;
    turbo = value.turbo;
    lowPower = value.lowPower;
    debounce = value.debounceMs;
    lod = value.lod;
    sleep = value.sleepMinutes;
    longDistance = false;
    longDistanceTouched = false;
    pulsarSeeded = true;
  }

  function openPulsar() {
    open = { source: 'pulsar' };
    section = null;
    if (pulsar?.present && !pulsarSeeded) seedPulsar(pulsar);
  }

  function close() {
    if (open?.source === 'logi' && !dirty) seededFor = null;
    if (open?.source === 'pulsar' && !pulsarDirty) pulsarSeeded = false;
    open = null;
    section = null;
  }

  function acceptPulsar(value: PulsarSnapshot) {
    const keepDraft = pulsar?.present && pulsarSeeded && pulsarDirty;
    pulsar = value;
    if (!value.present && open?.source === 'pulsar') open = null;
    if (value.present && !keepDraft) seedPulsar(value);
  }

  async function applyPulsar() {
    if (!pulsar?.present || pulsarApplying) return;
    pulsarApplying = true;
    const change: PulsarApply = {};
    if (stages.join() !== pulsar.dpiStages.join()) change.dpiStages = stages;
    if (activeStage !== pulsar.activeStage) change.activeStage = activeStage;
    if (pulsar.reportRate != null && pulsarRate != null && pulsarRate !== pulsar.reportRate) change.reportRate = pulsarRate;
    if (motionSync !== pulsar.motionSync) change.motionSync = motionSync;
    if (ripple !== pulsar.ripple) change.ripple = ripple;
    if (angleSnap !== pulsar.angleSnap) change.angleSnap = angleSnap;
    if (turbo !== pulsar.turbo) change.turbo = turbo;
    if (lowPower !== pulsar.lowPower) change.lowPower = lowPower;
    if (debounce !== pulsar.debounceMs) change.debounceMs = debounce;
    if (lod !== pulsar.lod) change.lod = lod;
    if (sleep !== pulsar.sleepMinutes) change.sleepMinutes = sleep;
    if (longDistanceTouched) change.longDistance = longDistance;
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


{#if open?.source === 'logi' && device}
  <header>
    <button class="btn ghost sm back" type="button" onclick={close}>‹ Devices</button>
    <div class="split">
      <div>
        <h1>{device.name}</h1>
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
    </div>
  </header>

  {#if logiSections.length > 1}
    <div class="picker" role="tablist" aria-label="Setting groups">
      {#each logiSections as item (item.id)}
        <button type="button" role="tab" aria-selected={logiSection === item.id} class={{ on: logiSection === item.id }} onclick={() => (section = item.id)}>
          {item.label}
        </button>
      {/each}
    </div>
  {:else if logiSections.length === 0}
    <div class="card empty">
      <p>Nothing to change here</p>
      <p class="muted">Helm can see this device, but it doesn't expose pointer, audio, or lighting controls.</p>
    </div>
  {/if}

  {#if logiSection === 'pointer' && device.mouse}
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

  {#if logiSection === 'audio' && device.audio}
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

  {#if logiSection === 'lighting' && device.lighting && (device.lighting.on != null || device.lighting.brightness != null)}
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
{:else if open?.source === 'pulsar' && pulsar?.present}
  <header>
    <button class="btn ghost sm back" type="button" onclick={close}>‹ Devices</button>
    <div class="split">
      <div>
        <h1>{pulsar.model || 'Pulsar mouse'}</h1>
        <p class="faint">
          Pulsar
          {#if pulsar.firmware}· firmware {pulsar.firmware}{/if}
          · {pulsar.online ? 'online' : 'asleep'}
        </p>
      </div>
      {#if pulsar.battery != null}
        <div class="battery">
          <span class="num">{pulsar.battery}%</span>
          <span class="faint">{pulsar.charging ? 'Charging' : 'Battery'}</span>
        </div>
      {/if}
    </div>
  </header>

  {#if pulsar.vendorApp}
    <section class="card block note">
      <Icon name="warn" size={16} />
      <p class="faint">Pulsar's own app is open. Close it so Helm and Pulsar don't fight over the mouse, or readings and changes may be unreliable.</p>
    </section>
  {/if}

  {#if pulsarSections.length > 1}
    <div class="picker" role="tablist" aria-label="Setting groups">
      {#each pulsarSections as item (item.id)}
        <button type="button" role="tab" aria-selected={pulsarSection === item.id} class={{ on: pulsarSection === item.id }} onclick={() => (section = item.id)}>
          {item.label}
        </button>
      {/each}
    </div>
  {/if}

  {#if pulsarSection === 'dpi' && pulsar.dpiStages.length}
    <section class="card block">
      <div class="block-head">
        <h2>DPI</h2>
        <p class="faint">Each stage is a step you cycle on the mouse. The highlighted one is active.</p>
      </div>
      <div class="steps">
        {#each stages as stage, index (index)}
          <label class={{ step: true, on: activeStage === index + 1 }}>
            <input
              class="input num"
              type="number"
              min="50"
              max="32000"
              step="50"
              value={stage}
              onchange={(e) => (stages = stages.map((s, i) => (i === index ? Math.round(+e.currentTarget.value) : s)))}
            />
            <button type="button" onclick={() => (activeStage = index + 1)}>Use</button>
          </label>
        {/each}
      </div>
    </section>
  {/if}

  {#if pulsarSection === 'polling' && pulsar.reportRate != null && pulsar.reportRates.length}
    <section class="card block">
      <div class="block-head">
        <h2>Polling rate</h2>
        <p class="faint">How often the mouse reports its position.</p>
      </div>
      <div class="rates" role="radiogroup" aria-label="Polling rate">
        {#each pulsar.reportRates as rate (rate)}
          <button class={{ on: pulsarRate === rate }} role="radio" aria-checked={pulsarRate === rate} onclick={() => (pulsarRate = rate)}>
            {rate >= 1000 ? `${rate / 1000} kHz` : `${rate} Hz`}
          </button>
        {/each}
      </div>
    </section>
  {/if}

  {#if pulsarSection === 'performance'}
    <section class="card block">
      <div class="block-head">
        <h2>Performance</h2>
        <p class="faint">Sensor and tracking options stored on the mouse.</p>
      </div>
      <label class="option">
        <div><strong>Motion Sync</strong><p class="faint">Aligns sensor sampling with the polling clock.</p></div>
        <Toggle label="Motion Sync" checked={motionSync} onchange={(v) => (motionSync = v)} />
      </label>
      <label class="option">
        <div><strong>Ripple control</strong><p class="faint">Smooths jitter at the cost of a little latency.</p></div>
        <Toggle label="Ripple control" checked={ripple} onchange={(v) => (ripple = v)} />
      </label>
      <label class="option">
        <div><strong>Angle snap</strong><p class="faint">Straightens near-horizontal and vertical motion.</p></div>
        <Toggle label="Angle snap" checked={angleSnap} onchange={(v) => (angleSnap = v)} />
      </label>
      <label class="option">
        <div><strong>Turbo / fire key</strong><p class="faint">Rapid-fire repeat on the assigned button.</p></div>
        <Toggle label="Turbo" checked={turbo} onchange={(v) => (turbo = v)} />
      </label>
      <label class="slider">
        <span>Lift-off distance</span>
        <div class="rates">
          {#each pulsar.lodOptions as level (level)}
            <button class={{ on: lod === level }} onclick={() => (lod = level)}>{level}</button>
          {/each}
        </div>
      </label>
      <label class="slider">
        <span>Debounce</span>
        <input type="range" min="0" max="20" step="1" bind:value={debounce} />
        <span class="num">{debounce} ms</span>
      </label>
    </section>
  {/if}

  {#if pulsarSection === 'power'}
    <section class="card block">
      <div class="block-head">
        <h2>Power</h2>
        <p class="faint">Wireless battery and link options.</p>
      </div>
      <label class="slider">
        <span>Auto-sleep</span>
        <div class="rates">
          {#each pulsar.sleepOptions as minutes (minutes)}
            <button class={{ on: sleep === minutes }} onclick={() => (sleep = minutes)}>
              {minutes < 60 ? `${minutes} min` : `${minutes / 60} h`}
            </button>
          {/each}
        </div>
      </label>
      <label class="option">
        <div><strong>Low-power mode</strong><p class="faint">Reduces sensor power to extend battery life.</p></div>
        <Toggle label="Low-power mode" checked={lowPower} onchange={(v) => (lowPower = v)} />
      </label>
      <label class="option">
        <div><strong>Long-distance mode</strong><p class="faint">Stronger wireless link and range; uses more battery. Applied as a command, so its state isn't read back.</p></div>
        <Toggle label="Long-distance mode" checked={longDistance} onchange={(v) => { longDistance = v; longDistanceTouched = true; }} />
      </label>
    </section>
  {/if}

  <section class="card block note">
    <Icon name="warn" size={16} />
    <p class="faint">Button assignment and macros are still managed in Pulsar's own app. Helm covers DPI, polling, performance, power, and the battery readout.</p>
  </section>

  <div class={['applybar', { visible: pulsarDirty }]}>
    <span class="muted">{pulsarApplying ? 'Applying…' : 'Unapplied Pulsar changes'}</span>
    <div class="spacer"></div>
    <button class="btn primary" disabled={pulsarApplying} onclick={applyPulsar}>
      <Icon name="check" size={14} />Apply
    </button>
  </div>
{:else}
  {@const logi = snap?.agent ? snap.devices : []}
  {@const hasPulsar = !!pulsar?.present}

  <header>
    <div>
      <h1>Devices</h1>
      <p class="muted">Select a device to change its settings.</p>
    </div>
  </header>

  {#if !snap && !pulsar}
    <div class="card empty"><p class="muted">Looking for devices…</p></div>
  {:else if logi.length === 0 && !hasPulsar}
    {#if snap && !snap.agent}
      <div class="card empty">
        <Icon name="warn" size={24} />
        <p>G HUB isn't available</p>
        <p class="muted">{snap.message}</p>
      </div>
    {:else if !snap}
      <div class="card empty"><p class="muted">Looking for the G HUB agent…</p></div>
    {:else}
      <div class="card empty">
        <p>No device is connected</p>
        <p class="muted">Plug one in, or wake a lightspeed receiver. Helm reads Logitech gear through G HUB.</p>
      </div>
    {/if}
  {:else}
    {#if snap && !snap.agent}
      <div class="card empty">
        <Icon name="warn" size={24} />
        <p>G HUB isn't available</p>
        <p class="muted">{snap.message}</p>
      </div>
    {/if}

    <div class="roster">
      {#each logi as item (item.id)}
        <button class="card row" type="button" onclick={() => openLogi(item.id)}>
          <span class="mark"><Icon name="device" size={18} /></span>
          <span class="meta">
            <span class="name">{item.name}</span>
            <span class="faint">{statusLine(item)}</span>
          </span>
          <span class="charge">
            {#if item.battery?.percent != null}
              <span class="num">{item.battery.percent}%</span>
              <span class="faint">{item.battery.charging ? 'Charging' : 'Battery'}</span>
            {/if}
          </span>
          <span class="chev" aria-hidden="true">›</span>
        </button>
      {/each}

      {#if pulsar?.present}
        <button class="card row" type="button" onclick={openPulsar}>
          <span class="mark"><Icon name="device" size={18} /></span>
          <span class="meta">
            <span class="name">{pulsar.model || 'Pulsar mouse'}</span>
            <span class="faint">
              Pulsar · {pulsar.online ? 'online' : 'asleep'}{#if pulsar.firmware} · firmware {pulsar.firmware}{/if}{#if pulsarCaps.length} · {pulsarCaps.join(', ')}{/if}
            </span>
          </span>
          <span class="charge">
            {#if pulsar.battery != null}
              <span class="num">{pulsar.battery}%</span>
              <span class="faint">{pulsar.charging ? 'Charging' : 'Battery'}</span>
            {/if}
          </span>
          <span class="chev" aria-hidden="true">›</span>
        </button>
      {/if}
    </div>
  {/if}

  {#if snap?.known.length}
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

  .block {
    padding: 16px 18px;
    margin-bottom: 14px;
  }

  .split {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 16px;
  }

  .back {
    margin: 0 0 10px;
    padding-left: 0;
  }

  .roster {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .row {
    width: 100%;
    display: grid;
    grid-template-columns: 40px minmax(0, 1fr) auto 16px;
    align-items: center;
    gap: 14px;
    padding: 14px 16px;
    text-align: left;
    color: inherit;
  }

  .row:hover {
    background: var(--hover);
  }

  .mark {
    width: 40px;
    height: 40px;
    border-radius: var(--radius-sm);
    display: grid;
    place-items: center;
    background: var(--raised);
    color: var(--muted);
  }

  .meta {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .name {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .meta .faint {
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .charge {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
  }

  .chev {
    color: var(--faint);
    font-size: 20px;
    line-height: 1;
  }

  .option:has(div) {
    grid-template-columns: 1fr auto;
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

  .steps + .rates {
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

  .note {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    color: var(--warn);
  }

  .note p {
    margin: 0;
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
