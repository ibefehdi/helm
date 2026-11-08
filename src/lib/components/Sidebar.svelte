<script lang="ts">
  import { app, type View } from '$lib/app.svelte';
  import { api } from '$lib/api';
  import Icon, { type IconName } from './Icon.svelte';

  const items: { view: View; label: string; icon: IconName }[] = [
    { view: 'overview', label: 'Overview', icon: 'overview' },
    { view: 'fans', label: 'Fans', icon: 'fan' },
    { view: 'sensors', label: 'Sensors', icon: 'sensors' },
    { view: 'lighting', label: 'Lighting', icon: 'light' },
    { view: 'overclock', label: 'Tuning', icon: 'chip' },
    { view: 'graphics', label: 'Graphics', icon: 'gpu' },
    { view: 'devices', label: 'Devices', icon: 'device' },
    { view: 'storage', label: 'Storage', icon: 'disk' },
    { view: 'updates', label: 'Updates', icon: 'update' },
    { view: 'profiles', label: 'Profiles', icon: 'profiles' },
    { view: 'alerts', label: 'Alerts', icon: 'alerts' },
    { view: 'settings', label: 'Settings', icon: 'settings' },
  ];

  const alertCount = $derived(Object.keys(app.activeAlerts).length);
  const manualCount = $derived(Object.keys(app.overrides.manual).length);
  const board = $derived(app.info ? `Board ${app.info.groupNumber}` : '');
</script>

<nav>
  <div class="brand">
    <svg width="22" height="22" viewBox="0 0 24 24" aria-hidden="true">
      <circle cx="12" cy="12" r="9" fill="none" stroke="var(--accent)" stroke-width="2" />
      <path d="M12 3v18M3 12h18M5.6 5.6l12.8 12.8M18.4 5.6 5.6 18.4" stroke="var(--accent)" stroke-width="1.4" opacity="0.55" />
      <circle cx="12" cy="12" r="3" fill="var(--accent)" />
    </svg>
    <span>Helm</span>
  </div>

  <ul>
    {#each items as item (item.view)}
      <li>
        <button class={{ active: app.view === item.view }} onclick={() => (app.view = item.view)}>
          <Icon name={item.icon} />
          <span>{item.label}</span>
          {#if item.view === 'alerts' && alertCount}
            <span class="badge danger">{alertCount}</span>
          {/if}
        </button>
      </li>
    {/each}
  </ul>

  <div class="foot">
    {#if app.overrides.failsafe}
      <div class="notice danger">
        <Icon name="shield" size={14} />
        <span>Failsafe: all fans 100%</span>
      </div>
    {:else if app.overrides.calibrating !== null}
      <div class="notice warn">
        <Icon name="gauge" size={14} />
        <span>Calibrating {app.fanTitle(app.overrides.calibrating)}</span>
      </div>
    {:else if manualCount}
      <div class="notice warn">
        <Icon name="bolt" size={14} />
        <span>{manualCount} fan{manualCount > 1 ? 's' : ''} forced</span>
        <button class="btn ghost sm" onclick={() => app.run(() => api.releaseFans())}>Release</button>
      </div>
    {/if}

    <div class="profile">
      <span class="faint">Profile</span>
      <span class="name">{app.activeProfile?.name ?? 'Custom'}</span>
    </div>

    <div class="conn">
      <span class={['led', { ok: app.status.phase === 'ready', bad: app.status.phase === 'error' }]}></span>
      <span class="faint">{app.status.phase === 'ready' ? board || 'Connected' : app.status.phase}</span>
    </div>
  </div>
</nav>

<style>
  nav {
    width: 212px;
    flex: none;
    display: flex;
    flex-direction: column;
    padding: 16px 10px;
    border-right: 1px solid var(--line);
    background: var(--surface);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 10px 20px;
    font-weight: 650;
    font-size: 16px;
    letter-spacing: 0.01em;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }

  li button {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 10px;
    height: 36px;
    padding: 0 10px;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--muted);
    font-size: 13.5px;
    text-align: left;
  }

  li button:hover {
    background: var(--hover);
    color: var(--text);
  }

  li button.active {
    background: var(--raised);
    color: var(--text);
    box-shadow: inset 2px 0 0 var(--accent);
  }

  li button span:not(.badge) {
    flex: 1;
  }

  .badge {
    min-width: 18px;
    height: 18px;
    padding: 0 5px;
    border-radius: 9px;
    font-size: 11px;
    font-weight: 700;
    display: grid;
    place-items: center;
  }

  .badge.danger {
    background: var(--danger);
    color: #2a0a0a;
  }

  .foot {
    margin-top: auto;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 0 6px;
  }

  .notice {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px;
    border-radius: var(--radius-sm);
    font-size: 12px;
    font-weight: 500;
  }

  .notice span {
    flex: 1;
  }

  .notice.danger {
    color: var(--danger);
    background: color-mix(in srgb, var(--danger) 10%, transparent);
  }

  .notice.warn {
    color: var(--warn);
    background: color-mix(in srgb, var(--warn) 10%, transparent);
  }

  .profile {
    display: flex;
    flex-direction: column;
    font-size: 12px;
    padding: 0 4px;
  }

  .profile .name {
    font-weight: 600;
    font-size: 13px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .conn {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 11.5px;
    padding: 0 4px;
    text-transform: capitalize;
  }

  .led {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--warn);
  }

  .led.ok {
    background: var(--ok);
    box-shadow: 0 0 8px var(--ok);
  }

  .led.bad {
    background: var(--danger);
  }
</style>
