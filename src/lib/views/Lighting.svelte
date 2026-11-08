<script lang="ts">
  import { onMount } from 'svelte';
  import { app } from '$lib/app.svelte';
  import { api, errorMessage } from '$lib/api';
  import Icon from '$lib/components/Icon.svelte';
  import Toggle from '$lib/components/Toggle.svelte';
  import type { LightingZone, RgbState, RgbZone } from '$lib/types';

  interface Effect {
    mode: number;
    label: string;
    color: boolean;
    speed: boolean;
    addressable?: boolean;
  }

  /** Firmware effects the controller runs on its own, so they keep going without Helm or GCC running. */
  const effects: Effect[] = [
    { mode: 0, label: 'Static', color: true, speed: false },
    { mode: 1, label: 'Pulse', color: true, speed: true },
    { mode: 4, label: 'Flash', color: true, speed: true },
    { mode: 11, label: 'Double flash', color: true, speed: true },
    { mode: 9, label: 'Color cycle', color: false, speed: true },
    { mode: 72, label: 'Wave', color: false, speed: true, addressable: true },
    { mode: 8, label: 'Off', color: false, speed: false },
  ];

  const swatches = [0xff2a2a, 0xff7a1a, 0xffd21a, 0x7dff3a, 0x22e0a0, 0x22c8ff, 0x3a6bff, 0x8a4dff, 0xff4dd2, 0xffffff];

  let rgb = $state<RgbState | null>(null);
  let zones = $state<RgbZone[]>([]);
  let selected = $state<number[]>([]);
  let loading = $state(true);
  let loadError = $state<string | null>(null);
  let saving = $state(false);
  let presetName = $state('');
  let pending: ReturnType<typeof setTimeout> | undefined;

  const target = $derived(zones.filter((z) => selected.includes(z.index)));
  const lead = $derived(target[0] ?? null);
  const effect = $derived(effects.find((e) => e.mode === lead?.mode) ?? effects[0]);
  const allAddressable = $derived(target.length > 0 && target.every(isAddressable));
  const restore = $derived(app.settings?.restoreLighting ?? true);
  const presets = $derived(app.settings?.lightingPresets ?? []);

  function isAddressable(z: RgbZone) {
    return z.ledType === 2 || z.ledType === 4;
  }

  function zoneName(z: RgbZone): string {
    const sameType = zones.filter((o) => o.ledType === z.ledType);
    const n = sameType.length > 1 ? ` ${sameType.indexOf(z) + 1}` : '';
    switch (z.ledType) {
      case 0:
      case 1:
        return sameType.length > 1 ? `Onboard${n}` : 'Onboard LEDs';
      case 3:
        return `12V RGB header${n}`;
      case 4:
        return `ARGB header${n}`;
      default:
        return `Addressable zone${n}`;
    }
  }

  function zoneKind(z: RgbZone) {
    return z.ledType === 0 ? 'Single colour' : z.ledType === 3 ? '4-pin, 12V' : isAddressable(z) ? '3-pin, 5V addressable' : 'RGB';
  }

  function hex(c: number) {
    return `#${(c & 0xffffff).toString(16).padStart(6, '0')}`;
  }

  function parseHex(s: string): number | null {
    const m = /^#?([0-9a-f]{6})$/i.exec(s.trim());
    return m ? parseInt(m[1], 16) : null;
  }

  function duration(speed: number) {
    return `${(3.4 - (speed - 1) * 0.35).toFixed(2)}s`;
  }

  async function load() {
    loading = true;
    loadError = null;
    try {
      const state = await api.rgbGet();
      rgb = state;
      zones = state.zones;
      if (selected.length === 0) selected = state.zones.map((z) => z.index);
    } catch (e) {
      loadError = errorMessage(e);
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    void load();
    return () => clearTimeout(pending);
  });

  function plain(list: RgbZone[]): LightingZone[] {
    return list.map(({ index, mode, color, speed, brightness }) => ({ index, mode, color, speed, brightness }));
  }

  /** Writes are debounced so dragging a slider sends one update, not dozens. */
  function schedule() {
    clearTimeout(pending);
    pending = setTimeout(() => void commit(), 280);
  }

  async function commit() {
    saving = true;
    try {
      await api.rgbSet(plain(zones));
    } catch (e) {
      app.toast('error', errorMessage(e));
    } finally {
      saving = false;
    }
  }

  function edit(patch: Partial<LightingZone>) {
    if (target.length === 0) return;
    zones = zones.map((z) => {
      if (!selected.includes(z.index)) return z;
      const next = { ...z, ...patch };
      if (next.mode === 72 && !isAddressable(z)) next.mode = 9;
      return next;
    });
    schedule();
  }

  function toggleZone(index: number, additive: boolean) {
    if (!additive) {
      selected = [index];
      return;
    }
    selected = selected.includes(index) ? selected.filter((i) => i !== index) : [...selected, index];
  }

  async function savePreset() {
    const name = presetName.trim() || `Lighting ${presets.length + 1}`;
    const p = await app.run(() => api.saveLightingPreset(name, plain(zones)), `Saved “${name}”`);
    if (p) presetName = '';
  }

  function applyPreset(zonesIn: LightingZone[]) {
    zones = zones.map((z) => {
      const p = zonesIn.find((x) => x.index === z.index);
      return p ? { ...z, ...p } : z;
    });
    void commit();
  }
</script>

<header>
  <div>
    <h1>Lighting</h1>
    <p class="muted">Effects run on the board's own controller, so they keep going when Helm is closed.</p>
  </div>
  <div class="actions">
    <div class="restore">
      <span class="muted">Re-apply at startup and after sleep</span>
      <Toggle label="Re-apply lighting" checked={restore} onchange={(v) => app.saveSettings({ restoreLighting: v })} />
    </div>
    <button class="btn" onclick={load} disabled={loading}><Icon name="refresh" size={14} />Reload</button>
  </div>
</header>

{#if loading && !rgb}
  <div class="card empty"><p class="muted">Talking to the RGB controller…</p></div>
{:else if loadError}
  <div class="card empty">
    <Icon name="warn" size={24} />
    <p>Couldn't reach the RGB controller</p>
    <p class="muted">{loadError}</p>
    <button class="btn" onclick={load}>Try again</button>
  </div>
{:else if rgb}
  {#if rgb.gccRunning}
    <div class="banner warn">
      <Icon name="warn" size={15} />
      <span>GIGABYTE Control Center is running and may overwrite these settings with its own. Close it, or turn off its RGB Fusion module.</span>
    </div>
  {/if}

  <div class="layout">
    <section class="zones">
      <div class="zones-head">
        <h2>Zones</h2>
        <button class="btn ghost sm" onclick={() => (selected = zones.map((z) => z.index))}>Select all</button>
      </div>
      {#each zones as z (z.index)}
        {@const fx = effects.find((e) => e.mode === z.mode)}
        <button
          class={['card zone', { on: selected.includes(z.index) }]}
          onclick={(e) => toggleZone(z.index, e.ctrlKey || e.shiftKey || e.metaKey)}
        >
          <span
            class={['preview', `fx-${z.mode}`]}
            style:--c={hex(z.color)}
            style:--dur={duration(z.speed)}
            style:--dim={0.25 + (z.brightness / 9) * 0.75}
          ></span>
          <span class="zone-text">
            <span class="zone-name">{zoneName(z)}</span>
            <span class="faint">{zoneKind(z)}</span>
          </span>
          <span class="chip">{fx?.label ?? `Mode ${z.mode}`}</span>
        </button>
      {/each}
      <p class="hint faint">Ctrl-click to edit several zones at once.</p>
    </section>

    <section class="card editor">
      {#if !lead}
        <p class="muted">Select a zone to edit it.</p>
      {:else}
        <div class="editor-head">
          <h3>{target.length === zones.length ? 'All zones' : target.length > 1 ? `${target.length} zones` : zoneName(lead)}</h3>
          {#if saving}<span class="chip accent">Applying…</span>{/if}
        </div>

        <div class="field">
          <h2>Effect</h2>
          <div class="effects">
            {#each effects as e (e.mode)}
              <button
                class={['effect', { active: lead.mode === e.mode }]}
                disabled={e.addressable && !allAddressable}
                title={e.addressable && !allAddressable ? 'Only addressable (ARGB) zones can run a wave' : undefined}
                onclick={() => edit({ mode: e.mode })}
              >
                <span class={['dot', `fx-${e.mode}`]} style:--c={hex(lead.color)} style:--dur="2s" style:--dim="1"></span>
                {e.label}
              </button>
            {/each}
          </div>
        </div>

        {#if effect.color}
          <div class="field">
            <h2>Colour</h2>
            <div class="colors">
              <label class="picker" style:--c={hex(lead.color)}>
                <input type="color" value={hex(lead.color)} oninput={(e) => edit({ color: parseInt(e.currentTarget.value.slice(1), 16) })} />
              </label>
              <input
                class="input num hexin"
                value={hex(lead.color).toUpperCase()}
                onchange={(e) => {
                  const c = parseHex(e.currentTarget.value);
                  if (c !== null) edit({ color: c });
                }}
              />
              <div class="swatches">
                {#each swatches as s (s)}
                  <button
                    class={['swatch', { active: (lead.color & 0xffffff) === s }]}
                    style:--c={hex(s)}
                    aria-label={hex(s)}
                    onclick={() => edit({ color: s })}
                  ></button>
                {/each}
              </div>
            </div>
          </div>
        {/if}

        {#if lead.mode !== 8}
          <div class="sliders">
            <label class="field">
              <span class="row"><h2>Brightness</h2><span class="num muted">{lead.brightness}/9</span></span>
              <input type="range" min="1" max="9" step="1" value={lead.brightness} oninput={(e) => edit({ brightness: +e.currentTarget.value })} />
            </label>
            {#if effect.speed}
              <label class="field">
                <span class="row"><h2>Speed</h2><span class="num muted">{lead.speed}/9</span></span>
                <input type="range" min="1" max="9" step="1" value={lead.speed} oninput={(e) => edit({ speed: +e.currentTarget.value })} />
              </label>
            {/if}
          </div>
        {/if}

        <div class="quick">
          <button class="btn sm" onclick={() => edit({ mode: 8 })}><Icon name="power" size={13} />Turn off</button>
          <button
            class="btn sm"
            onclick={() => {
              const { mode, color, speed, brightness } = lead;
              selected = zones.map((z) => z.index);
              edit({ mode, color, speed, brightness });
            }}><Icon name="copy" size={13} />Copy to all zones</button
          >
        </div>
      {/if}
    </section>
  </div>

  <section class="presets">
    <h2>Presets</h2>
    <form
      class="card save"
      onsubmit={(e) => {
        e.preventDefault();
        void savePreset();
      }}
    >
      <input class="input" placeholder="Name this lighting setup…" bind:value={presetName} />
      <button class="btn primary" type="submit"><Icon name="plus" size={14} />Save current</button>
    </form>
    {#if presets.length}
      <div class="preset-list">
        {#each presets as p (p.id)}
          <div class="card preset">
            <div class="strip">
              {#each p.zones as z (z.index)}
                <span class={['seg', `fx-${z.mode}`]} style:--c={hex(z.color)} style:--dur={duration(z.speed)} style:--dim="1"></span>
              {/each}
            </div>
            <span class="pname">{p.name}</span>
            <button class="btn sm" onclick={() => applyPreset(p.zones)}>Apply</button>
            <button class="btn ghost sm" aria-label="Delete preset" onclick={() => app.run(() => api.deleteLightingPreset(p.id))}><Icon name="trash" size={13} /></button>
          </div>
        {/each}
      </div>
    {/if}
  </section>
{/if}

<style>
  header {
    display: flex;
    justify-content: space-between;
    align-items: flex-end;
    gap: 16px;
    margin-bottom: 20px;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 14px;
  }

  .restore {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 12.5px;
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

  .banner {
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 10px 14px;
    border-radius: var(--radius);
    margin-bottom: 16px;
    font-size: 13px;
  }

  .banner.warn {
    color: var(--warn);
    background: color-mix(in srgb, var(--warn) 9%, transparent);
    border: 1px solid color-mix(in srgb, var(--warn) 25%, transparent);
  }

  .layout {
    display: grid;
    grid-template-columns: minmax(260px, 340px) 1fr;
    gap: 18px;
    align-items: start;
  }

  .zones {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .zones-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .zone {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 12px;
    text-align: left;
    transition: border-color 0.12s;
  }

  .zone:hover {
    border-color: var(--line-strong);
  }

  .zone.on {
    border-color: color-mix(in srgb, var(--accent) 55%, transparent);
    background: color-mix(in srgb, var(--accent) 4%, var(--surface));
  }

  .zone-text {
    flex: 1;
    display: flex;
    flex-direction: column;
    font-size: 12px;
    min-width: 0;
  }

  .zone-name {
    font-size: 13.5px;
    font-weight: 600;
  }

  .hint {
    font-size: 11.5px;
    padding: 0 2px;
  }

  .preview {
    width: 34px;
    height: 34px;
    border-radius: 8px;
    flex: none;
    background: var(--c);
    opacity: var(--dim);
    box-shadow: 0 0 16px -2px var(--c);
  }

  .editor {
    padding: 18px 20px;
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  .editor-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .row {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
  }

  .effects {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(118px, 1fr));
    gap: 6px;
  }

  .effect {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 36px;
    padding: 0 10px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--line-strong);
    background: var(--raised);
    font-size: 13px;
  }

  .effect:hover:not(:disabled) {
    background: var(--hover);
  }

  .effect.active {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, var(--raised));
  }

  .dot {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--c);
    flex: none;
  }

  .colors {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }

  .picker {
    width: 42px;
    height: 32px;
    border-radius: var(--radius-sm);
    background: var(--c);
    border: 1px solid var(--line-strong);
    position: relative;
    overflow: hidden;
    cursor: pointer;
  }

  .picker input {
    position: absolute;
    inset: 0;
    opacity: 0;
    cursor: pointer;
  }

  .hexin {
    width: 96px;
    text-transform: uppercase;
  }

  .swatches {
    display: flex;
    gap: 6px;
  }

  .swatch {
    width: 22px;
    height: 22px;
    border-radius: 50%;
    border: 2px solid transparent;
    background: var(--c);
    padding: 0;
  }

  .swatch.active {
    border-color: var(--text);
  }

  .sliders {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 20px;
  }

  .sliders input {
    width: 100%;
  }

  .quick {
    display: flex;
    gap: 8px;
    padding-top: 14px;
    border-top: 1px solid var(--line);
  }

  .presets {
    margin-top: 26px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .save {
    display: flex;
    gap: 10px;
    padding: 12px;
  }

  .save .input {
    flex: 1;
  }

  .preset-list {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
    gap: 10px;
  }

  .preset {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
  }

  .strip {
    display: flex;
    gap: 3px;
  }

  .seg {
    width: 10px;
    height: 22px;
    border-radius: 3px;
    background: var(--c);
  }

  .pname {
    flex: 1;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Effect previews */
  .fx-1 {
    animation: pulse var(--dur) ease-in-out infinite;
  }

  .fx-4 {
    animation: flash var(--dur) steps(1) infinite;
  }

  .fx-11 {
    animation: dflash var(--dur) linear infinite;
  }

  .fx-9 {
    background: hsl(0 90% 60%);
    animation: cycle calc(var(--dur) * 2) linear infinite;
  }

  .fx-72 {
    background: linear-gradient(90deg, #ff3b3b, #ffd23b, #3bff8a, #3bc8ff, #a63bff, #ff3b3b);
    background-size: 300% 100%;
    animation: wave var(--dur) linear infinite;
  }

  .fx-8 {
    background: var(--raised);
    box-shadow: inset 0 0 0 1px var(--line-strong);
    opacity: 1;
  }

  @keyframes pulse {
    50% {
      opacity: 0.12;
    }
  }

  @keyframes flash {
    50% {
      opacity: 0.08;
    }
  }

  @keyframes dflash {
    0%,
    10%,
    20%,
    30% {
      opacity: var(--dim, 1);
    }
    5%,
    15%,
    30.01%,
    100% {
      opacity: 0.08;
    }
  }

  @keyframes cycle {
    to {
      filter: hue-rotate(360deg);
    }
  }

  @keyframes wave {
    to {
      background-position: 300% 0;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .fx-1,
    .fx-4,
    .fx-11,
    .fx-9,
    .fx-72 {
      animation: none;
    }
  }
</style>
