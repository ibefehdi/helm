<script lang="ts">
  import { onMount } from 'svelte';
  import { theme, type ThemeName } from '$lib/theme.svelte';

  let { compact = false }: { compact?: boolean } = $props();
  let ready = $state(false);

  onMount(() => {
    theme.sync();
    ready = true;
  });

  function choose(next: ThemeName) {
    theme.set(next);
    ready = true;
  }
</script>

<div class={['switch', { compact }]} role="group" aria-label="Theme">
  <button
    type="button"
    class={{ on: ready && theme.current === 'light' }}
    aria-pressed={ready ? theme.current === 'light' : undefined}
    onclick={() => choose('light')}
  >
    Light
  </button>
  <button
    type="button"
    class={{ on: ready && theme.current === 'dark' }}
    aria-pressed={ready ? theme.current === 'dark' : undefined}
    onclick={() => choose('dark')}
  >
    Dark
  </button>
</div>

<style>
  .switch {
    display: inline-flex;
    padding: 2px;
    border-radius: var(--saqr-radius-md);
    background: var(--saqr-bg-subtle);
    border: 1px solid var(--saqr-stroke-default);
  }

  .compact {
    display: flex;
    width: 100%;
  }

  button {
    height: 28px;
    padding: 0 12px;
    border: 0;
    border-radius: var(--saqr-radius-sm);
    background: transparent;
    color: var(--saqr-text-sub);
    font-size: 12px;
    font-weight: 500;
  }

  .compact button {
    flex: 1;
  }

  button.on {
    background: var(--saqr-bg-surface);
    color: var(--saqr-text-strong);
    box-shadow: 0 1px 2px var(--saqr-shadow-key);
  }
</style>
