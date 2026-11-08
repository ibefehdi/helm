<script lang="ts">
  import { onMount } from 'svelte';
  import { app } from '$lib/app.svelte';
  import { api, errorMessage } from '$lib/api';
  import { heat } from '$lib/format';
  import Icon from '$lib/components/Icon.svelte';
  import type { Disk } from '$lib/types';

  let disks = $state<Disk[]>([]);
  let loading = $state(true);
  let loadError = $state<string | null>(null);
  let open = $state<number | null>(null);
  let trimming = $state<string | null>(null);

  async function load(quiet = false) {
    if (!quiet) loading = true;
    try {
      disks = await api.storageScan();
      loadError = null;
    } catch (e) {
      loadError = errorMessage(e);
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    void load();
    const id = setInterval(() => void load(true), 30_000);
    return () => clearInterval(id);
  });

  function bytes(n: number | null, digits = 1): string {
    if (n === null) return '—';
    const units = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];
    let i = 0;
    while (n >= 1000 && i < units.length - 1) {
      n /= 1000;
      i++;
    }
    return `${n.toFixed(i >= 3 ? digits : 0)} ${units[i]}`;
  }

  function hours(h: number | null): string {
    if (h === null) return '—';
    if (h < 48) return `${h} h`;
    const days = h / 24;
    return days < 365 ? `${Math.round(days)} days` : `${(days / 365).toFixed(1)} years`;
  }

  function count(n: number | null): string {
    return n === null ? '—' : n.toLocaleString();
  }

  function tone(d: Disk) {
    return d.healthLabel === 'Good' ? 'ok' : d.healthLabel === 'Caution' ? 'warn' : d.healthLabel === 'Bad' ? 'danger' : '';
  }

  /** Rough endurance projection from the drive's own wear counter and its power-on time. */
  function lifeLeft(d: Disk): string | null {
    if (d.percentageUsed === null || d.percentageUsed < 1 || d.powerOnHours === null || d.powerOnHours < 200) return null;
    const perHour = d.percentageUsed / d.powerOnHours;
    const remaining = (100 - d.percentageUsed) / perHour;
    // Drives mostly sit idle; assume the observed duty cycle continues.
    return hours(Math.round(remaining));
  }

  async function trim(letter: string) {
    trimming = letter;
    const msg = await app.run(() => api.storageTrim(letter));
    if (msg) app.toast('success', `${letter}: ${msg}`);
    trimming = null;
  }
</script>

<header>
  <div>
    <h1>Storage</h1>
    <p class="muted">Health straight from each drive's own NVMe log or SMART data. Nothing is written to the drives.</p>
  </div>
  <button class="btn" onclick={() => load()} disabled={loading}><Icon name="refresh" size={14} />Refresh</button>
</header>

{#if loading && disks.length === 0}
  <div class="card empty"><p class="muted">Reading drives…</p></div>
{:else if loadError}
  <div class="card empty"><Icon name="warn" size={24} /><p>{loadError}</p></div>
{:else}
  <div class="list">
    {#each disks as d (d.index)}
      {@const expanded = open === d.index}
      {@const left = lifeLeft(d)}
      <article class={['card disk', tone(d)]}>
        <button class="summary" onclick={() => (open = expanded ? null : d.index)} aria-expanded={expanded}>
          <div class="ring" style:--p={(d.health ?? 0) / 100}>
            <span class="num">{d.health ?? '—'}{d.health !== null ? '%' : ''}</span>
          </div>
          <div class="ident">
            <span class="model">{d.model}</span>
            <span class="faint">
              <span class="chip">{d.bus}</span>
              {bytes(d.size, 0)} · Disk {d.index}{d.volumes.length ? ` · ${d.volumes.map((v) => `${v.letter}:`).join(' ')}` : ''}
            </span>
          </div>
          <div class="stat">
            <span class="faint">Health</span>
            <span class={['state', tone(d)]}>{d.healthLabel}</span>
          </div>
          <div class="stat">
            <span class="faint">Temperature</span>
            <span class="num" style:color={heat(d.temperature)}>{d.temperature !== null ? `${d.temperature}°C` : '—'}</span>
          </div>
          <div class="stat">
            <span class="faint">Written</span>
            <span class="num">{bytes(d.bytesWritten)}</span>
          </div>
          <div class="stat">
            <span class="faint">Powered on</span>
            <span class="num">{hours(d.powerOnHours)}</span>
          </div>
          <span class={['chev', { expanded }]}>›</span>
        </button>

        {#if d.warnings.length}
          <ul class="warnings">
            {#each d.warnings as w (w)}
              <li><Icon name="warn" size={13} />{w}</li>
            {/each}
          </ul>
        {/if}

        {#if expanded}
          <div class="details">
            <div class="facts">
              <div><span class="faint">Model</span><span>{d.model}</span></div>
              <div><span class="faint">Serial</span><span class="num sel">{d.serial || '—'}</span></div>
              <div><span class="faint">Firmware</span><span class="num">{d.firmware || '—'}</span></div>
              <div><span class="faint">Wear used</span><span class="num">{d.percentageUsed !== null ? `${d.percentageUsed}%` : '—'}</span></div>
              {#if left}<div><span class="faint">Projected life</span><span class="num">~{left} at this pace</span></div>{/if}
              {#if d.availableSpare !== null}
                <div><span class="faint">Spare blocks</span><span class="num">{d.availableSpare}% (alarm at {d.spareThreshold}%)</span></div>
              {/if}
              <div><span class="faint">Data read</span><span class="num">{bytes(d.bytesRead)}</span></div>
              <div><span class="faint">Data written</span><span class="num">{bytes(d.bytesWritten)}</span></div>
              <div><span class="faint">Power cycles</span><span class="num">{count(d.powerCycles)}</span></div>
              <div><span class="faint">Unsafe shutdowns</span><span class="num">{count(d.unsafeShutdowns)}</span></div>
              {#if d.mediaErrors !== null}<div><span class="faint">Media errors</span><span class={['num', { bad: (d.mediaErrors ?? 0) > 0 }]}>{count(d.mediaErrors)}</span></div>{/if}
              {#if d.errorLogEntries !== null}<div><span class="faint">Error log entries</span><span class="num">{count(d.errorLogEntries)}</span></div>{/if}
              {#if d.temperatureWarning !== null}
                <div><span class="faint">Temp limits</span><span class="num">warn {d.temperatureWarning}°C · critical {d.temperatureCritical ?? '—'}°C</span></div>
              {/if}
            </div>

            {#if d.volumes.length}
              <h2>Volumes</h2>
              <div class="volumes">
                {#each d.volumes as v (v.letter)}
                  {@const used = v.size ? (v.size - v.free) / v.size : 0}
                  <div class="volume">
                    <span class="vletter">{v.letter}:</span>
                    <div class="vinfo">
                      <span>{v.label || 'Local disk'} <span class="faint">{v.fileSystem}</span></span>
                      <div class="bar"><span style:width={`${used * 100}%`} class={{ full: used > 0.9 }}></span></div>
                      <span class="faint num">{bytes(v.free)} free of {bytes(v.size)}</span>
                    </div>
                    {#if v.fileSystem === 'NTFS' || v.fileSystem === 'ReFS'}
                      <button class="btn sm" disabled={trimming !== null} onclick={() => trim(v.letter)} title="Tell the SSD which blocks are free (TRIM), like Windows' weekly optimisation">
                        {trimming === v.letter ? 'Optimising…' : 'Run TRIM'}
                      </button>
                    {/if}
                  </div>
                {/each}
              </div>
            {/if}

            {#if d.smart.length}
              <h2>SMART attributes</h2>
              <table>
                <thead>
                  <tr><th>ID</th><th>Attribute</th><th>Value</th><th>Worst</th><th>Threshold</th><th>Raw</th></tr>
                </thead>
                <tbody>
                  {#each d.smart as a (a.id)}
                    <tr class={{ failing: a.failing }}>
                      <td class="num faint">{a.id.toString(16).toUpperCase().padStart(2, '0')}</td>
                      <td>{a.name}</td>
                      <td class="num">{a.current}</td>
                      <td class="num">{a.worst}</td>
                      <td class="num">{a.threshold || '—'}</td>
                      <td class="num">{a.raw.toLocaleString()}</td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            {/if}
          </div>
        {/if}
      </article>
    {/each}
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

  .empty {
    padding: 40px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    color: var(--muted);
  }

  .list {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .disk {
    --tone: var(--muted);
    overflow: hidden;
  }

  .disk.ok {
    --tone: var(--ok);
  }

  .disk.warn {
    --tone: var(--warn);
    border-color: color-mix(in srgb, var(--warn) 35%, transparent);
  }

  .disk.danger {
    --tone: var(--danger);
    border-color: color-mix(in srgb, var(--danger) 45%, transparent);
  }

  .summary {
    width: 100%;
    display: grid;
    grid-template-columns: 56px minmax(220px, 2fr) repeat(4, minmax(90px, 1fr)) 16px;
    align-items: center;
    gap: 16px;
    padding: 14px 18px;
    border: none;
    background: transparent;
    text-align: left;
  }

  .summary:hover {
    background: var(--hover);
  }

  .ring {
    width: 52px;
    height: 52px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background:
      radial-gradient(closest-side, var(--surface) 80%, transparent 82%),
      conic-gradient(var(--tone) calc(var(--p) * 360deg), var(--line) 0);
    font-size: 12.5px;
    font-weight: 650;
  }

  .ident {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }

  .ident .faint {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
  }

  .model {
    font-weight: 600;
    font-size: 14px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .stat {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: 13px;
  }

  .stat .faint {
    font-size: 11.5px;
  }

  .state {
    font-weight: 600;
    color: var(--tone);
  }

  .chev {
    color: var(--faint);
    font-size: 18px;
    transition: transform 0.15s;
  }

  .chev.expanded {
    transform: rotate(90deg);
  }

  .warnings {
    list-style: none;
    margin: 0;
    padding: 0 18px 12px 90px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    color: var(--warn);
    font-size: 12.5px;
  }

  .warnings li {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .details {
    border-top: 1px solid var(--line);
    padding: 16px 18px 18px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .facts {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(250px, 1fr));
    gap: 8px 24px;
    font-size: 13px;
  }

  .facts div {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    padding-bottom: 6px;
    border-bottom: 1px dashed var(--line);
  }

  .sel {
    user-select: text;
  }

  .bad {
    color: var(--danger);
  }

  .volumes {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .volume {
    display: flex;
    align-items: center;
    gap: 14px;
  }

  .vletter {
    font-size: 18px;
    font-weight: 650;
    width: 34px;
  }

  .vinfo {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 12.5px;
  }

  .bar {
    height: 6px;
    border-radius: 3px;
    background: var(--line);
    overflow: hidden;
  }

  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
  }

  .bar span.full {
    background: var(--warn);
  }

  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 12.5px;
  }

  th {
    text-align: left;
    font-weight: 600;
    color: var(--muted);
    padding: 6px 8px;
    border-bottom: 1px solid var(--line);
  }

  td {
    padding: 5px 8px;
    border-bottom: 1px solid var(--line);
  }

  tr.failing td {
    color: var(--danger);
  }
</style>
