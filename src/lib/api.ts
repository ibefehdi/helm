import { invoke } from '@tauri-apps/api/core';
import type {
  BiosDownload,
  BiosInfo,
  CalibrationPoint,
  Disk,
  DriverScan,
  FanCurve,
  FlashReadiness,
  History,
  Info,
  InstallResult,
  LightingPreset,
  LightingZone,
  DeviceApply,
  DevicesSnapshot,
  GpuApply,
  GpuSnapshot,
  OcApplyResult,
  OcState,
  Overrides,
  Profile,
  RgbState,
  Sample,
  Settings,
  Status,
  UsbDrive,
} from './types';

export const api = {
  status: () => invoke<Status>('get_status'),
  info: () => invoke<Info>('get_info'),
  reconnect: () => invoke<void>('reconnect'),
  relaunchAsAdmin: () => invoke<void>('relaunch_as_admin'),

  fans: () => invoke<FanCurve[]>('get_fans'),
  setFan: (curve: FanCurve) => invoke<FanCurve>('set_fan', { curve }),
  setFans: (curves: FanCurve[]) => invoke<FanCurve[]>('set_fans', { curves }),
  forceFan: (index: number, percent: number) => invoke<void>('force_fan', { index, percent }),
  releaseFans: () => invoke<void>('release_fans'),
  overrides: () => invoke<Overrides>('get_overrides'),
  calibrate: (index: number) => invoke<CalibrationPoint[]>('calibrate_fan', { index }),
  cancelCalibration: () => invoke<void>('cancel_calibration'),

  latest: () => invoke<Sample | null>('get_latest'),
  history: (keys?: string[], maxPoints?: number) => invoke<History>('get_history', { keys, maxPoints }),
  resetStats: () => invoke<void>('reset_stats'),
  logDir: () => invoke<string>('log_dir'),
  openLogDir: () => invoke<void>('open_log_dir'),

  profiles: () => invoke<Profile[]>('list_profiles'),
  saveProfile: (name: string, fans?: FanCurve[]) => invoke<Profile>('save_profile', { name, fans }),
  renameProfile: (id: string, name: string) => invoke<void>('rename_profile', { id, name }),
  deleteProfile: (id: string) => invoke<void>('delete_profile', { id }),
  applyProfile: (id: string) => invoke<FanCurve[]>('apply_profile', { id }),
  importGcc: () => invoke<Profile[]>('import_gcc'),
  exportProfile: (id: string, path: string) => invoke<void>('export_profile', { id, path }),
  importProfileFile: (path: string) => invoke<Profile[]>('import_profile_file', { path }),

  settings: () => invoke<Settings>('get_settings'),
  updateSettings: (settings: Settings) => invoke<Settings>('update_settings', { settings }),

  rgbGet: () => invoke<RgbState>('rgb_get'),
  rgbSet: (zones: LightingZone[]) => invoke<RgbState>('rgb_set', { zones }),
  saveLightingPreset: (name: string, zones: LightingZone[]) => invoke<LightingPreset>('save_lighting_preset', { name, zones }),
  deleteLightingPreset: (id: string) => invoke<void>('delete_lighting_preset', { id }),

  ocGet: () => invoke<OcState>('oc_get'),
  ocApply: (changes: { appId: number; value: number }[], allowReboot: boolean) =>
    invoke<OcApplyResult>('oc_apply', { changes, allowReboot }),
  ocConfirm: () => invoke<boolean>('oc_confirm'),
  ocRestore: () => invoke<OcApplyResult>('oc_restore'),

  gpuGet: () => invoke<GpuSnapshot>('gpu_get'),
  gpuApply: (change: GpuApply) => invoke<GpuSnapshot>('gpu_apply', { change }),
  gpuConfirm: () => invoke<boolean>('gpu_confirm'),
  gpuRestore: () => invoke<GpuSnapshot>('gpu_restore'),

  devicesGet: () => invoke<DevicesSnapshot>('devices_get'),
  devicesApply: (change: DeviceApply) => invoke<DevicesSnapshot>('devices_apply', { change }),

  storageScan: () => invoke<Disk[]>('storage_scan'),
  storageTrim: (letter: string) => invoke<string>('storage_trim', { letter }),

  updatesScan: () => invoke<DriverScan>('updates_scan'),
  updatesInstall: (ids: number[]) => invoke<InstallResult[]>('updates_install', { ids }),
  updatesCancel: () => invoke<void>('updates_cancel'),
  biosCheck: () => invoke<BiosInfo>('bios_check'),
  biosDownload: () => invoke<BiosDownload>('bios_download'),
  biosUsbDrives: () => invoke<UsbDrive[]>('bios_usb_drives'),
  biosCopyToUsb: (letter: string) => invoke<string>('bios_copy_to_usb', { letter }),
  biosFlashReadiness: () => invoke<FlashReadiness>('bios_flash_readiness'),
  biosFlash: (confirmModel: string, suspendBitlocker: boolean) => invoke<void>('bios_flash', { confirmModel, suspendBitlocker }),
  openDownloads: () => invoke<void>('open_downloads'),
};

export function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e;
  if (e && typeof e === 'object' && 'message' in e) return String((e as { message: unknown }).message);
  return String(e);
}
