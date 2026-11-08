<script lang="ts">
  let {
    checked,
    onchange,
    label,
    disabled = false,
  }: { checked: boolean; onchange: (value: boolean) => void; label: string; disabled?: boolean } = $props();
</script>

<button
  class={['toggle', { on: checked }]}
  role="switch"
  aria-checked={checked}
  aria-label={label}
  {disabled}
  onclick={() => onchange(!checked)}
>
  <span class="knob"></span>
</button>

<style>
  .toggle {
    position: relative;
    width: 40px;
    height: 24px;
    flex: none;
    border-radius: var(--saqr-radius-full);
    border: 1px solid var(--saqr-stroke-default);
    background: var(--saqr-bg-subtle);
    padding: 0;
    transition:
      background-color 150ms ease-out,
      border-color 150ms ease-out;
  }

  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: var(--saqr-bg-surface);
    border: 1px solid var(--saqr-stroke-default);
    box-shadow: 0 1px 2px var(--saqr-shadow-key);
    transition: transform 150ms ease-out;
  }

  .on {
    background: var(--saqr-action-primary);
    border-color: var(--saqr-action-primary);
  }

  .on .knob {
    transform: translateX(16px);
    border-color: transparent;
  }
</style>
