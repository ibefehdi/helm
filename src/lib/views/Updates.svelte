<script lang="ts">
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { app } from '$lib/app.svelte';
  import { api, errorMessage } from '$lib/api';
  import { timeAgo } from '$lib/format';
  import Icon from '$lib/components/Icon.svelte';
  import type { BiosInfo, DriverScan, FlashReadiness, InstallResult, UpdateProgress, UsbDrive } from '$lib/types';

  let scan = $state<DriverScan | null>(null);
  let scanning = $state(false);
  let scanPhase = $state('');
  let selected = $state<number[]>([]);
  let installing = $state(false);
  let progress = $state<UpdateProgress | null>(null);
  let results = $state<InstallResult[]>([]);
  let showCurrent = $state(false);

  let bios = $state<BiosInfo | null>(null);
  let biosBusy = $state<'check' | 'download' | 'copy' | 'flash' | null>(null);
  let biosProgress = $state<UpdateProgress | null>(null);
  let biosError = $state<string | null>(null);
  let usb = $state<UsbDrive[]>([]);
  let usbLetter = $state('');
  let copiedTo = $state<string | null>(null);
  let showFlash = $state(false);
  let readiness = $state<FlashReadiness | null>(null);
  let typedModel = $state('');
  let suspendBitlocker = $state(true);

  const outdated = $derived(scan?.items.filter((i) => i.outdated && i.size > 0) ?? []);
  const current = $derived(scan?.items.filter((i) => !i.outdated) ?? []);
  const selectedSize = $derived(outdated.filter((i) => selected.includes(i.id)).reduce((s, i) => s + i.size, 0));
  const flashBlocked = $derived(
    !readiness?.flasherFound ||
      readiness?.onAcPower === false ||
      (readiness?.bitlockerOn === true && !suspendBitlocker) ||
      typedModel.trim() !== (bios?.model ?? '').trim(),
  );

  const phaseText: Record<string, string> = {
    catalog: 'Downloading GIGABYTE’s driver catalog…',
    devices: 'Matching devices…',
    sizes: 'Checking downloads…',
  };

  onMount(() => {
    const unlisten = listen<UpdateProgress>('updates-progress', (e) => {
      const p = e.payload;
      if (p.id === 'bios') biosProgress = p;
      else if (p.id !== undefined) progress = p;
      else scanPhase = phaseText[p.phase] ?? '';
    });
    void checkBios();
    return () => void unlisten.then((f) => f());
  });

  function bytes(n: number): string {
    if (!n) return '—';
    if (n < 1e6) return `${Math.round(n / 1e3)} KB`;
    if (n < 1e9) return `${(n / 1e6).toFixed(n < 1e7 ? 1 : 0)} MB`;
    return `${(n / 1e9).toFixed(2)} GB`;
  }

  async function runScan() {
    scanning = true;
    results = [];
    try {
      scan = await api.updatesScan();
      selected = scan.items.filter((i) => i.outdated && i.size > 0).map((i) => i.id);
    } catch (e) {
      app.toast('error', errorMessage(e));
    } finally {
      scanning = false;
      scanPhase = '';
    }
  }

  async function install() {
    installing = true;
    results = [];
    try {
      results = await api.updatesInstall(selected);
      const ok = results.filter((r) => r.ok).length;
      const reboot = results.some((r) => r.reboot);
      app.toast(ok === results.length ? 'success' : 'warn', `${ok} of ${results.length} installed${reboot ? '. Restart to finish.' : ''}`);
      await runScan();
    } catch (e) {
      app.toast('error', errorMessage(e));
    } finally {
      installing = false;
      progress = null;
    }
  }

  function toggle(id: number) {
    selected = selected.includes(id) ? selected.filter((x) => x !== id) : [...selected, id];
  }

  async function checkBios() {
    biosBusy = 'check';
    biosError = null;
    try {
      bios = await api.biosCheck();
      usb = await api.biosUsbDrives();
      usbLetter = usb.find((d) => d.fat32)?.letter ?? usb[0]?.letter ?? '';
    } catch (e) {
      biosError = errorMessage(e);
    } finally {
      biosBusy = null;
    }
  }

  async function downloadBios() {
    biosBusy = 'download';
    biosProgress = null;
    try {
      const d = await api.biosDownload();
      if (bios) bios.downloaded = d.image;
      app.toast('success', `BIOS ${d.version} downloaded and verified`);
    } catch (e) {
      app.toast('error', errorMessage(e));
    } finally {
      biosBusy = null;
      biosProgress = null;
    }
  }

  async function refreshUsb() {
    usb = await api.biosUsbDrives();
    if (!usb.some((d) => d.letter === usbLetter)) usbLetter = usb.find((d) => d.fat32)?.letter ?? usb[0]?.letter ?? '';
  }

  async function copyToUsb() {
    biosBusy = 'copy';
    try {
      copiedTo = await api.biosCopyToUsb(usbLetter);
    } catch (e) {
      app.toast('error', errorMessage(e));
    } finally {
      biosBusy = null;
    }
  }

  async function openFlash() {
    showFlash = true;
    typedModel = '';
    readiness = await app.run(() => api.biosFlashReadiness()) ?? null;
  }

  async function flash() {
    biosBusy = 'flash';
    try {
      await api.biosFlash(typedModel, suspendBitlocker);
      showFlash = false;
      app.toast('info', 'GIGABYTE’s flasher has started. Don’t turn the PC off; it will restart by itself.', 30000);
    } catch (e) {
      app.toast('error', errorMessage(e));
    } finally {
      biosBusy = null;
    }
  }

  function pct(p: UpdateProgress | null) {
    return p?.total ? Math.round(((p.received ?? 0) / p.total) * 100) : 0;
  }
</script>

<header>
  <div>
    <h1>Updates</h1>
    <p class="muted">Drivers and BIOS from GIGABYTE’s own update catalog, matched to the hardware in this PC.</p>
  </div>
  <button class="btn ghost" onclick={() => app.run(() => api.openDownloads())}><Icon name="folder" size={14} />Downloads</button>
</header>

<!-- BIOS -->
<section class="card bios">
  <div class="bios-main">
    <div class="chipicon"><Icon name="chip" size={22} /></div>
    <div class="bios-text">
      <h2>BIOS</h2>
      {#if biosBusy === 'check' && !bios}
        <p class="muted">Checking…</p>
      {:else if biosError}
        <p class="bad">{biosError}</p>
      {:else if bios}
        <p class="board">{bios.model || 'Unknown board'} <span class="faint num">{bios.partNumber ?? ''}</span></p>
        <p class="versions">
          <span>Installed <b class="num">{bios.installed || '—'}</b>{bios.installedDate ? ` (${bios.installedDate})` : ''}</span>
          {#if bios.latest}
            <span>Latest <b class="num">{bios.latest}</b></span>
          {/if}
          {#if bios.updateAvailable}<span class="chip accent">Update available</span>
          {:else if bios.latest}<span class="chip ok">Up to date</span>{/if}
        </p>
        {#if bios.note}<p class="faint note">{bios.note}</p>{/if}
      {/if}
    </div>
    <div class="bios-actions">
      <button class="btn" disabled={biosBusy !== null} onclick={checkBios}><Icon name="refresh" size={14} />Check</button>
      {#if bios?.url && bios.size > 0 && !bios.downloaded}
        <button class={['btn', { primary: bios.updateAvailable }]} disabled={biosBusy !== null} onclick={downloadBios}>
          <Icon name="download" size={14} />{biosBusy === 'download' ? `${pct(biosProgress)}%` : `Download ${bios.latest} (${bytes(bios.size)})`}
        </button>
      {/if}
    </div>
  </div>

  {#if biosBusy === 'download'}
    <div class="progress"><span style:width={`${pct(biosProgress)}%`}></span></div>
    <p class="faint small">{biosProgress?.phase === 'verify' ? 'Verifying the package…' : 'Downloading…'}</p>
  {/if}

  {#if bios?.downloaded}
    <div class="handoff">
      <div class="verified"><Icon name="check" size={14} />Verified image: <span class="num faint">{bios.downloaded.split('\\').pop()}</span></div>
      <div class="paths">
        <div class="path recommended">
          <div class="path-head">
            <Icon name="usb" size={16} />
            <h3>Q-Flash from a USB stick</h3>
            <span class="chip ok">Recommended</span>
          </div>
          <p class="muted small">Copy the image to a FAT32 USB stick, restart, press <b>End</b> during startup to open Q-Flash, then pick the file. Flashing happens outside Windows, which is the safest way.</p>
          {#if usb.length}
            <div class="row">
              <select class="input" bind:value={usbLetter}>
                {#each usb as d (d.letter)}
                  <option value={d.letter}>{d.letter}: {d.label || 'USB drive'} · {d.fileSystem} · {bytes(d.free)} free</option>
                {/each}
              </select>
              <button class="btn" disabled={!usbLetter || biosBusy !== null || !usb.find((d) => d.letter === usbLetter)?.fat32} onclick={copyToUsb}>
                {biosBusy === 'copy' ? 'Copying…' : 'Copy to USB'}
              </button>
              <button class="btn ghost sm" aria-label="Rescan USB drives" onclick={refreshUsb}><Icon name="refresh" size={13} /></button>
            </div>
            {#if usbLetter && !usb.find((d) => d.letter === usbLetter)?.fat32}
              <p class="warn small">This drive isn't FAT32. Q-Flash can't read it.</p>
            {/if}
          {:else}
            <div class="row"><span class="faint small">No USB stick found.</span><button class="btn sm" onclick={refreshUsb}>Rescan</button></div>
          {/if}
          {#if copiedTo}<p class="ok small"><Icon name="check" size={13} /> Copied and checked: {copiedTo}</p>{/if}
        </div>

        <div class="path">
          <div class="path-head">
            <Icon name="bolt" size={16} />
            <h3>Flash from Windows</h3>
            <span class="chip warn">Advanced</span>
          </div>
          <p class="muted small">Hands the verified image to GIGABYTE’s own flasher, which restarts the PC into the update. A power cut during the flash can leave the board unbootable.</p>
          <div class="row"><button class="btn" disabled={biosBusy !== null} onclick={openFlash}>Prepare flash…</button></div>
        </div>
      </div>
    </div>
  {/if}
</section>

<!-- Drivers -->
<section class="drivers">
  <div class="drivers-head">
    <div>
      <h2>Drivers</h2>
      {#if scan}
        <p class="faint small">Catalog {scan.revision} · {scan.deviceCount} devices checked · {timeAgo(scan.scannedAt)}</p>
      {/if}
    </div>
    <div class="row">
      {#if outdated.length}
        <button class="btn primary" disabled={installing || selected.length === 0} onclick={install}>
          <Icon name="download" size={14} />{installing ? 'Installing…' : `Install ${selected.length} (${bytes(selectedSize)})`}
        </button>
      {/if}
      {#if installing}
        <button class="btn ghost" onclick={() => api.updatesCancel()}>Stop after current</button>
      {/if}
      <button class="btn" disabled={scanning || installing} onclick={runScan}><Icon name="refresh" size={14} />{scan ? 'Rescan' : 'Scan for updates'}</button>
    </div>
  </div>

  {#if scanning}
    <div class="card empty"><p class="muted">{scanPhase || 'Scanning…'}</p></div>
  {:else if !scan}
    <div class="card empty">
      <Icon name="update" size={24} />
      <p>Scan to compare installed drivers with GIGABYTE’s latest releases for this board.</p>
      <p class="faint small">Installers are only run after their digital signature checks out. Bundled third-party apps are never offered.</p>
    </div>
  {:else}
    {#if installing && progress}
      <div class="card installing">
        <span>{(progress.index ?? 0) + 1}/{progress.count} · {progress.name}</span>
        <span class="faint">
          {progress.phase === 'download' ? `Downloading ${pct(progress)}%` : progress.phase === 'verify' ? 'Checking signature…' : progress.phase === 'install' ? `Installing (signed by ${progress.signer})…` : progress.message}
        </span>
        <div class="progress"><span style:width={`${progress.phase === 'download' ? pct(progress) : 100}%`}></span></div>
      </div>
    {/if}

    {#if results.length}
      <div class="card results">
        {#each results as r (r.id)}
          <div class="result">
            <Icon name={r.ok ? 'check' : 'warn'} size={14} />
            <span>{r.name}</span>
            <span class={['small', r.ok ? 'ok' : 'warn']}>{r.message}</span>
          </div>
        {/each}
      </div>
    {/if}

    {#if outdated.length === 0}
      <div class="card allgood"><Icon name="check" size={18} /><span>All matched drivers are up to date.</span></div>
    {:else}
      <div class="card table">
        {#each outdated as d (d.id)}
          <label class="drow">
            <input type="checkbox" checked={selected.includes(d.id)} disabled={installing} onchange={() => toggle(d.id)} />
            <div class="dname">
              <span>{d.name}</span>
              <span class="faint small">{d.devices.join(', ') || 'Installed package'}</span>
            </div>
            <span class="chip">{d.category}</span>
            <span class="num ver"><span class="faint">{d.installed}</span> → <b>{d.latest}</b></span>
            <span class="num faint size">{bytes(d.size)}</span>
          </label>
        {/each}
      </div>
    {/if}

    {#if current.length}
      <button class="btn ghost sm more" onclick={() => (showCurrent = !showCurrent)}>
        {showCurrent ? 'Hide' : 'Show'} {current.length} up-to-date driver{current.length === 1 ? '' : 's'}
      </button>
      {#if showCurrent}
        <div class="card table">
          {#each current as d (d.id)}
            <div class="drow static">
              <Icon name="check" size={14} />
              <div class="dname">
                <span>{d.name}</span>
                <span class="faint small">{d.devices.join(', ') || 'Installed package'}</span>
              </div>
              <span class="chip">{d.category}</span>
              <span class="num ver">{d.installed}</span>
              <span class="num faint size">{d.latest !== d.installed ? `catalog ${d.latest}` : ''}</span>
            </div>
          {/each}
        </div>
      {/if}
    {/if}
  {/if}
</section>

{#if showFlash && bios}
  <div class="scrim" role="presentation">
    <div class="card dialog" role="dialog" aria-modal="true" aria-labelledby="flash-title">
      <h3 id="flash-title">Flash BIOS {bios.latest} from Windows</h3>
      <ul class="checks">
        <li class={readiness?.flasherFound ? 'ok' : 'bad'}>
          <Icon name={readiness?.flasherFound ? 'check' : 'x'} size={14} />GIGABYTE flasher {readiness?.flasherFound ? 'found' : 'not installed (use the USB method)'}
        </li>
        <li class={readiness?.onAcPower === false ? 'bad' : 'ok'}>
          <Icon name={readiness?.onAcPower === false ? 'x' : 'check'} size={14} />{readiness?.onAcPower === false ? 'Running on battery. Connect AC power.' : 'On AC power'}
        </li>
        <li class={readiness?.bitlockerOn ? 'warnc' : 'ok'}>
          <Icon name={readiness?.bitlockerOn ? 'warn' : 'check'} size={14} />
          {#if readiness?.bitlockerOn}
            <label class="inline"><input type="checkbox" bind:checked={suspendBitlocker} />BitLocker is on. Suspend it for one restart, so Windows doesn't ask for the recovery key</label>
          {:else}
            BitLocker {readiness?.bitlockerOn === false ? 'off' : 'not detected'}
          {/if}
        </li>
        <li class="ok"><Icon name="check" size={14} />Image verified: {readiness?.image?.split('\\').pop()}</li>
      </ul>
      <p class="muted small">Save your work first. The PC restarts on its own; don't power it off until it's back in Windows.</p>
      <label class="confirm">
        <span class="small">Type <b class="num">{bios.model}</b> to confirm</span>
        <input class="input" bind:value={typedModel} placeholder={bios.model} />
      </label>
      <div class="dialog-actions">
        <button class="btn" onclick={() => (showFlash = false)}>Cancel</button>
        <button class="btn primary danger-fill" disabled={flashBlocked || biosBusy !== null} onclick={flash}>Flash and restart</button>
      </div>
    </div>
  </div>
{/if}

<style>
  header {
    display: flex;
    justify-content: space-between;
    align-items: flex-end;
    gap: 16px;
    margin-bottom: 20px;
  }

  .small {
    font-size: 12px;
  }

  .ok {
    color: var(--ok);
  }

  .warn {
    color: var(--warn);
  }

  .bad {
    color: var(--danger);
  }

  .row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .bios {
    padding: 18px 20px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    margin-bottom: 26px;
  }

  .bios-main {
    display: flex;
    align-items: center;
    gap: 16px;
  }

  .chipicon {
    width: 46px;
    height: 46px;
    border-radius: 12px;
    display: grid;
    place-items: center;
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 10%, transparent);
  }

  .bios-text {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .board {
    font-size: 15px;
    font-weight: 600;
  }

  .versions {
    display: flex;
    align-items: center;
    gap: 16px;
    font-size: 13px;
    color: var(--muted);
  }

  .versions b {
    color: var(--text);
  }

  .note {
    font-size: 12px;
  }

  .bios-actions {
    display: flex;
    gap: 8px;
  }

  .progress {
    height: 5px;
    border-radius: 3px;
    background: var(--line);
    overflow: hidden;
  }

  .progress span {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 0.2s;
  }

  .handoff {
    border-top: 1px solid var(--line);
    padding-top: 14px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .verified {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--ok);
    font-size: 13px;
  }

  .paths {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
  }

  .path {
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .path.recommended {
    border-color: color-mix(in srgb, var(--ok) 30%, transparent);
  }

  .path-head {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .path-head h3 {
    flex: 1;
  }

  .path select {
    flex: 1;
    min-width: 0;
  }

  .drivers {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .drivers-head {
    display: flex;
    justify-content: space-between;
    align-items: flex-end;
  }

  .empty {
    padding: 32px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    text-align: center;
    color: var(--muted);
  }

  .installing {
    padding: 12px 16px;
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 6px 12px;
    font-size: 13px;
  }

  .installing .progress {
    grid-column: 1 / -1;
  }

  .results {
    padding: 8px 16px;
  }

  .result {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 0;
    font-size: 13px;
  }

  .result span:nth-child(2) {
    flex: 1;
  }

  .allgood {
    padding: 16px;
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--ok);
  }

  .table {
    overflow: hidden;
  }

  .drow {
    display: grid;
    grid-template-columns: 20px minmax(200px, 1fr) auto minmax(200px, auto) 70px;
    align-items: center;
    gap: 14px;
    padding: 10px 16px;
    border-top: 1px solid var(--line);
    cursor: pointer;
  }

  .drow:first-child {
    border-top: none;
  }

  .drow:hover {
    background: var(--hover);
  }

  .drow.static {
    cursor: default;
    color: var(--ok);
  }

  .drow.static .dname span:first-child,
  .drow.static .ver {
    color: var(--text);
  }

  .dname {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .dname span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .ver {
    font-size: 12.5px;
    text-align: right;
  }

  .size {
    font-size: 12px;
    text-align: right;
  }

  .more {
    align-self: flex-start;
  }

  .scrim {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    background: var(--saqr-utility-overlay);
    z-index: 50;
  }

  .dialog {
    width: 480px;
    padding: 22px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .checks {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-size: 13px;
  }

  .checks li {
    display: flex;
    align-items: flex-start;
    gap: 8px;
  }

  .checks li.ok {
    color: var(--ok);
  }

  .checks li.bad {
    color: var(--danger);
  }

  .checks li.warnc {
    color: var(--warn);
  }

  .inline {
    display: flex;
    gap: 8px;
    align-items: flex-start;
  }

  .confirm {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .dialog-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  .danger-fill:not(:disabled) {
    background: var(--saqr-action-danger);
    border-color: var(--saqr-action-danger);
    color: var(--saqr-action-on-danger);
  }
</style>
