<script lang="ts">
  import { app } from '$lib/app.svelte';
  import { api } from '$lib/api';
  import { fixedCurve, isFixed, normalize, pwmAt, pwmToPct, stallPct, validate } from '$lib/curve';
  import { fmt, heat } from '$lib/format';
  import { presetPoints, presetsFor, roleOf } from '$lib/presets';
  import CurveEditor from '$lib/components/CurveEditor.svelte';
  import Icon from '$lib/components/Icon.svelte';
  import type { CurvePoint, FanCurve } from '$lib/types';

  interface Draft {
    points: CurvePoint[];
    hysteresis: number;
    temperatureSource: number;
  }

  let drafts = $state<Record<number, Draft>>({});
  let mode = $state<Record<number, 'curve' | 'fixed'>>({});
  let fixedPct = $state(50);
  let safetyTemp = $state(85);
  let testPct = $state(50);
  let applying = $state(false);
  let renaming = $state(false);
  let nameInput = $state('');

  const index = $derived(Math.min(app.selectedFan, Math.max(0, (app.info?.fanCount ?? 1) - 1)));
  const applied = $derived<FanCurve | undefined>(app.fans[index]);
  const header = $derived(app.info?.fans[index]);
  const draft = $derived<Draft | undefined>(
    drafts[index] ??
      (applied
        ? { points: applied.points.map((p) => [...p] as CurvePoint), hysteresis: applied.hysteresis, temperatureSource: applied.temperatureSource }
        : undefined),
  );

  const role = $derived(roleOf(header?.title ?? '', app.sourceTitle(draft?.temperatureSource ?? -1)));
  const presets = $derived(presetsFor(role));
  const calibration = $derived(app.settings?.calibration[String(index)]);
  const stall = $derived(stallPct(calibration));
  const issues = $derived(draft ? validate(draft.points, stall) : []);
  const blocking = $derived(issues.some((i) => i.level === 'error'));
  const liveTemp = $derived(app.headerTemperature(index));
  const liveRpm = $derived(app.headerRpm(index));
  const currentMode = $derived(mode[index] ?? (draft && isFixed(draft.points) !== null ? 'fixed' : 'curve'));
  const forced = $derived(app.overrides.manual[String(index)]);
  const calibrating = $derived(app.overrides.calibrating === index);
  const busyOverride = $derived(app.overrides.failsafe || app.overrides.calibrating !== null);

  const dirtyIndexes = $derived(
    Object.keys(drafts)
      .map(Number)
      .filter((i) => isDirty(i)),
  );
  const dirty = $derived(dirtyIndexes.includes(index));

  function isDirty(i: number) {
    const d = drafts[i];
    const a = app.fans[i];
    if (!d || !a) return false;
    return (
      d.hysteresis !== a.hysteresis ||
      d.temperatureSource !== a.temperatureSource ||
      JSON.stringify(normalize(d.points)) !== JSON.stringify(a.points)
    );
  }

  function edit(patch: Partial<Draft>, i = index) {
    const base = drafts[i] ?? (i === index ? draft : undefined);
    if (!base) return;
    drafts[i] = { ...$state.snapshot(base), ...patch };
  }

  function setPoints(points: CurvePoint[]) {
    edit({ points });
  }

  function setMode(m: 'curve' | 'fixed') {
    mode[index] = m;
    if (m === 'fixed') {
      const current = draft ? isFixed(draft.points) : null;
      if (current !== null) fixedPct = current;
      edit({ points: fixedCurve(fixedPct, safetyTemp) });
    } else if (draft && isFixed(draft.points) !== null) {
      edit({ points: presetPoints(presets.find((p) => p.id === 'balanced') ?? presets[0]) });
    }
  }

  function updateFixed() {
    edit({ points: fixedCurve(fixedPct, safetyTemp) });
  }

  function revert(i = index) {
    delete drafts[i];
    delete mode[i];
  }

  async function applyIndexes(indexes: number[]) {
    const curves = indexes
      .map((i) => {
        const d = drafts[i];
        const a = app.fans[i];
        return d && a ? { ...$state.snapshot(a), ...$state.snapshot(d), points: normalize($state.snapshot(d).points) } : null;
      })
      .filter((c): c is FanCurve => c !== null);
    if (!curves.length) return;
    applying = true;
    const written = await app.run(() => (curves.length === 1 ? api.setFan(curves[0]).then((c) => [c]) : api.setFans(curves)));
    applying = false;
    if (!written) return;
    const unverified: string[] = [];
    for (const c of written) {
      app.fans[c.index] = c;
      revert(c.index);
      if (c.verified === false) unverified.push(app.fanTitle(c.index));
    }
    if (unverified.length) {
      app.toast('warn', `The controller read back a different curve for ${unverified.join(', ')}. The firmware may have rejected part of it.`);
    } else {
      app.toast('success', written.length === 1 ? `${app.fanTitle(written[0].index)} updated` : `${written.length} fans updated`);
    }
  }

  function copyTo(target: number) {
    const a = app.fans[target];
    if (!draft || !a || target === index) return;
    drafts[target] = {
      points: $state.snapshot(draft.points) as CurvePoint[],
      hysteresis: draft.hysteresis,
      temperatureSource: drafts[target]?.temperatureSource ?? a.temperatureSource,
    };
    app.toast('info', `Copied to ${app.fanTitle(target)} — apply to write it.`);
  }

  function startRename() {
    nameInput = app.fanTitle(index);
    renaming = true;
  }

  async function saveRename() {
    renaming = false;
    if (!app.settings) return;
    const labels = { ...app.settings.fanLabels };
    const trimmed = nameInput.trim();
    if (!trimmed || trimmed === header?.title) delete labels[String(index)];
    else labels[String(index)] = trimmed;
    await app.saveSettings({ fanLabels: labels });
  }

  async function calibrate() {
    const points = await app.run(() => api.calibrate(index));
    if (points) {
      const s = stallPct(points);
      app.toast('success', `Calibrated ${app.fanTitle(index)}: ${Math.round(Math.max(...points.map((p) => p[1])))} rpm max${s !== null ? `, stops at ${s}%` : ''}.`);
    }
  }

  function focusOnMount(node: HTMLInputElement) {
    node.focus();
    node.select();
  }
</script>

<div class="layout">
  <aside class="headers">
    <h2>Headers</h2>
    {#each app.info?.fans ?? [] as h (h.index)}
      {@const rpm = app.headerRpm(h.index)}
      <button class={['header', { active: h.index === index }]} onclick={() => (app.selectedFan = h.index)}>
        <span class="htitle">
          {app.fanTitle(h.index)}
          {#if dirtyIndexes.includes(h.index)}<span class="dot-dirty" title="Unapplied changes"></span>{/if}
        </span>
        <span class="num muted">{fmt(rpm, 'RPM')} rpm</span>
      </button>
    {/each}
    {#if dirtyIndexes.length > 1}
      <button class="btn primary apply-all" disabled={applying} onclick={() => applyIndexes(dirtyIndexes)}>
        Apply {dirtyIndexes.length} changes
      </button>
    {/if}
  </aside>

  {#if draft && applied}
    <div class="main">
      <header>
        <div class="title-row">
          {#if renaming}
            <input
              class="input rename"
              bind:value={nameInput}
              {@attach focusOnMount}
              onblur={saveRename}
              onkeydown={(e) => {
                if (e.key === 'Enter') saveRename();
                if (e.key === 'Escape') renaming = false;
              }}
            />
          {:else}
            <h1>{app.fanTitle(index)}</h1>
            <button class="btn ghost sm" aria-label="Rename header" onclick={startRename}><Icon name="edit" size={14} /></button>
          {/if}
          {#if header && app.fanTitle(index) !== header.title}
            <span class="faint">({header.title})</span>
          {/if}
        </div>
        <div class="live">
          <div>
            <span class="faint">Speed</span>
            <b class="num">{fmt(liveRpm, 'RPM')}</b><small>rpm</small>
          </div>
          <div>
            <span class="faint">{app.sourceTitle(applied.temperatureSource)}</span>
            <b class="num" style:color={heat(liveTemp)}>{fmt(liveTemp, '°C')}</b><small>°C</small>
          </div>
          <div>
            <span class="faint">Duty</span>
            <b class="num">
              {forced !== undefined ? forced : liveTemp !== null ? Math.round(pwmToPct(pwmAt(applied.points, liveTemp))) : '—'}
            </b><small>%</small>
          </div>
        </div>
      </header>

      <div class="card editor-card">
        <div class="toolbar">
          <div class="seg" role="tablist">
            <button role="tab" aria-selected={currentMode === 'curve'} class={{ on: currentMode === 'curve' }} onclick={() => setMode('curve')}>Curve</button>
            <button role="tab" aria-selected={currentMode === 'fixed'} class={{ on: currentMode === 'fixed' }} onclick={() => setMode('fixed')}>Fixed</button>
          </div>
          {#if currentMode === 'curve'}
            <div class="presets">
              {#each presets as p (p.id)}
                <button class="btn sm" onclick={() => setPoints(presetPoints(p))}>{p.label}</button>
              {/each}
            </div>
          {:else}
            <label class="fixed">
              <input type="range" min="0" max="100" bind:value={fixedPct} oninput={updateFixed} />
              <span class="num">{fixedPct}%</span>
            </label>
            <label class="inline">
              <span class="muted">Full speed above</span>
              <input class="input" type="number" min="40" max="100" bind:value={safetyTemp} onchange={updateFixed} />
              <span class="muted">°C</span>
            </label>
          {/if}
        </div>

        <CurveEditor
          {liveTemp}
          {calibration}
          {stall}
          readonly={currentMode === 'fixed'}
          bind:points={() => draft.points, setPoints}
        />

        <p class="hint faint">
          {#if currentMode === 'curve'}
            Drag points · double-click to add (max 8) · right-click or Delete to remove · arrow keys nudge (Shift ×5).
            The last point is always 100% — the controller ignores curves that never reach full speed.
          {:else}
            Holds a constant duty, with a safety ramp to 100% if the sensor passes the limit.
          {/if}
        </p>

        {#if issues.length}
          <ul class="issues">
            {#each issues as issue (issue.message)}
              <li class={issue.level}><Icon name="warn" size={14} />{issue.message}</li>
            {/each}
          </ul>
        {/if}

        <div class="controls">
          <label class="inline">
            <span class="muted">Follows</span>
            <select
              class="input"
              value={draft.temperatureSource}
              onchange={(e) => edit({ temperatureSource: Number(e.currentTarget.value) })}
            >
              {#each app.info?.temperatureSources ?? [] as s (s.index)}
                <option value={s.index}>{s.title}</option>
              {/each}
            </select>
          </label>
          <label class="inline" title="Temperature must fall this many degrees before the fan slows down, to stop it hunting.">
            <span class="muted">Hysteresis</span>
            <input
              class="input"
              type="number"
              min="0"
              max="10"
              value={draft.hysteresis}
              onchange={(e) => edit({ hysteresis: Math.max(0, Math.min(10, Number(e.currentTarget.value) || 0)) })}
            />
            <span class="muted">°C</span>
          </label>
          <label class="inline">
            <span class="muted">Copy to</span>
            <select
              class="input"
              value=""
              onchange={(e) => {
                copyTo(Number(e.currentTarget.value));
                e.currentTarget.value = '';
              }}
            >
              <option value="" disabled>Header…</option>
              {#each (app.info?.fans ?? []).filter((h) => h.index !== index) as h (h.index)}
                <option value={h.index}>{app.fanTitle(h.index)}</option>
              {/each}
            </select>
          </label>
          <div class="spacer"></div>
          <button class="btn" disabled={!dirty || applying} onclick={() => revert()}><Icon name="undo" size={14} />Revert</button>
          <button class="btn primary" disabled={!dirty || blocking || applying} onclick={() => applyIndexes([index])}>
            <Icon name="check" size={14} />{applying ? 'Applying…' : 'Apply'}
          </button>
        </div>
      </div>

      <div class="tools">
        <div class="card tool">
          <h3>Test</h3>
          <p class="muted">Temporarily hold this header at a fixed duty to hear or measure it. Released when you leave Helm.</p>
          <div class="test-row">
            <input type="range" min="0" max="100" step="5" bind:value={testPct} disabled={busyOverride} />
            <span class="num pct">{testPct}%</span>
            <button class="btn" disabled={busyOverride} onclick={() => app.run(() => api.forceFan(index, testPct))}>
              <Icon name="play" size={13} />Hold
            </button>
            <button class="btn" disabled={Object.keys(app.overrides.manual).length === 0 || app.overrides.failsafe} onclick={() => app.run(() => api.releaseFans())}>
              <Icon name="stop" size={13} />Release
            </button>
          </div>
          {#if forced !== undefined}
            <p class="chip warn">Holding at {forced}%</p>
          {/if}
        </div>

        <div class="card tool">
          <h3>Calibrate</h3>
          <p class="muted">
            Sweeps 100%→0% in 10% steps (~45 s) to map duty to RPM and find where the fan stalls. Aborts if the CPU passes 85°C.
          </p>
          {#if calibrating && app.calibration}
            <div class="progress"><span style:width="{(app.calibration.step / app.calibration.total) * 100}%"></span></div>
            <div class="test-row">
              <span class="muted num">{app.calibration.percent}% → {app.calibration.rpm} rpm</span>
              <div class="spacer"></div>
              <button class="btn danger" onclick={() => api.cancelCalibration()}>Cancel</button>
            </div>
          {:else if calibrating}
            <div class="progress indeterminate"><span></span></div>
          {:else}
            <div class="test-row">
              {#if calibration}
                <span class="muted">
                  Max <b class="num">{Math.round(Math.max(...calibration.map((p) => p[1])))}</b> rpm ·
                  {stall !== null ? `stalls ≤ ${stall}%` : 'never stalls'}
                </span>
              {:else}
                <span class="faint">Not calibrated</span>
              {/if}
              <div class="spacer"></div>
              <button class="btn" disabled={busyOverride} onclick={calibrate}><Icon name="gauge" size={14} />{calibration ? 'Recalibrate' : 'Calibrate'}</button>
            </div>
          {/if}
        </div>
      </div>
    </div>
  {:else}
    <div class="main empty muted">Loading fan configuration…</div>
  {/if}
</div>

<style>
  .layout {
    display: flex;
    gap: 24px;
    min-height: 100%;
  }

  .headers {
    width: 190px;
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .headers h2 {
    margin: 6px 0 10px 10px;
  }

  .header {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 1px;
    padding: 8px 10px;
    border-radius: var(--radius-sm);
    border: 1px solid transparent;
    background: transparent;
    text-align: left;
  }

  .header:hover {
    background: var(--surface);
  }

  .header.active {
    background: var(--surface);
    border-color: var(--line-strong);
  }

  .htitle {
    font-weight: 550;
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .header .num {
    font-size: 12px;
  }

  .dot-dirty {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--warn);
  }

  .apply-all {
    margin-top: 12px;
    justify-content: center;
  }

  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .empty {
    display: grid;
    place-items: center;
  }

  header {
    display: flex;
    justify-content: space-between;
    align-items: flex-end;
    gap: 16px;
    flex-wrap: wrap;
  }

  .title-row {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .rename {
    font-size: 18px;
    font-weight: 600;
    width: 260px;
  }

  .live {
    display: flex;
    gap: 28px;
  }

  .live div {
    display: flex;
    flex-direction: column;
    font-size: 12px;
  }

  .live b {
    font-size: 22px;
    font-weight: 600;
    display: inline;
  }

  .live small {
    color: var(--muted);
    margin-left: 2px;
  }

  .live div {
    display: grid;
    grid-template-columns: auto auto;
    justify-content: start;
    align-items: baseline;
  }

  .live div .faint {
    grid-column: 1 / -1;
  }

  .editor-card {
    padding: 14px 16px 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 14px;
    flex-wrap: wrap;
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
    padding: 0 14px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--muted);
    font-size: 12.5px;
    font-weight: 500;
  }

  .seg button.on {
    background: var(--raised);
    color: var(--text);
  }

  .presets {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }

  .fixed {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .fixed input {
    width: 220px;
  }

  .inline {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
  }

  .hint {
    font-size: 12px;
  }

  .issues {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 12.5px;
  }

  .issues li {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .issues .error {
    color: var(--danger);
  }

  .issues .warn {
    color: var(--warn);
  }

  .controls {
    display: flex;
    align-items: center;
    gap: 16px;
    flex-wrap: wrap;
    padding-top: 12px;
    border-top: 1px solid var(--line);
  }

  .spacer {
    flex: 1;
  }

  .tools {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 16px;
  }

  .tool {
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .tool p.muted {
    font-size: 12.5px;
  }

  .test-row {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 13px;
  }

  .test-row input[type='range'] {
    flex: 1;
  }

  .pct {
    width: 40px;
    text-align: right;
  }

  .tool .chip {
    align-self: flex-start;
  }

  .progress {
    height: 6px;
    border-radius: 3px;
    background: var(--raised);
    overflow: hidden;
  }

  .progress span {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 0.3s;
  }

  .progress.indeterminate span {
    width: 30%;
    animation: slide 1.2s ease-in-out infinite;
  }

  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(340%);
    }
  }

  @media (max-width: 1150px) {
    .tools {
      grid-template-columns: 1fr;
    }
  }
</style>
