import { pctToPwm } from './curve';
import type { CurvePoint } from './types';

export type FanRole = 'cpu' | 'system' | 'pump';

export interface Preset {
  id: string;
  label: string;
  /** [temperature °C, duty %] */
  curve: [number, number][];
}

/** "Standard" is GIGABYTE's own AM5 CPU curve (AmdAm5SmartFanCoordinateSystemProfileModulator); the rest are Helm's. */
const presets: Record<FanRole, Preset[]> = {
  cpu: [
    { id: 'silent', label: 'Silent', curve: [[0, 22], [45, 22], [58, 32], [67, 42], [75, 58], [82, 75], [87, 90], [91, 100]] },
    { id: 'standard', label: 'GIGABYTE standard', curve: [[0, 25], [20, 25], [30, 51], [40, 60], [50, 65], [60, 68], [65, 80], [70, 100]] },
    { id: 'balanced', label: 'Balanced', curve: [[0, 28], [40, 28], [52, 38], [62, 50], [71, 64], [78, 78], [84, 90], [88, 100]] },
    { id: 'performance', label: 'Performance', curve: [[0, 38], [35, 38], [45, 50], [55, 64], [64, 78], [72, 90], [78, 96], [82, 100]] },
  ],
  system: [
    { id: 'silent', label: 'Silent', curve: [[0, 20], [32, 20], [42, 30], [50, 42], [58, 58], [66, 76], [72, 90], [76, 100]] },
    { id: 'standard', label: 'GIGABYTE standard', curve: [[0, 25], [20, 25], [30, 44], [40, 62], [45, 72], [50, 81], [55, 91], [60, 100]] },
    { id: 'balanced', label: 'Balanced', curve: [[0, 28], [30, 28], [40, 42], [48, 56], [55, 70], [62, 84], [67, 94], [70, 100]] },
    { id: 'performance', label: 'Performance', curve: [[0, 40], [28, 40], [36, 55], [44, 70], [50, 82], [55, 92], [58, 97], [60, 100]] },
  ],
  pump: [
    { id: 'quiet', label: 'Quiet pump', curve: [[0, 60], [1, 60], [69, 60], [70, 100]] },
    { id: 'standard', label: 'Standard pump', curve: [[0, 80], [1, 80], [69, 80], [70, 100]] },
    { id: 'max', label: 'Max pump', curve: [[0, 100], [1, 100], [2, 100]] },
  ],
};

export function roleOf(title: string, temperatureSourceTitle: string | undefined): FanRole {
  if (/pump/i.test(title)) return 'pump';
  if (/cpu/i.test(title) || /cpu/i.test(temperatureSourceTitle ?? '')) return 'cpu';
  return 'system';
}

export function presetsFor(role: FanRole): Preset[] {
  return presets[role];
}

export function presetPoints(preset: Preset): CurvePoint[] {
  return preset.curve.map(([t, pct]) => [t, pctToPwm(pct)] as CurvePoint);
}

export const FULL_SPEED: CurvePoint[] = [[0, 255], [1, 255], [2, 255]];
