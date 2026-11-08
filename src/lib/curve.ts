import type { CalibrationPoint, CurvePoint } from './types';

/** Firmware constraints recovered from Gigabyte.EnvironmentControl.Common (SmartGuardianFanConfigConverter). */
export const CURVE_MIN_POINTS = 3;
export const CURVE_MAX_POINTS = 8;
export const CURVE_MAX_TEMP = 100;
export const PWM_MAX = 255;

export const pwmToPct = (pwm: number) => (pwm / PWM_MAX) * 100;
export const pctToPwm = (pct: number) => Math.round((pct / 100) * PWM_MAX);

export const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** Duty (PWM) the curve asks for at a temperature, interpolating linearly between points. */
export function pwmAt(points: CurvePoint[], temperature: number): number {
  if (points.length === 0) return 0;
  // Point 0's duty is ignored by the firmware; the fan holds the start duty (point 1) until the first slope.
  const pts = points.length > 1 ? [[points[0][0], points[1][1]] as CurvePoint, ...points.slice(1)] : points;
  if (temperature <= pts[0][0]) return pts[0][1];
  for (let i = 1; i < pts.length; i++) {
    const [x1, y1] = pts[i];
    if (temperature <= x1) {
      const [x0, y0] = pts[i - 1];
      return x1 === x0 ? y1 : y0 + ((temperature - x0) / (x1 - x0)) * (y1 - y0);
    }
  }
  return pts[pts.length - 1][1];
}

/** Estimated RPM at a duty %, from a calibration sweep. */
export function rpmAt(calibration: CalibrationPoint[] | undefined, pct: number): number | null {
  if (!calibration || calibration.length < 2) return null;
  const pts = [...calibration].sort((a, b) => a[0] - b[0]);
  if (pct <= pts[0][0]) return pts[0][1];
  for (let i = 1; i < pts.length; i++) {
    if (pct <= pts[i][0]) {
      const [x0, y0] = pts[i - 1];
      const [x1, y1] = pts[i];
      return y0 + ((pct - x0) / (x1 - x0)) * (y1 - y0);
    }
  }
  return pts[pts.length - 1][1];
}

/** Highest calibrated duty % at which the fan was stopped (or null if it never stopped). */
export function stallPct(calibration: CalibrationPoint[] | undefined): number | null {
  if (!calibration) return null;
  const stopped = calibration.filter(([, rpm]) => rpm < 100).map(([pct]) => pct);
  return stopped.length ? Math.max(...stopped) : null;
}

export interface CurveIssue {
  level: 'error' | 'warn';
  message: string;
}

export function validate(points: CurvePoint[], stall: number | null): CurveIssue[] {
  const issues: CurveIssue[] = [];
  if (points.length < CURVE_MIN_POINTS) issues.push({ level: 'error', message: `At least ${CURVE_MIN_POINTS} points are required.` });
  if (points.length > CURVE_MAX_POINTS) issues.push({ level: 'error', message: `The controller stores at most ${CURVE_MAX_POINTS} points.` });
  for (let i = 1; i < points.length; i++) {
    if (points[i][0] <= points[i - 1][0]) {
      issues.push({ level: 'error', message: 'Temperatures must be strictly ascending.' });
      break;
    }
  }
  const last = points[points.length - 1];
  if (last && last[1] !== PWM_MAX) issues.push({ level: 'error', message: 'The last point must be 100% — the controller ignores curves that never reach full speed.' });
  for (let i = 2; i < points.length; i++) {
    if (points[i][1] < points[i - 1][1]) {
      issues.push({ level: 'warn', message: 'Duty drops as temperature rises somewhere on this curve.' });
      break;
    }
  }
  if (stall !== null && points.length > 1 && pwmToPct(points[1][1]) <= stall) {
    issues.push({ level: 'warn', message: `Start duty is at or below this fan's measured stall point (${stall}%). It may stop at idle.` });
  }
  return issues;
}

/** Enforces the controller's invariants on an edited curve. */
export function normalize(points: CurvePoint[]): CurvePoint[] {
  const pts = points.map(([x, y]) => [Math.round(clamp(x, 0, CURVE_MAX_TEMP)), Math.round(clamp(y, 0, PWM_MAX))] as CurvePoint);
  pts.sort((a, b) => a[0] - b[0]);
  for (let i = 1; i < pts.length; i++) {
    if (pts[i][0] <= pts[i - 1][0]) pts[i][0] = pts[i - 1][0] + 1;
  }
  if (pts.length > 1) pts[0][1] = pts[1][1];
  return pts;
}

/** A flat duty with a safety ramp to 100% near the top of the range. */
export function fixedCurve(pct: number, safetyTemp = 90): CurvePoint[] {
  const pwm = pctToPwm(pct);
  if (pwm >= PWM_MAX) return [[0, PWM_MAX], [1, PWM_MAX], [2, PWM_MAX]];
  return [[0, pwm], [1, pwm], [safetyTemp - 1, pwm], [safetyTemp, PWM_MAX]];
}

export function isFixed(points: CurvePoint[]): number | null {
  if (points.length < 3) return null;
  const body = points.slice(1, -1);
  const duty = body[0][1];
  return body.every(([, y]) => y === duty) ? Math.round(pwmToPct(duty)) : null;
}
