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
    padding: 10px 8px 10px 12px;
    border-radius: var(--radius);
    background: var(--raised);
    border: 1px solid var(--line-strong);
    box-shadow: 0 12px 32px rgb(0 0 0 / 0.45);
    font-size: 13px;
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
