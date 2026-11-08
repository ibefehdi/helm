<script lang="ts">
  import { onMount } from 'svelte';
  import { app } from '$lib/app.svelte';
  import Sidebar from '$lib/components/Sidebar.svelte';
  import Gate from '$lib/components/Gate.svelte';
  import Toasts from '$lib/components/Toasts.svelte';
  import Overview from '$lib/views/Overview.svelte';
  import Fans from '$lib/views/Fans.svelte';
  import Sensors from '$lib/views/Sensors.svelte';
  import Profiles from '$lib/views/Profiles.svelte';
  import Alerts from '$lib/views/Alerts.svelte';
  import Settings from '$lib/views/Settings.svelte';
  import Lighting from '$lib/views/Lighting.svelte';
  import Overclock from '$lib/views/Overclock.svelte';
  import Storage from '$lib/views/Storage.svelte';
  import Updates from '$lib/views/Updates.svelte';
  import Graphics from '$lib/views/Graphics.svelte';
  import Devices from '$lib/views/Devices.svelte';

  /** Views that are useful without a live engine connection. */
  const offline = new Set(['profiles', 'alerts', 'settings', 'storage', 'updates', 'graphics', 'devices']);

  onMount(() => {
    void app.start();
  });
</script>

<div class="shell">
  <Sidebar />
  <main>
    {#if app.ready || offline.has(app.view)}
      {#key app.view}
        <div class="view">
          {#if app.view === 'overview'}
            <Overview />
          {:else if app.view === 'fans'}
            <Fans />
          {:else if app.view === 'sensors'}
            <Sensors />
          {:else if app.view === 'lighting'}
            <Lighting />
          {:else if app.view === 'overclock'}
            <Overclock />
          {:else if app.view === 'graphics'}
            <Graphics />
          {:else if app.view === 'devices'}
            <Devices />
          {:else if app.view === 'storage'}
            <Storage />
          {:else if app.view === 'updates'}
            <Updates />
          {:else if app.view === 'profiles'}
            <Profiles />
          {:else if app.view === 'alerts'}
            <Alerts />
          {:else}
            <Settings />
          {/if}
        </div>
      {/key}
    {:else}
      <Gate />
    {/if}
  </main>
</div>
<Toasts />

<style>
  .shell {
    display: flex;
    height: 100vh;
  }

  main {
    flex: 1;
    min-width: 0;
    display: flex;
    overflow: hidden;
  }

  .view {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
    padding: 24px 28px 40px;
  }
</style>
