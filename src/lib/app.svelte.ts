import { listen } from '@tauri-apps/api/event';
import { api, errorMessage } from './api';
import type {
  AlertEvent,
  CalibrationProgress,
  FanCurve,
  Info,
  Overrides,
  Profile,
  Reading,
  Sample,
  Settings,
  Status,
} from './types';

export type View =
  | 'overview'
  | 'fans'
  | 'sensors'
  | 'lighting'
  | 'overclock'
  | 'graphics'
  | 'devices'
  | 'storage'
  | 'updates'
  | 'profiles'
  | 'alerts'
  | 'settings';

export interface Toast {
  id: number;
  kind: 'info' | 'success' | 'warn' | 'error';
  message: string;
}

const HISTORY_FALLBACK_MINUTES = 15;

class AppStore {
  view = $state<View>('overview');
  selectedFan = $state(0);
  status = $state<Status>({
    phase: 'starting',
    code: null,
    message: null,
    elevated: true,
    serviceState: null,
    bridgeVersion: null,
  });
  info = $state<Info | null>(null);
  sample = $state<Sample | null>(null);
  fans = $state<FanCurve[]>([]);
  settings = $state<Settings | null>(null);
  profiles = $state<Profile[]>([]);
  overrides = $state<Overrides>({ failsafe: false, manual: {}, calibrating: null });
  calibration = $state<CalibrationProgress | null>(null);
  activeAlerts = $state<Record<string, AlertEvent>>({});
  toasts = $state<Toast[]>([]);
  /** Bumped whenever the history buffer changes; read it to subscribe to `series()`. */
  historyTick = $state(0);

  /** Rolling per-sensor history (key -> [t ms, value][]), kept out of deep reactivity. */
  #history: Record<string, [number, number][]> = {};

  #toastId = 0;
  #started = false;

  readings = $derived<Record<string, Reading>>(
    Object.fromEntries((this.sample?.readings ?? []).map((r) => [r.key, r])),
  );

  ready = $derived(this.status.phase === 'ready' && this.info !== null);

  activeProfile = $derived(this.profiles.find((p) => p.id === this.settings?.activeProfile) ?? null);

  openFan(index: number) {
    this.selectedFan = index;
    this.view = 'fans';
  }

  async applyProfile(id: string) {
    const name = this.profiles.find((p) => p.id === id)?.name ?? 'profile';
    const fans = await this.run(() => api.applyProfile(id), `Applied “${name}”`);
    if (fans) this.fans = fans;
  }

  fanTitle(index: number): string {
    return this.settings?.fanLabels[String(index)] || this.info?.fans[index]?.title || `Fan ${index + 1}`;
  }

  sourceTitle(index: number): string {
    return this.info?.temperatureSources.find((s) => s.index === index)?.title ?? `Input ${index}`;
  }

  /** Current temperature driving a header, as reported by the controller. */
  headerTemperature(index: number): number | null {
    return this.sample?.headerTemperatures[index] ?? null;
  }

  headerRpm(index: number): number | null {
    return this.sample?.headerRpms[index] ?? null;
  }

  toast(kind: Toast['kind'], message: string, ttl = kind === 'error' ? 8000 : 4000) {
    const id = ++this.#toastId;
    this.toasts.push({ id, kind, message });
    setTimeout(() => this.dismiss(id), ttl);
  }

  dismiss(id: number) {
    this.toasts = this.toasts.filter((t) => t.id !== id);
  }

  /** Runs an action, reporting failures as toasts. Returns undefined on failure. */
  async run<T>(action: () => Promise<T>, success?: string): Promise<T | undefined> {
    try {
      const result = await action();
      if (success) this.toast('success', success);
      return result;
    } catch (e) {
      this.toast('error', errorMessage(e));
      return undefined;
    }
  }

  async start() {
    if (this.#started) return;
    this.#started = true;

    await Promise.all([
      listen<Status>('status', (e) => {
        const wasReady = this.status.phase === 'ready';
        this.status = e.payload;
        if (!wasReady && e.payload.phase === 'ready') void this.loadConnected();
      }),
      listen<Info>('info', (e) => (this.info = e.payload)),
      listen<Sample>('sample', (e) => this.onSample(e.payload)),
      listen<Settings>('settings', (e) => (this.settings = e.payload)),
      listen<Profile[]>('profiles', (e) => (this.profiles = e.payload)),
      listen<FanCurve[]>('fans', (e) => (this.fans = e.payload)),
      listen<Overrides>('overrides', (e) => {
        this.overrides = e.payload;
        if (e.payload.calibrating === null) this.calibration = null;
      }),
      listen<CalibrationProgress>('calibration', (e) => (this.calibration = e.payload)),
      listen<AlertEvent>('alert', (e) => {
        const next = { ...this.activeAlerts };
        if (e.payload.active) next[e.payload.id] = e.payload;
        else delete next[e.payload.id];
        this.activeAlerts = next;
      }),
      listen<{ kind: Toast['kind']; message: string }>('toast', (e) => this.toast(e.payload.kind, e.payload.message)),
    ]);

    const [status, settings, profiles, overrides] = await Promise.all([
      api.status(),
      api.settings(),
      api.profiles(),
      api.overrides(),
    ]);
    this.status = status;
    this.settings = settings;
    this.profiles = profiles;
    this.overrides = overrides;

    this.#history = await api.history(undefined, 900).catch(() => ({}));
    this.historyTick++;
    const latest = await api.latest().catch(() => null);
    if (latest) this.sample = latest;

    if (status.phase === 'ready') await this.loadConnected();
  }

  async loadConnected() {
    try {
      const [info, fans] = await Promise.all([api.info(), api.fans()]);
      this.info = info;
      this.fans = fans;
    } catch (e) {
      this.toast('error', errorMessage(e));
    }
  }

  async reloadFans() {
    const fans = await this.run(() => api.fans());
    if (fans) this.fans = fans;
  }

  async saveSettings(patch: Partial<Settings>) {
    if (!this.settings) return;
    const next = await this.run(() => api.updateSettings({ ...$state.snapshot(this.settings!), ...patch }));
    if (next) this.settings = next;
  }

  private onSample(sample: Sample) {
    this.sample = sample;
    const horizon = (this.settings?.historyMinutes ?? HISTORY_FALLBACK_MINUTES) * 60_000;
    const cutoff = sample.t - horizon;
    for (const r of sample.readings) {
      if (r.value === null) continue;
      const series = (this.#history[r.key] ??= []);
      series.push([sample.t, r.value]);
      let drop = 0;
      while (drop < series.length && series[drop][0] < cutoff) drop++;
      if (drop) series.splice(0, drop);
    }
    this.historyTick++;
  }

  /** History for one sensor; reactive through `historyTick`. */
  series(key: string): [number, number][] {
    void this.historyTick;
    return this.#history[key]?.slice() ?? [];
  }

  clearHistory() {
    this.#history = {};
    this.historyTick++;
  }
}

export const app = new AppStore();
