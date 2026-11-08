<script lang="ts">
  import { app } from '$lib/app.svelte';
  import { api } from '$lib/api';
  import Icon from './Icon.svelte';

  let busy = $state(false);

  const hints: Record<string, string> = {
    not_admin: 'The GIGABYTE engine only accepts connections from administrators.',
    service_unavailable:
      'The “EasyTune Engine Service” is not installed or not running. Install GIGABYTE Control Center (Smart Fan / EasyTune module) or start the service from services.msc.',
    engine_unreachable: 'The engine service is running but did not answer. It may still be starting — try again in a few seconds.',
    access_denied: 'The engine refused the connection. Make sure Helm is running as administrator.',
    fans_unsupported: 'This board does not expose Smart Fan control through the GIGABYTE engine.',
    bridge_exited: 'The Helm bridge process stopped unexpectedly.',
  };

  async function retry() {
    busy = true;
    await app.run(() => api.reconnect());
    busy = false;
  }
</script>

<div class="gate">
  <div class="panel card">
    {#if !app.status.elevated}
      <Icon name="shield" size={28} />
      <h1>Administrator rights needed</h1>
      <p class="muted">{hints.not_admin} Helm will ask once through Windows UAC.</p>
      <button class="btn primary" onclick={() => app.run(() => api.relaunchAsAdmin())}>Relaunch as administrator</button>
    {:else if app.status.phase === 'error'}
      <Icon name="warn" size={28} />
      <h1>Can't reach the fan controller</h1>
      <p class="muted">{(app.status.code && hints[app.status.code]) || app.status.message}</p>
      {#if app.status.message && app.status.code && hints[app.status.code]}
        <code>{app.status.message}</code>
      {/if}
      <button class="btn primary" disabled={busy} onclick={retry}>
        <Icon name="refresh" size={14} />
        {busy ? 'Connecting…' : 'Try again'}
      </button>
    {:else}
      <div class="spinner" aria-hidden="true"></div>
      <h1>Connecting to the engine</h1>
      <p class="muted">Starting the hardware bridge and talking to the EasyTune Engine service…</p>
    {/if}
  </div>
</div>

<style>
  .gate {
    flex: 1;
    display: grid;
    place-items: center;
    padding: 32px;
  }

  .panel {
    max-width: 440px;
    padding: 32px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 14px;
    text-align: center;
    color: var(--accent);
  }

  .panel h1 {
    color: var(--text);
  }

  code {
    font-family: var(--mono);
    font-size: 11.5px;
    color: var(--faint);
    user-select: text;
    word-break: break-word;
  }

  .spinner {
    width: 28px;
    height: 28px;
    border-radius: 50%;
    border: 2.5px solid var(--line-strong);
    border-top-color: var(--accent);
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
