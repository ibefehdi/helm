<script lang="ts">
  import { app, type View } from '$lib/app.svelte';
  import { api } from '$lib/api';
  import Icon, { type IconName } from './Icon.svelte';
  import ThemeSwitch from './ThemeSwitch.svelte';

  const groups: { label: string; items: { view: View; label: string; icon: IconName }[] }[] = [
    {
      label: 'Monitor',
      items: [
        { view: 'overview', label: 'Overview', icon: 'overview' },
        { view: 'fans', label: 'Fans', icon: 'fan' },
        { view: 'sensors', label: 'Sensors', icon: 'sensors' },
      ],
    },
    {
      label: 'Control',
      items: [
        { view: 'lighting', label: 'Lighting', icon: 'light' },
        { view: 'overclock', label: 'Tuning', icon: 'chip' },
        { view: 'graphics', label: 'Graphics', icon: 'gpu' },
      ],
    },
    {
      label: 'System',
      items: [
        { view: 'devices', label: 'Devices', icon: 'device' },
        { view: 'storage', label: 'Storage', icon: 'disk' },
        { view: 'updates', label: 'Updates', icon: 'update' },
        { view: 'profiles', label: 'Profiles', icon: 'profiles' },
        { view: 'alerts', label: 'Alerts', icon: 'alerts' },
        { view: 'settings', label: 'Settings', icon: 'settings' },
      ],
    },
  ];

  const alertCount = $derived(Object.keys(app.activeAlerts).length);
  const manualCount = $derived(Object.keys(app.overrides.manual).length);
  const board = $derived(app.info ? `Board ${app.info.groupNumber}` : '');
</script>

<nav>
  <div class="brand">
    <span class="mark" aria-hidden="true">
      <svg width="18" height="18" viewBox="0 0 24 24">
        <circle cx="12" cy="12" r="8.25" fill="none" stroke="currentColor" stroke-width="1.75" />
        <path d="M12 3.75v16.5M3.75 12h16.5" stroke="currentColor" stroke-width="1.25" opacity="0.55" />
        <circle cx="12" cy="12" r="2.4" fill="currentColor" />
      </svg>
    </span>
    <span class="brand-copy">
      <span class="brand-name">Helm</span>
      <span class="brand-sub">Hardware</span>
    </span>
  </div>

  <div class="groups">
    {#each groups as group (group.label)}
      <div class="group">
        <div class="group-label">{group.label}</div>
        <ul>
          {#each group.items as item (item.view)}
            <li>
              <button
                class={{ active: app.view === item.view }}
                aria-current={app.view === item.view ? 'page' : undefined}
                onclick={() => (app.view = item.view)}
              >
                <Icon name={item.icon} />
                <span>{item.label}</span>
                {#if item.view === 'alerts' && alertCount}
                  <span class="badge">{alertCount}</span>
                {/if}
              </button>
            </li>
          {/each}
        </ul>
      </div>
    {/each}
  </div>

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

    <ThemeSwitch compact />

    <div class="conn">
      <span class={['led', { ok: app.status.phase === 'ready', bad: app.status.phase === 'error' }]}></span>
      <span class="faint">{app.status.phase === 'ready' ? board || 'Connected' : app.status.phase}</span>
    </div>
  </div>
</nav>

<style>
  nav {
    width: 260px;
    flex: none;
    display: flex;
    flex-direction: column;
    background: var(--saqr-bg-surface);
    border-right: 1px solid var(--saqr-stroke-soft);
  }

  .brand {
    height: 72px;
    flex: none;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 16px;
    border-bottom: 1px solid var(--saqr-stroke-soft);
  }

  .mark {
    width: 36px;
    height: 36px;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: var(--saqr-radius-md);
    background: var(--saqr-bg-selected);
    color: var(--saqr-action-primary);
  }

  .brand-copy {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .brand-name {
    font-size: 14px;
    font-weight: 600;
    letter-spacing: -0.01em;
    color: var(--saqr-text-strong);
    line-height: 1.2;
  }

  .brand-sub {
    font-size: 12px;
    color: var(--saqr-text-soft);
    line-height: 1.3;
  }

  .groups {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .group {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .group-label {
    padding: 4px 12px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--saqr-text-soft);
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  li button {
    position: relative;
    width: 100%;
    display: flex;
    align-items: center;
    gap: 10px;
    height: 36px;
    padding: 0 12px;
    border: none;
    border-radius: var(--saqr-radius-md);
    background: transparent;
    color: var(--saqr-navigation-content-default);
    font-size: 13px;
    font-weight: 500;
    text-align: left;
  }

  li button:hover {
    background: var(--saqr-navigation-item-hover);
    color: var(--saqr-text-strong);
  }

  li button.active {
    background: var(--saqr-navigation-item-active);
    color: var(--saqr-navigation-content-active);
    font-weight: 600;
  }

  li button.active::before {
    content: '';
    position: absolute;
    inset-inline-start: 0;
    top: 8px;
    bottom: 8px;
    width: 3px;
    border-radius: var(--saqr-radius-full);
    background: var(--saqr-brand-clay-500);
  }

  li button span:not(.badge) {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .badge {
    min-width: 18px;
    height: 18px;
    padding: 0 5px;
    border-radius: var(--saqr-radius-full);
    font-size: 11px;
    font-weight: 600;
    display: grid;
    place-items: center;
    background: var(--saqr-state-danger-surface);
    color: var(--saqr-state-danger-content);
    border: 1px solid var(--saqr-state-danger-border);
  }

  .foot {
    margin-top: auto;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px;
    border-top: 1px solid var(--saqr-stroke-soft);
  }

  .notice {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    border-radius: var(--saqr-radius-md);
    font-size: 12px;
    font-weight: 500;
    border: 1px solid transparent;
  }

  .notice span {
    flex: 1;
  }

  .notice.danger {
    color: var(--saqr-state-danger-content);
    background: var(--saqr-state-danger-surface);
    border-color: var(--saqr-state-danger-border);
  }

  .notice.warn {
    color: var(--saqr-state-warning-content);
    background: var(--saqr-state-warning-surface);
    border-color: var(--saqr-state-warning-border);
  }

  .profile {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 10px 12px;
    border-radius: var(--saqr-radius-lg);
    background: var(--saqr-bg-subtle);
    border: 1px solid var(--saqr-stroke-soft);
  }

  .profile .name {
    font-weight: 600;
    font-size: 13px;
    color: var(--saqr-text-strong);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .conn {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    padding: 0 4px;
    text-transform: capitalize;
    color: var(--saqr-text-sub);
  }

  .led {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--saqr-system-warning-500);
  }

  .led.ok {
    background: var(--saqr-system-success-500);
  }

  .led.bad {
    background: var(--saqr-system-danger-500);
  }
</style>
