<script lang="ts">
  import { app } from '$lib/app.svelte';
  import { fmt } from '$lib/format';
  import Icon from '$lib/components/Icon.svelte';
  import Toggle from '$lib/components/Toggle.svelte';
  import type { Alert } from '$lib/types';

  const alerts = $derived(app.settings?.alerts ?? []);
  const sensors = $derived(app.sample?.readings ?? []);

  function unitOf(key: string) {
    const r = app.readings[key];
    if (r) return r.unit === 'RPM' ? 'rpm' : r.unit;
    return key.startsWith('Temperature/') ? '°C' : key.startsWith('Fan/') ? 'rpm' : key.startsWith('Voltage/') ? 'V' : '';
  }

  function save(next: Alert[]) {
    void app.saveSettings({ alerts: next });
  }

  function update(id: string, patch: Partial<Alert>) {
    save(alerts.map((a) => (a.id === id ? { ...a, ...patch } : a)));
  }

  function add() {
    const cpu = sensors.find((r) => r.key === 'Temperature/CPU') ?? sensors[0];
    save([
      ...alerts,
      {
        id: crypto.randomUUID(),
        sensor: cpu?.key ?? 'Temperature/CPU',
        op: 'above',
        threshold: cpu?.kind === 'Fan' ? 300 : 80,
        hysteresis: 3,
        action: 'notify',
        enabled: true,
      },
    ]);
  }

  function remove(id: string) {
    save(alerts.filter((a) => a.id !== id));
  }
</script>

<header>
  <div>
    <h1>Alerts</h1>
    <p class="muted">
      Watch any sensor. “Full speed” is a failsafe: every header goes to 100% until the reading recovers past the hysteresis band — even
      with the window closed.
    </p>
  </div>
  <button class="btn primary" onclick={add}><Icon name="plus" size={14} />New alert</button>
</header>

{#if alerts.length === 0}
  <div class="card empty muted">No alerts configured.</div>
{:else}
  <div class="list">
    {#each alerts as a (a.id)}
      {@const active = !!app.activeAlerts[a.id]}
      {@const value = app.readings[a.sensor]?.value ?? null}
      <div class={['card alert', { active, off: !a.enabled }]}>
        <Toggle checked={a.enabled} label="Enabled" onchange={(v) => update(a.id, { enabled: v })} />

        <div class="rule">
          <span class="muted">When</span>
          <select class="input" value={a.sensor} onchange={(e) => update(a.id, { sensor: e.currentTarget.value })}>
            {#if !sensors.some((s) => s.key === a.sensor)}
              <option value={a.sensor}>{a.sensor.replace('/', ' · ')}</option>
            {/if}
            {#each ['Temperature', 'Fan', 'Voltage'] as kind (kind)}
              <optgroup label={kind}>
                {#each sensors.filter((s) => s.kind === kind) as s (s.key)}
                  <option value={s.key}>{s.title}</option>
                {/each}
              </optgroup>
            {/each}
          </select>
          <span class="muted">goes</span>
          <select class="input" value={a.op} onchange={(e) => update(a.id, { op: e.currentTarget.value as Alert['op'] })}>
            <option value="above">above</option>
            <option value="below">below</option>
          </select>
          <input
            class="input"
            type="number"
            step={a.sensor.startsWith('Voltage/') ? 0.01 : 1}
            value={a.threshold}
            onchange={(e) => update(a.id, { threshold: Number(e.currentTarget.value) })}
          />
          <span class="muted unit">{unitOf(a.sensor)}</span>
          <span class="muted">→</span>
          <select class="input" value={a.action} onchange={(e) => update(a.id, { action: e.currentTarget.value as Alert['action'] })}>
            <option value="notify">Notify</option>
            <option value="fullSpeed">Notify + all fans 100%</option>
          </select>
          <span class="muted">recover</span>
          <input
            class="input small"
            type="number"
            min="0"
            step={a.sensor.startsWith('Voltage/') ? 0.01 : 1}
            value={a.hysteresis}
            title="How far back past the threshold the reading must go before the alert clears"
            onchange={(e) => update(a.id, { hysteresis: Math.max(0, Number(e.currentTarget.value)) })}
          />
        </div>

        <div class="state">
          <span class="num now">{fmt(value, unitOf(a.sensor))} <small>{unitOf(a.sensor)}</small></span>
          {#if active}
            <span class="chip danger">Triggered</span>
          {:else if a.enabled}
            <span class="chip ok">OK</span>
          {:else}
            <span class="chip">Off</span>
          {/if}
        </div>

        <button class="btn ghost sm" aria-label="Delete alert" onclick={() => remove(a.id)}><Icon name="trash" size={14} /></button>
      </div>
    {/each}
  </div>
{/if}

<style>
  header {
    display: flex;
    justify-content: space-between;
    align-items: flex-end;
    gap: 24px;
    margin-bottom: 20px;
  }

  header p {
    max-width: 640px;
  }

  .empty {
    padding: 32px;
    text-align: center;
  }

  .list {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .alert {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 12px 14px;
  }

  .alert.active {
    border-color: color-mix(in srgb, var(--danger) 50%, transparent);
    background: color-mix(in srgb, var(--danger) 5%, var(--surface));
  }

  .alert.off .rule {
    opacity: 0.55;
  }

  .rule {
    flex: 1;
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
    font-size: 13px;
  }

  .rule select.input {
    max-width: 220px;
  }

  .input.small {
    width: 64px;
  }

  .unit {
    min-width: 22px;
  }

  .state {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 4px;
    min-width: 92px;
  }

  .now {
    font-size: 16px;
    font-weight: 600;
  }

  .now small {
    font-size: 11px;
    color: var(--muted);
  }
</style>
