using System;
using System.Collections;
using System.Collections.Generic;
using System.Linq;
using Gigabyte.Engine.EnvironmentControl.CoolingDevice.Fan;
using Gigabyte.Engine.EnvironmentControl.HardwareMonitor;
using Gigabyte.EnvironmentControl.Common.CoolingDevice.Fan;
using Gigabyte.EnvironmentControl.Common.CoolingDevice.Fan.CoordinateSystem;
using Gigabyte.EnvironmentControl.Common.HardwareMonitor;

namespace HelmBridge
{
    /// <summary>
    /// Thin adapter over the remoting client modules GCC itself uses
    /// (ipc://GigabyteEngine/EnvironmentControl/{HardwareMonitor,SmartGuardianFanControlCoordinateSystem}).
    /// </summary>
    internal sealed class Engine : IDisposable
    {
        // The SIO firmware stores at most 8 curve points per header (MaximumSlope + 2 in Gigabyte.SIV.Profile).
        private const int MaxCurvePoints = 8;
        // SmartGuardianFanConfigConverter.ToPhysicalLayerFormat silently drops curves with fewer points.
        private const int MinCurvePoints = 3;
        // Temperature limits are written as bytes; the ITE full-speed limit tops out at 127 °C.
        private const int MaxCurveTemperature = 127;

        private readonly SmartGuardianFanControlCoordinateSystemModule _fans;
        private readonly HardwareMonitorControlModule _monitor;

        private Dictionary<string, object> _description;
        private bool _forced;
        private bool _lastWriteVerified;

        public Engine()
        {
            _fans = new SmartGuardianFanControlCoordinateSystemModule();
            _monitor = new HardwareMonitorControlModule();

            if (_monitor.SourceType == HardwareMonitorSourceTypes.None)
                _monitor.Initialize(HardwareMonitorSourceTypes.HwRegister);

            if (!_fans.IsSupported)
                throw new BridgeException("fans_unsupported", "The engine reports that fan control is not supported on this board.");
        }

        public Dictionary<string, object> Describe()
        {
            if (_description != null)
            {
                RefreshTemperatureSelections();
                return _description;
            }

            EnvironmentControllerInfo hw = _fans.HardwareInfo;

            var temperatureSources = new List<object>();
            for (int i = 0; i < hw.TemperatureInputCount; i++)
            {
                _fans.GetSensorTitle(SensorTypes.Temperature, i, out string title);
                temperatureSources.Add(new Dictionary<string, object> { ["index"] = i, ["title"] = title ?? ("Temp " + (i + 1)) });
            }

            var fans = new List<object>();
            for (int i = 0; i < hw.FanControlCount; i++)
            {
                _fans.GetSensorTitle(SensorTypes.Fan, i, out string title);
                _fans.GetTemperatureInputSelection(i, out int source);
                fans.Add(new Dictionary<string, object>
                {
                    ["index"] = i,
                    ["title"] = string.IsNullOrWhiteSpace(title) ? "Fan " + (i + 1) : title,
                    ["temperatureSource"] = source,
                });
            }

            _description = new Dictionary<string, object>
            {
                ["groupNumber"] = hw.GroupNumber.ToString("X"),
                ["platformNumber"] = hw.PlatformNumber,
                ["fanCount"] = hw.FanControlCount,
                ["tachometerCount"] = hw.FanTachometerInputCount,
                ["temperatureInputCount"] = hw.TemperatureInputCount,
                ["voltageInputCount"] = hw.VoltageInputCount,
                ["maxCurvePoints"] = MaxCurvePoints,
                ["temperatureSources"] = temperatureSources,
                ["fans"] = fans,
            };
            return _description;
        }

        private void RefreshTemperatureSelections()
        {
            foreach (Dictionary<string, object> fan in (List<object>)_description["fans"])
            {
                _fans.GetTemperatureInputSelection((int)fan["index"], out int source);
                fan["temperatureSource"] = source;
            }
        }

        public Dictionary<string, object> ReadSensors()
        {
            var collection = new HardwareMonitoredDataCollection();
            _monitor.GetCurrentMonitoredData(ref collection, false, false);

            var sensors = collection
                .Where(d => d != null)
                .Select(d => (object)new Dictionary<string, object>
                {
                    ["type"] = d.Type.ToString(),
                    ["title"] = d.Title,
                    ["uuid"] = (int)d.DeviceUUID,
                    ["value"] = d.Value,
                    ["unit"] = d.Unit,
                    ["visible"] = d.DisplayItem,
                })
                .ToList();

            var headerTemperatures = new List<float>();
            var headerRpms = new List<float>();
            _fans.GetHardwareMonitorDatas(ref headerTemperatures, ref headerRpms);

            return new Dictionary<string, object>
            {
                ["timestamp"] = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds(),
                ["sensors"] = sensors,
                ["headerTemperatures"] = headerTemperatures,
                ["headerRpms"] = headerRpms,
            };
        }

        public Dictionary<string, object> GetFan(int index)
        {
            RequireFanIndex(index);
            var set = new SmartFanDataPointSet();
            _fans.Get(index, ref set);
            _fans.GetTemperatureInputSelection(index, out int source);

            return new Dictionary<string, object>
            {
                ["index"] = index,
                ["mode"] = set.Mode.ToString(),
                ["hysteresis"] = set.DeltaTemperature,
                ["fanSpeed"] = set.FanSpeed,
                ["temperatureSource"] = source,
                ["points"] = set.DataPoints
                    .Where(p => p != null && p.IsDefined())
                    .Select(p => (object)new List<object> { p.X, p.Y })
                    .ToList(),
            };
        }

        public List<object> GetAllFans()
        {
            int count = _fans.HardwareInfo.FanControlCount;
            var result = new List<object>(count);
            for (int i = 0; i < count; i++) result.Add(GetFan(i));
            return result;
        }

        /// <returns>Whether reading the curve back matched what was written.</returns>
        public bool SetFan(int index, Dictionary<string, object> args)
        {
            RequireFanIndex(index);

            var current = new SmartFanDataPointSet();
            _fans.Get(index, ref current);

            var next = new SmartFanDataPointSet
            {
                Mode = ParseMode(Arg.Str(args, "mode", current.Mode.ToString())),
                DeltaTemperature = Clamp(Arg.Int(args, "hysteresis", current.DeltaTemperature), 0, 10),
                FanSpeed = Arg.Int(args, "fanSpeed", current.FanSpeed),
            };

            if (!args.TryGetValue("points", out object raw) || !(raw is IEnumerable rawPoints))
                throw new BridgeException("bad_args", "points is required");

            foreach (object item in rawPoints)
            {
                var pair = ((IEnumerable)item).Cast<object>().Select(Convert.ToDouble).ToArray();
                if (pair.Length != 2) throw new BridgeException("bad_args", "Each point must be [temperature, pwm]");
                next.DataPoints.Add(new SmartFanDataPoint(Math.Round(pair[0]), Math.Round(pair[1])));
            }
            NormalizeCurve(next);
            ValidateCurve(next.DataPoints);

            _fans.Set(index, next);
            _lastWriteVerified = VerifyWrite(index, next);

            int source = Arg.Int(args, "temperatureSource", -1);
            if (source >= 0)
            {
                if (source >= _fans.HardwareInfo.TemperatureInputCount)
                    throw new BridgeException("bad_args", "temperatureSource out of range");
                _fans.GetTemperatureInputSelection(index, out int currentSource);
                if (currentSource != source && !_fans.SetTemperatureInputSelection(index, source))
                    throw new BridgeException("write_failed", "The engine rejected the temperature source change.");
            }
            return _lastWriteVerified;
        }

        public void ForceFan(int index, int percent)
        {
            RequireFanIndex(index);
            _fans.SetCalibrationPwm(index, Clamp(percent, 0, 100));
            _forced = true;
        }

        public void ReleaseFans()
        {
            _fans.ResetCalibrationPwm();
            _forced = false;
        }

        public void ReleaseFansIfForced()
        {
            if (_forced) ReleaseFans();
        }

        public void Dispose()
        {
            ReleaseFansIfForced();
            _fans.Dispose();
            _monitor.Dispose();
        }

        private void RequireFanIndex(int index)
        {
            if (index < 0 || index >= _fans.HardwareInfo.FanControlCount)
                throw new BridgeException("bad_args", "Fan index out of range: " + index);
        }

        // Point 0 only contributes its temperature (the "fan off below" limit); its duty is ignored by the
        // firmware, so mirror the start duty to keep what we report identical to what the fan does.
        private static void NormalizeCurve(SmartFanDataPointSet set)
        {
            List<SmartFanDataPoint> points = set.DataPoints;
            if (points.Count >= 2) points[0].Y = points[1].Y;
        }

        private static void ValidateCurve(List<SmartFanDataPoint> points)
        {
            if (points.Count < MinCurvePoints || points.Count > MaxCurvePoints)
                throw new BridgeException("bad_curve", $"A curve needs between {MinCurvePoints} and {MaxCurvePoints} points.");

            for (int i = 0; i < points.Count; i++)
            {
                SmartFanDataPoint p = points[i];
                if (p.X < 0 || p.X > MaxCurveTemperature) throw new BridgeException("bad_curve", $"Temperatures must be within 0-{MaxCurveTemperature} °C.");
                if (p.Y < 0 || p.Y > 255) throw new BridgeException("bad_curve", "PWM must be within 0-255.");
                if (i > 0 && p.X <= points[i - 1].X) throw new BridgeException("bad_curve", "Curve temperatures must be strictly ascending (slopes are stored per segment).");
            }
            if (points[points.Count - 1].Y != 255)
                throw new BridgeException("bad_curve", "The last point must be 100% duty; the firmware discards curves that never reach full speed.");
        }

        private bool VerifyWrite(int index, SmartFanDataPointSet written)
        {
            var readBack = new SmartFanDataPointSet();
            _fans.Get(index, ref readBack);
            List<SmartFanDataPoint> got = readBack.DataPoints.Where(p => p != null && p.IsDefined()).ToList();
            if (got.Count != written.DataPoints.Count) return false;
            for (int i = 0; i < got.Count; i++)
            {
                // Slopes are quantised by the firmware, so allow a few PWM steps of drift on intermediate points.
                if (Math.Abs(got[i].X - written.DataPoints[i].X) > 0.5) return false;
                if (Math.Abs(got[i].Y - written.DataPoints[i].Y) > 6) return false;
            }
            return true;
        }

        private static FanConfigMode ParseMode(string mode)
        {
            if (Enum.TryParse(mode, true, out FanConfigMode parsed) && parsed != FanConfigMode.Unknown) return parsed;
            throw new BridgeException("bad_args", "Unsupported fan mode: " + mode);
        }

        private static int Clamp(int v, int lo, int hi) => v < lo ? lo : v > hi ? hi : v;
    }
}
