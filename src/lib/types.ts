export type Phase = 'starting' | 'connecting' | 'ready' | 'error';

export interface Status {
  phase: Phase;
  code: string | null;
  message: string | null;
  elevated: boolean;
  serviceState: string | null;
  bridgeVersion: string | null;
}

export interface TemperatureSource {
  index: number;
  title: string;
}

export interface FanHeader {
  index: number;
  title: string;
  temperatureSource: number;
}

export interface Info {
  groupNumber: string;
  platformNumber: number;
  fanCount: number;
  tachometerCount: number;
  temperatureInputCount: number;
  voltageInputCount: number;
  maxCurvePoints: number;
  temperatureSources: TemperatureSource[];
  fans: FanHeader[];
}

/** [temperature °C, PWM 0-255] */
export type CurvePoint = [number, number];

export interface FanCurve {
  index: number;
  mode: string;
  hysteresis: number;
  fanSpeed: number;
  temperatureSource: number;
  points: CurvePoint[];
  verified?: boolean;
}

export interface Reading {
  key: string;
  kind: string;
  title: string;
  unit: string;
  value: number | null;
  min: number | null;
  max: number | null;
  avg: number | null;
}

export interface Sample {
  t: number;
  readings: Reading[];
  headerRpms: number[];
  headerTemperatures: number[];
}

export interface Profile {
  id: string;
  name: string;
  createdAt: number;
  source: string | null;
  fans: FanCurve[];
}

export type AlertOp = 'above' | 'below';
export type AlertAction = 'notify' | 'fullSpeed';

export interface Alert {
  id: string;
  sensor: string;
  op: AlertOp;
  threshold: number;
  hysteresis: number;
  action: AlertAction;
  enabled: boolean;
}

export interface AlertEvent {
  id: string;
  sensor: string;
  active: boolean;
  value: number;
  threshold: number;
  action: AlertAction;
}

/** [duty %, RPM] */
export type CalibrationPoint = [number, number];

export interface Settings {
  pollMs: number;
  historyMinutes: number;
  closeToTray: boolean;
  startMinimized: boolean;
  startWithWindows: boolean;
  applyProfileOnStart: string | null;
  activeProfile: string | null;
  logCsv: boolean;
  logDir: string | null;
  alerts: Alert[];
  fanLabels: Record<string, string>;
  calibration: Record<string, CalibrationPoint[]>;
  hiddenSensors: string[];
  pinnedSensors: string[];
  lighting: LightingZone[] | null;
  restoreLighting: boolean;
  lightingPresets: LightingPreset[];
  gpu: GpuControl;
}

// ---- lighting -----------------------------------------------------------------

export interface LightingZone {
  index: number;
  mode: number;
  /** 0xRRGGBB */
  color: number;
  /** 1-9 */
  speed: number;
  /** 1-9 */
  brightness: number;
}

export interface RgbZone extends LightingZone {
  ledType: number;
  mapTo: number;
}

export interface RgbState {
  supported: boolean;
  devId: number;
  mcu: string;
  model: string;
  advanced: boolean;
  gccRunning: boolean;
  zones: RgbZone[];
}

export interface LightingPreset {
  id: string;
  name: string;
  zones: LightingZone[];
}

// ---- graphics ------------------------------------------------------------------

export interface GpuControl {
  coreOffset: number;
  memoryOffset: number;
  powerLimitW: number | null;
  tempLimitC: number | null;
  fanMode: 'auto' | 'manual' | 'curve';
  fanPercent: number;
  /** [temperature °C, fan %] */
  fanCurve: [number, number][];
  applyOnStart: boolean;
  osd: boolean;
}

export interface GpuDevice {
  index: number;
  name: string;
  temperature: number | null;
  fanPercent: number | null;
  fanRpm: number | null;
  fanCount: number;
  fanAuto: boolean;
  graphicsClock: number | null;
  memoryClock: number | null;
  utilization: number | null;
  memoryController: number | null;
  memoryUsed: number | null;
  memoryTotal: number | null;
  powerW: number | null;
  powerLimitW: number | null;
  powerDefaultW: number | null;
  powerMinW: number | null;
  powerMaxW: number | null;
  tempLimit: number | null;
  tempLimitMin: number | null;
  tempLimitMax: number | null;
  tempSlowdown: number | null;
  coreOffset: number | null;
  coreOffsetMin: number | null;
  coreOffsetMax: number | null;
  memoryOffset: number | null;
  memoryOffsetMin: number | null;
  memoryOffsetMax: number | null;
  performanceState: number | null;
  limits: string[];
}

export interface GpuFrames {
  rtss: boolean;
  fps: number | null;
  frameMs: number | null;
  app: string | null;
  onePercentLow: number | null;
}

export interface GpuSnapshot {
  available: boolean;
  message: string | null;
  devices: GpuDevice[];
  frames: GpuFrames;
  pendingConfirm: boolean;
  history: { t: number; temperature: number | null; powerW: number | null; utilization: number | null; graphicsClock: number | null; fps: number | null }[];
}

export interface GpuApply {
  index: number;
  coreOffset: number;
  memoryOffset: number;
  powerLimitW: number | null;
  tempLimitC: number | null;
  fanMode: string;
  fanPercent: number;
  fanCurve: [number, number][];
  applyOnStart: boolean;
  osd: boolean;
}

// ---- devices -------------------------------------------------------------------

export interface DeviceBattery {
  percent: number | null;
  charging: boolean | null;
}

export interface DeviceMouse {
  activeDpi: number;
  defaultDpi: number;
  levels: number[];
  reportRate: number | null;
  reportRates: number[];
}

export interface DeviceAudio {
  volume: number | null;
  muted: boolean | null;
  sidetone: number | null;
  sidetoneMuted: boolean | null;
}

export interface DeviceLighting {
  on: boolean | null;
  brightness: number | null;
}

export interface LogiDevice {
  id: string;
  name: string;
  kind: string;
  model: string;
  connection: string;
  state: string;
  battery: DeviceBattery | null;
  mouse: DeviceMouse | null;
  audio: DeviceAudio | null;
  lighting: DeviceLighting | null;
}

export interface KnownDevice {
  model: string;
  name: string;
  kind: string;
  lastSeen: number | null;
}

export interface DevicesSnapshot {
  agent: boolean;
  message: string | null;
  devices: LogiDevice[];
  known: KnownDevice[];
}

export interface DeviceApply {
  id: string;
  activeDpi?: number;
  dpiLevels?: number[];
  reportRate?: number;
  volume?: number;
  muted?: boolean;
  sidetone?: number;
  sidetoneMuted?: boolean;
  lightingOn?: boolean;
  brightness?: number;
}

// ---- pulsar mouse --------------------------------------------------------------

export interface PulsarSnapshot {
  present: boolean;
  message: string | null;
  model: string;
  firmware: string | null;
  online: boolean;
  battery: number | null;
  charging: boolean;
  dpiStages: number[];
  activeStage: number;
  reportRate: number | null;
  reportRates: number[];
}

export interface PulsarApply {
  dpiStages?: number[];
  activeStage?: number;
  reportRate?: number;
}

// ---- overclocking -------------------------------------------------------------

export interface OcOption {
  value: number;
  label: string;
  raw: number;
}

export interface OcRange {
  base: number;
  max: number;
  step: number;
  precision: number;
}

export interface OcItem {
  appId: number;
  key: string;
  title: string;
  hint: string | null;
  group: string;
  unit: string;
  encoding: 'unsigned' | 'signed' | 'biased';
  bias: number;
  current: number;
  default: number;
  active: number;
  boot: number;
  isAuto: boolean;
  autoOption: boolean;
  editable: boolean;
  engineEditable: boolean;
  live: boolean;
  limits: { min?: number; max?: number; maxAbs?: number; maxDeltaFromBoot?: number } | null;
  options: OcOption[];
  ranges: OcRange[];
}

export interface OcState {
  supported: boolean;
  iccReadOnly: boolean;
  items: OcItem[];
  pendingConfirm?: boolean;
}

export interface OcApplyResult {
  rebootRequired: boolean;
  changed: number;
  mismatched: string[];
  items: OcItem[];
  revertInSecs?: number;
}

// ---- storage ------------------------------------------------------------------

export interface SmartAttribute {
  id: number;
  name: string;
  current: number;
  worst: number;
  threshold: number;
  raw: number;
  failing: boolean;
}

export interface Volume {
  letter: string;
  label: string;
  fileSystem: string;
  size: number;
  free: number;
}

export interface Disk {
  index: number;
  model: string;
  vendor: string;
  serial: string;
  firmware: string;
  bus: string;
  size: number;
  temperature: number | null;
  temperatureWarning: number | null;
  temperatureCritical: number | null;
  health: number | null;
  healthLabel: 'Good' | 'Caution' | 'Bad' | 'Unknown';
  percentageUsed: number | null;
  availableSpare: number | null;
  spareThreshold: number | null;
  criticalWarning: number | null;
  warnings: string[];
  powerOnHours: number | null;
  powerCycles: number | null;
  unsafeShutdowns: number | null;
  mediaErrors: number | null;
  errorLogEntries: number | null;
  bytesRead: number | null;
  bytesWritten: number | null;
  smart: SmartAttribute[];
  volumes: Volume[];
}

// ---- updates ------------------------------------------------------------------

export interface DriverItem {
  id: number;
  name: string;
  description: string;
  category: string;
  installed: string;
  latest: string;
  outdated: boolean;
  size: number;
  url: string;
  fileName: string;
  devices: string[];
  source: 'device' | 'registry';
}

export interface DriverScan {
  revision: string;
  scannedAt: number;
  deviceCount: number;
  items: DriverItem[];
}

export interface InstallResult {
  id: number;
  name: string;
  ok: boolean;
  exitCode: number | null;
  reboot: boolean;
  signer: string;
  message: string;
}

export interface UpdateProgress {
  phase: 'catalog' | 'devices' | 'sizes' | 'download' | 'verify' | 'install' | 'done' | 'failed';
  id?: number | 'bios';
  index?: number;
  count?: number;
  name?: string;
  received?: number;
  total?: number;
  signer?: string;
  message?: string;
}

export interface BiosInfo {
  model: string;
  partNumber: string | null;
  installed: string;
  installedDate: string | null;
  latest: string | null;
  updateAvailable: boolean;
  url: string | null;
  size: number;
  downloaded: string | null;
  note: string | null;
}

export interface BiosDownload {
  version: string;
  image: string;
  folder: string;
  size: number;
}

export interface UsbDrive {
  letter: string;
  label: string;
  fileSystem: string;
  size: number;
  free: number;
  fat32: boolean;
}

export interface FlashReadiness {
  flasherFound: boolean;
  onAcPower: boolean | null;
  batteryPercent: number | null;
  bitlockerOn: boolean | null;
  image: string | null;
}

export interface Overrides {
  failsafe: boolean;
  manual: Record<string, number>;
  calibrating: number | null;
}

export interface CalibrationProgress {
  index: number;
  step: number;
  total: number;
  percent: number;
  rpm: number;
}

export type History = Record<string, [number, number][]>;
