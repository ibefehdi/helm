<script lang="ts">
  import { app } from '$lib/app.svelte';
  import { api } from '$lib/api';
  import Icon from '$lib/components/Icon.svelte';
  import ThemeSwitch from '$lib/components/ThemeSwitch.svelte';
  import Toggle from '$lib/components/Toggle.svelte';
  import type { Settings } from '$lib/types';

  const s = $derived(app.settings);
  const logDir = api.logDir().catch(() => '');

  function set<K extends keyof Settings>(key: K, value: Settings[K]) {
    void app.saveSettings({ [key]: value } as Partial<Settings>);
  }

  async function reconnect() {
    await app.run(() => api.reconnect(), 'Reconnected to the engine');
  }
</script>

<header>
  <h1>Settings</h1>
</header>

<section>
  <h2>Appearance</h2>
  <div class="card group">
    <div class="row">
      <div class="text">
        <span>Theme</span>
        <span class="muted">Light or dark. Saved in this browser only, not in the engine settings.</span>
      </div>
      <ThemeSwitch />
    </div>
  </div>
</section>

{#if s}
  <section>
    <h2>Monitoring</h2>
    <div class="card group">
      <div class="row">
        <div class="text">
          <span>Polling interval</span>
          <span class="muted">How often sensors are read. Lower is smoother but costs a little CPU in the engine service.</span>
        </div>
        <select class="input" value={s.pollMs} onchange={(e) => set('pollMs', Number(e.currentTarget.value))}>
          {#each [250, 500, 1000, 2000, 5000] as ms (ms)}
            <option value={ms}>{ms < 1000 ? `${ms} ms` : `${ms / 1000} s`}</option>
          {/each}
        </select>
      </div>
      <div class="row">
        <div class="text">
          <span>History length</span>
          <span class="muted">Kept in memory for charts and min / max / average.</span>
        </div>
        <select class="input" value={s.historyMinutes} onchange={(e) => set('historyMinutes', Number(e.currentTarget.value))}>
          {#each [5, 15, 30, 60, 120, 240] as m (m)}
            <option value={m}>{m < 60 ? `${m} min` : `${m / 60} h`}</option>
          {/each}
        </select>
      </div>
      <div class="row">
        <div class="text">
          <span>Log sensors to CSV</span>
          <span class="muted num path">{#await logDir then dir}{dir}{/await}</span>
        </div>
        <button class="btn sm" onclick={() => app.run(() => api.openLogDir())}><Icon name="folder" size={13} />Open</button>
        <Toggle checked={s.logCsv} label="Log sensors to CSV" onchange={(v) => set('logCsv', v)} />
      </div>
    </div>
  </section>

  <section>
    <h2>Window &amp; startup</h2>
    <div class="card group">
      <div class="row">
        <div class="text">
          <span>Start with Windows</span>
          <span class="muted">Runs elevated at sign-in through Task Scheduler, so there's no UAC prompt every boot.</span>
        </div>
        <Toggle checked={s.startWithWindows} label="Start with Windows" onchange={(v) => set('startWithWindows', v)} />
      </div>
      <div class="row">
        <div class="text">
          <span>Start minimized to tray</span>
          <span class="muted">When launched at sign-in.</span>
        </div>
        <Toggle checked={s.startMinimized} label="Start minimized" onchange={(v) => set('startMinimized', v)} />
      </div>
      <div class="row">
        <div class="text">
          <span>Close to tray</span>
          <span class="muted">Keep monitoring and alerts running when the window is closed.</span>
        </div>
        <Toggle checked={s.closeToTray} label="Close to tray" onchange={(v) => set('closeToTray', v)} />
      </div>
      <div class="row">
        <div class="text">
          <span>Apply profile on startup</span>
          <span class="muted">Fan curves live in the controller and survive reboots; this re-applies one anyway.</span>
        </div>
        <select
          class="input"
          value={s.applyProfileOnStart ?? ''}
          onchange={(e) => set('applyProfileOnStart', e.currentTarget.value || null)}
        >
          <option value="">None</option>
          {#each app.profiles as p (p.id)}
            <option value={p.id}>{p.name}</option>
          {/each}
        </select>
      </div>
    </div>
  </section>

  <section>
    <h2>Engine</h2>
    <div class="card group">
      <div class="row">
        <div class="text">
          <span>Connection</span>
          <span class="muted">
            {app.status.phase === 'ready' ? 'Connected' : app.status.message ?? app.status.phase}
            {#if app.status.serviceState}· service {app.status.serviceState.toLowerCase()}{/if}
            {#if app.status.bridgeVersion}· bridge {app.status.bridgeVersion}{/if}
          </span>
        </div>
        <button class="btn sm" onclick={reconnect}><Icon name="refresh" size={13} />Reconnect</button>
      </div>
      {#if app.info}
        <div class="row">
          <div class="text">
            <span>Controller</span>
            <span class="muted num">
              Group {app.info.groupNumber} · {app.info.fanCount} fan headers · {app.info.temperatureInputCount} temperature inputs · {app.info.voltageInputCount}
              voltage inputs
            </span>
          </div>
        </div>
      {/if}
      <div class="row">
        <div class="text">
          <span>Calibration data</span>
          <span class="muted">{Object.keys(s.calibration).length} header(s) calibrated.</span>
        </div>
        <button class="btn sm danger" disabled={!Object.keys(s.calibration).length} onclick={() => set('calibration', {})}>Clear</button>
      </div>
    </div>
  </section>

  <p class="about faint">
    Helm talks to GIGABYTE's own EasyTune Engine service, so it uses the same driver path as Control Center. Curves are written to the
    Super I/O controller and keep working when Helm is closed.
  </p>
{/if}

<style>
  header {
    margin-bottom: 8px;
  }

  section {
    max-width: 820px;
  }

  h2 {
    margin: 22px 0 10px;
  }

  .group {
    overflow: hidden;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 12px 16px;
    min-height: 56px;
  }

  .row + .row {
    border-top: 1px solid var(--line);
  }

  .text {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .text .muted {
    font-size: 12.5px;
  }

  .path {
    font-size: 11.5px;
    user-select: text;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .about {
    max-width: 820px;
    margin-top: 24px;
    font-size: 12px;
  }
</style>
