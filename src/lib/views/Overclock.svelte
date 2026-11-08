<script lang="ts">
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { app } from '$lib/app.svelte';
  import { api, errorMessage } from '$lib/api';
  import { bounds, decimals, decode, discreteValues, display, encode, snap, unitOf } from '$lib/oc';
  import Icon from '$lib/components/Icon.svelte';
  import type { OcItem, OcState } from '$lib/types';

  const ACK_KEY = 'helm.tuning.acknowledged';

  const sections: { title: string; blurb: string; keys: string[] }[] = [
    { title: 'Processor', blurb: 'Applied immediately. Lost at the next restart unless set in the BIOS.', keys: ['ratio', 'bclk'] },
    { title: 'Core voltage', blurb: 'Pick a mode first; the value fields unlock to match it.', keys: ['vcoreMode', 'vcore', 'vcoreOffset'] },
    { title: 'Memory', blurb: 'Speed and EXPO/XMP are written to the BIOS and take effect after a restart.', keys: ['expo', 'memRatio', 'dramVdd', 'dramVddq', 'dramVpp'] },
  ];

  let oc = $state<OcState | null>(null);
  let loading = $state(true);
  let loadError = $state<string | null>(null);
  let staged = $state<Record<number, number>>({});
  let applying = $state(false);
  let rebootAck = $state(false);
  let acknowledged = $state(localStorage.getItem(ACK_KEY) === '1');
  let countdown = $state<number | null>(null);
  let rebootPending = $state(false);
  let timer: ReturnType<typeof setInterval> | undefined;

  const byKey = $derived(new Map((oc?.items ?? []).filter((i) => i.key).map((i) => [i.key!, i])));
  const readOnly = $derived((oc?.items ?? []).filter((i) => !i.key));
  const changes = $derived(
    Object.entries(staged)
      .map(([id, value]) => ({ appId: +id, value }))
      .filter((c) => oc?.items.find((i) => i.appId === c.appId)?.current !== c.value),
  );
  const needsReboot = $derived(changes.some((c) => oc?.items.find((i) => i.appId === c.appId)?.live === false));
  const liveChanged = $derived((oc?.items ?? []).some((i) => i.key && i.live && i.current !== i.boot));
  const bclk = $derived(byKey.get('bclk'));

  async function load() {
    loading = true;
    loadError = null;
    try {
      oc = await api.ocGet();
      if (oc.pendingConfirm && countdown === null) startCountdown(15);
    } catch (e) {
      loadError = errorMessage(e);
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    void load();
    const unlisten = listen<{ ok: boolean; error: string | null }>('oc-reverted', (e) => {
      stopCountdown();
      if (e.payload.ok) app.toast('warn', 'No confirmation, so the tuning changes were reverted.');
      else app.toast('error', `Automatic revert failed: ${e.payload.error}`);
      void load();
    });
    return () => {
      stopCountdown();
      void unlisten.then((f) => f());
    };
  });

  function startCountdown(secs: number) {
    stopCountdown();
    countdown = secs;
    timer = setInterval(() => {
      if (countdown !== null && countdown > 0) countdown--;
    }, 1000);
  }

  function stopCountdown() {
    clearInterval(timer);
    countdown = null;
  }

  function value(item: OcItem): number {
    return staged[item.appId] ?? item.current;
  }

  function stage(item: OcItem, raw: number) {
    if (raw === item.current) {
      const { [item.appId]: _, ...rest } = staged;
      staged = rest;
    } else {
      staged = { ...staged, [item.appId]: raw };
    }
  }

  function lockReason(item: OcItem): string | null {
    if (item.editable) return null;
    if (item.key === 'bclk') return 'Base clock is locked by the platform (the clock generator is read-only).';
    if (item.key === 'vcore') return 'Set Vcore mode to Static to edit.';
    if (item.key === 'vcoreOffset') return 'Set Vcore mode to Dynamic Offset to edit.';
    return 'Locked by the platform.';
  }

  async function apply() {
    if (needsReboot && !rebootAck) return;
    applying = true;
    try {
      const ordered = [...changes].sort((a, b) => (b.appId === 32773 ? 1 : 0) - (a.appId === 32773 ? 1 : 0));
      const result = await api.ocApply(ordered, needsReboot);
      oc = { ...oc!, items: result.items };
      staged = {};
      rebootAck = false;
      if (result.mismatched.length) app.toast('warn', `Didn't stick: ${result.mismatched.join(', ')}`);
      if (result.rebootRequired || needsReboot) rebootPending = true;
      if (result.revertInSecs) startCountdown(result.revertInSecs);
      else app.toast('success', result.changed ? 'Tuning applied' : 'Nothing changed');
    } catch (e) {
      app.toast('error', errorMessage(e));
    } finally {
      applying = false;
    }
  }

  async function keep() {
    const kept = await api.ocConfirm();
    stopCountdown();
    app.toast(kept ? 'success' : 'warn', kept ? 'Keeping the new settings until the next restart' : 'Too late, the settings were already reverted');
    if (!kept) void load();
  }

  async function restore() {
    stopCountdown();
    const result = await app.run(() => api.ocRestore(), 'Restored the values from when Helm started');
    if (result) oc = { ...oc!, items: result.items };
    staged = {};
  }

  function acknowledge() {
    localStorage.setItem(ACK_KEY, '1');
    acknowledged = true;
  }
</script>

{#snippet control(item: OcItem)}
  {@const v = value(item)}
  {@const lock = lockReason(item)}
  {@const changed = staged[item.appId] !== undefined && staged[item.appId] !== item.current}
  {@const discrete = discreteValues(item)}
  <div class={['item', { changed, locked: !!lock }]}>
    <div class="label">
      <span class="title">{item.title}</span>
      <span class="faint sub">
        {#if !item.live}<span class="chip warn">Restart</span>{/if}
        {#if item.current !== item.boot}At start: {display(item, item.boot)}{:else}Default {display(item, item.default)}{/if}
      </span>
    </div>

    <div class="input-wrap">
      {#if item.options.length}
        <select class="input" disabled={!!lock} value={v} onchange={(e) => stage(item, +e.currentTarget.value)}>
          {#each item.options as o (o.raw)}
            <option value={o.raw}>{o.label}</option>
          {/each}
        </select>
      {:else if discrete}
        <select class="input" disabled={!!lock} value={v} onchange={(e) => stage(item, +e.currentTarget.value)}>
          {#each discrete as d (d)}
            <option value={d}>{display(item, d)}</option>
          {/each}
        </select>
      {:else}
        {@const b = bounds(item)}
        <input
          type="range"
          min={b.min}
          max={b.max}
          step={b.step}
          disabled={!!lock}
          value={decode(item, v)}
          oninput={(e) => stage(item, encode(item, snap(item, +e.currentTarget.value)))}
        />
        <input
          class="input num"
          type="number"
          min={b.min}
          max={b.max}
          step={b.step}
          disabled={!!lock}
          value={decode(item, v).toFixed(decimals(item))}
          onchange={(e) => stage(item, encode(item, snap(item, +e.currentTarget.value)))}
        />
        <span class="unit faint">{unitOf(item).trim()}</span>
      {/if}
    </div>

    <div class="meta">
      {#if lock}
        <span class="faint">{lock}</span>
      {:else if item.key === 'ratio' && bclk}
        <span class="muted num">≈ {Math.round(decode(item, v) * decode(bclk, bclk.current))} MHz</span>
      {:else if item.limits?.max != null}
        <span class="faint">Helm cap {item.limits.max}{unitOf(item)}</span>
      {:else if item.limits?.maxAbs != null}
        <span class="faint">Helm cap ±{item.limits.maxAbs}{unitOf(item)}</span>
      {/if}
      {#if changed}
        <button class="btn ghost sm" onclick={() => stage(item, item.current)}><Icon name="undo" size={12} />{display(item, item.current)}</button>
      {/if}
    </div>
  </div>
{/snippet}

<header>
  <div>
    <h1>Tuning</h1>
    <p class="muted">CPU and memory overclocking through GIGABYTE's EasyTune engine, with stricter safety limits than EasyTune itself.</p>
  </div>
  <div class="actions">
    <button class="btn" onclick={load} disabled={loading}><Icon name="refresh" size={14} />Reload</button>
    <button class="btn" disabled={!liveChanged || applying} onclick={restore}><Icon name="undo" size={14} />Restore start values</button>
  </div>
</header>

{#if !acknowledged}
  <div class="card ack">
    <Icon name="warn" size={22} />
    <div>
      <h3>Before you tune</h3>
      <p class="muted">
        Raising clocks or voltages can make the system unstable, shorten component life, and may void your warranty. Helm caps
        voltages below EasyTune's limits and reverts live changes automatically unless you confirm them, but nothing can make an
        overclock risk-free.
      </p>
    </div>
    <button class="btn primary" onclick={acknowledge}>I understand</button>
  </div>
{:else if loading && !oc}
  <div class="card empty"><p class="muted">Reading the tuning tables…</p></div>
{:else if loadError}
  <div class="card empty">
    <Icon name="warn" size={24} />
    <p>Tuning isn't available</p>
    <p class="muted">{loadError}</p>
  </div>
{:else if oc}
  {#if rebootPending}
    <div class="banner warn">
      <Icon name="power" size={15} />
      <span>Memory settings were written to the BIOS. They take effect after you restart.</span>
    </div>
  {/if}

  <div class="sections">
    {#each sections as s (s.title)}
      {@const items = s.keys.map((k) => byKey.get(k)).filter((i): i is OcItem => !!i)}
      {#if items.length}
        <section class="card section">
          <div class="section-head">
            <h2>{s.title}</h2>
            <p class="faint">{s.blurb}</p>
          </div>
          {#each items as item (item.appId)}
            {@render control(item)}
          {/each}
        </section>
      {/if}
    {/each}

    {#if readOnly.length}
      <section class="card section">
        <div class="section-head">
          <h2>Platform voltages</h2>
          <p class="faint">Shown for reference. Helm doesn't write these.</p>
        </div>
        <div class="ro">
          {#each readOnly as item (item.appId)}
            <div class="ro-item">
              <span class="muted">{item.title}</span>
              <span class="num">{display(item, item.current)}</span>
            </div>
          {/each}
        </div>
      </section>
    {/if}
  </div>

  <div class={['applybar', { visible: changes.length > 0 }]}>
    <span>{changes.length} change{changes.length === 1 ? '' : 's'} staged</span>
    {#if needsReboot}
      <label class="ackbox">
        <input type="checkbox" bind:checked={rebootAck} />
        <span>Write memory settings to the BIOS (applies after a restart)</span>
      </label>
    {/if}
    <div class="spacer"></div>
    <button class="btn ghost" onclick={() => (staged = {})}>Discard</button>
    <button class="btn primary" disabled={applying || (needsReboot && !rebootAck)} onclick={apply}>
      <Icon name="check" size={14} />{applying ? 'Applying…' : 'Apply'}
    </button>
  </div>
{/if}

{#if countdown !== null}
  <div class="scrim" role="presentation">
    <div class="card dialog" role="alertdialog" aria-modal="true" aria-labelledby="keep-title">
      <div class="ring" style:--p={countdown / 20}>
        <span class="num">{countdown}</span>
      </div>
      <h3 id="keep-title">Keep these settings?</h3>
      <p class="muted">If the system is stable, keep them. Otherwise, or if you don't answer, Helm puts the previous values back.</p>
      <div class="dialog-actions">
        <button class="btn" onclick={restore}>Revert now</button>
        <button class="btn primary" onclick={keep}>Keep</button>
      </div>
    </div>
  </div>
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
    gap: 8px;
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

  .banner {
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 10px 14px;
    border-radius: var(--radius);
    margin-bottom: 16px;
    font-size: 13px;
    color: var(--warn);
    background: color-mix(in srgb, var(--warn) 9%, transparent);
    border: 1px solid color-mix(in srgb, var(--warn) 25%, transparent);
  }

  .sections {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding-bottom: 70px;
  }

  .section {
    padding: 6px 0;
  }

  .section-head {
    display: flex;
    align-items: baseline;
    gap: 12px;
    padding: 10px 18px 8px;
  }

  .section-head p {
    font-size: 12px;
  }

  .item {
    display: grid;
    grid-template-columns: minmax(180px, 1fr) minmax(260px, 1.4fr) minmax(160px, 0.9fr);
    align-items: center;
    gap: 16px;
    padding: 10px 18px;
    border-top: 1px solid var(--line);
  }

  .item.changed {
    background: color-mix(in srgb, var(--accent) 5%, transparent);
    box-shadow: inset 2px 0 0 var(--accent);
  }

  .item.locked .title {
    color: var(--muted);
  }

  .label {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .title {
    font-weight: 600;
  }

  .sub {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
  }

  .input-wrap {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .input-wrap input[type='range'] {
    flex: 1;
  }

  .input-wrap select {
    flex: 1;
  }

  .unit {
    width: 30px;
    font-size: 12px;
  }

  .meta {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    font-size: 12px;
    text-align: right;
  }

  .ro {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    gap: 1px;
    border-top: 1px solid var(--line);
  }

  .ro-item {
    display: flex;
    justify-content: space-between;
    padding: 8px 18px;
    font-size: 12.5px;
  }

  .applybar {
    position: sticky;
    bottom: 0;
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 12px 16px;
    margin-top: -56px;
    border-radius: var(--radius);
    border: 1px solid color-mix(in srgb, var(--accent) 40%, transparent);
    background: color-mix(in srgb, var(--accent) 8%, var(--surface));
    box-shadow: 0 -8px 24px rgb(0 0 0 / 40%);
    transform: translateY(140%);
    opacity: 0;
    transition:
      transform 0.18s,
      opacity 0.18s;
    pointer-events: none;
  }

  .applybar.visible {
    transform: none;
    opacity: 1;
    pointer-events: auto;
  }

  .ackbox {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12.5px;
    color: var(--warn);
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
</style>
