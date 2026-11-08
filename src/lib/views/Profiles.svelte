<script lang="ts">
  import { open, save } from '@tauri-apps/plugin-dialog';
  import { app } from '$lib/app.svelte';
  import { api } from '$lib/api';
  import { timeAgo } from '$lib/format';
  import Icon from '$lib/components/Icon.svelte';
  import type { Profile } from '$lib/types';

  let newName = $state('');
  let editing = $state<string | null>(null);
  let editName = $state('');
  let confirmDelete = $state<string | null>(null);

  const startup = $derived(app.settings?.applyProfileOnStart ?? null);

  async function saveCurrent() {
    const name = newName.trim() || `Profile ${app.profiles.length + 1}`;
    const p = await app.run(() => api.saveProfile(name), `Saved “${name}”`);
    if (p) newName = '';
  }

  async function rename(p: Profile) {
    const name = editName.trim();
    editing = null;
    if (name && name !== p.name) await app.run(() => api.renameProfile(p.id, name));
  }

  async function remove(p: Profile) {
    confirmDelete = null;
    await app.run(() => api.deleteProfile(p.id), `Deleted “${p.name}”`);
  }

  async function exportProfile(p: Profile) {
    const path = await save({
      defaultPath: `${p.name.replace(/[^\w\- ]+/g, '_')}.helm.json`,
      filters: [{ name: 'Helm profile', extensions: ['json'] }],
    });
    if (path) await app.run(() => api.exportProfile(p.id, path), 'Profile exported');
  }

  async function importFile() {
    const path = await open({ multiple: false, filters: [{ name: 'Helm profile', extensions: ['json'] }] });
    if (typeof path === 'string') {
      const added = await app.run(() => api.importProfileFile(path));
      if (added) app.toast('success', `Imported ${added.length} profile${added.length === 1 ? '' : 's'}`);
    }
  }

  async function importGcc() {
    const added = await app.run(() => api.importGcc());
    if (added) app.toast(added.length ? 'success' : 'info', added.length ? `Imported ${added.length} profile${added.length === 1 ? '' : 's'} from GIGABYTE Control Center` : 'No new Control Center profiles found');
  }

  function setStartup(id: string | null) {
    void app.saveSettings({ applyProfileOnStart: id });
  }

  function miniPath(points: [number, number][]) {
    if (points.length < 2) return '';
    const pts = [[0, points[1][1]], ...points.slice(1), [100, points[points.length - 1][1]]];
    return pts.map(([t, pwm], i) => `${i ? 'L' : 'M'}${t},${30 - (pwm / 255) * 30}`).join('');
  }

  function sourceLabel(p: Profile) {
    if (p.source === 'gcc') return 'From Control Center';
    if (p.source === 'file') return 'Imported';
    return 'Saved';
  }

  function focusOnMount(node: HTMLInputElement) {
    node.focus();
    node.select();
  }
</script>

<header>
  <div>
    <h1>Profiles</h1>
    <p class="muted">Snapshots of every header's curve, source and hysteresis. Apply one in a click or from the tray.</p>
  </div>
  <div class="actions">
    <button class="btn" onclick={importFile}><Icon name="upload" size={14} />Import file</button>
    <button class="btn" onclick={importGcc}><Icon name="download" size={14} />Import from Control Center</button>
  </div>
</header>

<form
  class="card save"
  onsubmit={(e) => {
    e.preventDefault();
    void saveCurrent();
  }}
>
  <input class="input" placeholder="Name for the current fan setup…" bind:value={newName} disabled={!app.ready} />
  <button class="btn primary" type="submit" disabled={!app.ready}><Icon name="plus" size={14} />Save current</button>
</form>

{#if app.profiles.length === 0}
  <div class="empty card">
    <Icon name="profiles" size={26} />
    <p>No profiles yet.</p>
    <p class="muted">Save your current setup above, or import the profiles you made in GIGABYTE Control Center.</p>
  </div>
{:else}
  <div class="grid">
    {#each app.profiles as p (p.id)}
      {@const active = app.settings?.activeProfile === p.id}
      <article class={['card profile', { active }]}>
        <div class="top">
          {#if editing === p.id}
            <input
              class="input"
              bind:value={editName}
              {@attach focusOnMount}
              onblur={() => rename(p)}
              onkeydown={(e) => {
                if (e.key === 'Enter') void rename(p);
                if (e.key === 'Escape') editing = null;
              }}
            />
          {:else}
            <h3>{p.name}</h3>
          {/if}
          <div class="chips">
            {#if active}<span class="chip accent">Active</span>{/if}
            {#if startup === p.id}<span class="chip ok">On startup</span>{/if}
          </div>
        </div>
        <p class="meta faint">{sourceLabel(p)} · {timeAgo(p.createdAt)} · {p.fans.length} headers</p>

        <div class="curves">
          {#each p.fans as f (f.index)}
            <div class="curve" title={app.fanTitle(f.index)}>
              <svg viewBox="0 0 100 30" preserveAspectRatio="none" aria-hidden="true"><path d={miniPath(f.points)} /></svg>
              <span class="faint">{app.fanTitle(f.index)}</span>
            </div>
          {/each}
        </div>

        <div class="bar">
          <button class="btn primary sm" disabled={!app.ready} onclick={() => app.applyProfile(p.id)}>
            <Icon name="check" size={13} />Apply
          </button>
          <button class="btn sm" onclick={() => setStartup(startup === p.id ? null : p.id)}>
            {startup === p.id ? 'Unset startup' : 'Use on startup'}
          </button>
          <div class="spacer"></div>
          <button
            class="btn ghost sm"
            aria-label="Rename"
            title="Rename"
            onclick={() => {
              editing = p.id;
              editName = p.name;
            }}><Icon name="edit" size={14} /></button
          >
          <button class="btn ghost sm" aria-label="Export" title="Export" onclick={() => exportProfile(p)}><Icon name="download" size={14} /></button>
          {#if confirmDelete === p.id}
            <button class="btn danger sm" onclick={() => remove(p)}>Delete?</button>
          {:else}
            <button class="btn ghost sm" aria-label="Delete" title="Delete" onclick={() => (confirmDelete = p.id)}><Icon name="trash" size={14} /></button>
          {/if}
        </div>
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

  .actions {
    display: flex;
    gap: 8px;
  }

  .save {
    display: flex;
    gap: 10px;
    padding: 12px;
    margin-bottom: 20px;
  }

  .save .input {
    flex: 1;
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

  .empty p:first-of-type {
    color: var(--text);
    font-weight: 600;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(340px, 1fr));
    gap: 14px;
  }

  .profile {
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .profile.active {
    border-color: color-mix(in srgb, var(--accent) 45%, transparent);
  }

  .top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }

  .top .input {
    flex: 1;
  }

  .chips {
    display: flex;
    gap: 4px;
  }

  .meta {
    font-size: 12px;
  }

  .curves {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 8px 12px;
    padding: 6px 0;
  }

  .curve {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: 11px;
    min-width: 0;
  }

  .curve span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .curve svg {
    width: 100%;
    height: 30px;
    background: var(--bg);
    border-radius: 4px;
    overflow: visible;
  }

  .curve path {
    fill: none;
    stroke: var(--accent);
    stroke-width: 1.4;
    vector-effect: non-scaling-stroke;
  }

  .bar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding-top: 10px;
    border-top: 1px solid var(--line);
  }

  .spacer {
    flex: 1;
  }
</style>
