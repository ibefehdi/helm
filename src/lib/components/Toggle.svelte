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
    width: 36px;
    height: 20px;
    flex: none;
    border-radius: 999px;
    border: 1px solid var(--line-strong);
    background: var(--raised);
    padding: 0;
    transition: background 0.15s;
  }

  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--muted);
    transition:
      transform 0.15s,
      background 0.15s;
  }

  .on {
    background: color-mix(in srgb, var(--accent) 25%, var(--raised));
    border-color: color-mix(in srgb, var(--accent) 60%, transparent);
  }

  .on .knob {
    transform: translateX(16px);
    background: var(--accent);
  }
</style>
