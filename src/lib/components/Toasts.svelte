<script lang="ts">
  import { fly } from 'svelte/transition';
  import { app } from '$lib/app.svelte';
  import Icon from './Icon.svelte';

  const icons = { info: 'gauge', success: 'check', warn: 'warn', error: 'warn' } as const;
</script>

<div class="stack" aria-live="polite">
  {#each app.toasts as toast (toast.id)}
    <div class="toast {toast.kind}" transition:fly={{ y: 12, duration: 160 }}>
      <Icon name={icons[toast.kind]} />
      <p>{toast.message}</p>
      <button class="btn ghost sm" aria-label="Dismiss" onclick={() => app.dismiss(toast.id)}>
        <Icon name="x" size={14} />
      </button>
    </div>
  {/each}
</div>

<style>
  .stack {
    position: fixed;
    right: 20px;
    bottom: 20px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    z-index: 50;
    max-width: 420px;
  }

  .toast {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 12px 8px 12px 14px;
    border-radius: var(--saqr-radius-lg);
    background: var(--saqr-bg-surface);
    border: 1px solid var(--saqr-stroke-soft);
    box-shadow:
      0 0 0 1px var(--saqr-stroke-soft),
      0 12px 28px var(--saqr-shadow-overlay);
    font-size: 13px;
    color: var(--saqr-text-strong);
  }

  .toast p {
    flex: 1;
    padding-top: 1px;
    user-select: text;
  }

  .success {
    color: var(--ok);
  }

  .warn {
    color: var(--warn);
  }

  .error {
    color: var(--danger);
  }

  .toast p {
    color: var(--text);
  }
</style>
