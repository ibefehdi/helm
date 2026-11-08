using System;
using System.Collections.Generic;
using System.Linq;
using Gigabyte.EasyTune.Common;
using Gigabyte.EasyTune.Common.Overclock;
using Gigabyte.Engine.EasyTune.Overclock;

namespace HelmBridge
{
    /// <summary>
    /// EasyTune overclocking through the engine's OverclockAdvanced module. Values are the engine's raw
    /// 32-bit encodings (two's complement for signed items); display value = raw / precision.
    /// </summary>
    internal sealed class Overclock : IDisposable
    {
        private static readonly OverClockControlTypes[] ReadTypes =
        {
            OverClockControlTypes.Cores, OverClockControlTypes.Frequency, OverClockControlTypes.Memory, OverClockControlTypes.Voltage,
        };

        /// <summary>
        /// Items Helm will write, with hard limits GCC does not enforce. Everything else is reported read-only.
        /// Application IDs come from MB.CPNT.EZTune's AmdOverclockDataFilter / OverclockDataFilter.
        /// </summary>
        private static readonly Dictionary<int, Rule> Rules = new Dictionary<int, Rule>
        {
            [1] = new Rule("bclk", "Base clock", live: true, maxDeltaFromBoot: 3.0),
            [2] = new Rule("ratio", "CPU ratio", live: true),
            [32773] = new Rule("vcoreMode", "Vcore mode", live: true),
            [5] = new Rule("vcore", "Vcore", live: true, max: 1.35),
            [6] = new Rule("vcoreOffset", "Vcore offset", live: true, maxAbs: 0.10),
            [9] = new Rule("memRatio", "Memory multiplier", live: false),
            [10] = new Rule("expo", "EXPO / XMP profile", live: false),
            [15] = new Rule("dramVdd", "DRAM VDD", live: true, max: 1.45),
            [16] = new Rule("dramVddq", "DRAM VDDQ", live: true, max: 1.45),
            [24] = new Rule("dramVpp", "DRAM VPP", live: true, max: 1.90),
        };

        private const uint AutoValue = 0x80000000;

        private readonly OverclockAdvancedControlModule _module;
        private readonly Dictionary<int, uint> _boot = new Dictionary<int, uint>();

        public Overclock()
        {
            _module = new OverclockAdvancedControlModule();
            if (!_module.IsSupported)
                throw new BridgeException("oc_unsupported", "EasyTune overclocking is not supported on this system.");
            foreach (var entry in ReadAll()) _boot[entry.Value.Data.ApplicationId] = entry.Value.Data.CurrentValue;
        }

        private sealed class Rule
        {
            public readonly string Key;
            public readonly string Label;
            public readonly bool Live;
            public readonly double? Max;
            public readonly double? MaxAbs;
            public readonly double? MaxDeltaFromBoot;

            public Rule(string key, string label, bool live, double? max = null, double? maxAbs = null, double? maxDeltaFromBoot = null)
            {
                Key = key;
                Label = label;
                Live = live;
                Max = max;
                MaxAbs = maxAbs;
                MaxDeltaFromBoot = maxDeltaFromBoot;
            }
        }

        private struct Entry
        {
            public OverClockControlTypes ControlType;
            public TuningConfigData Data;
        }

        private Dictionary<int, Entry> ReadAll()
        {
            _module.Refresh();
            var all = new Dictionary<int, Entry>();
            foreach (OverClockControlTypes type in ReadTypes)
            {
                var list = new List<TuningConfigData>();
                try { _module.GetData(type, ref list); }
                catch (NotSupportedException) { continue; }
                foreach (TuningConfigData d in list ?? new List<TuningConfigData>())
                    if (!all.ContainsKey(d.ApplicationId)) all[d.ApplicationId] = new Entry { ControlType = type, Data = d };
            }
            return all;
        }

        public Dictionary<string, object> Describe()
        {
            var items = ReadAll().Values.Select(Describe).ToList();
            return new Dictionary<string, object>
            {
                ["supported"] = true,
                ["iccReadOnly"] = _module.IsIccReadOnly,
                ["items"] = items,
            };
        }

        private Dictionary<string, object> Describe(Entry e)
        {
            TuningConfigData d = e.Data;
            Rules.TryGetValue(d.ApplicationId, out Rule rule);
            _boot.TryGetValue(d.ApplicationId, out uint boot);
            return new Dictionary<string, object>
            {
                ["appId"] = d.ApplicationId,
                ["key"] = rule?.Key,
                ["title"] = string.IsNullOrWhiteSpace(d.Title) ? rule?.Label ?? $"Item {d.ApplicationId}" : d.Title.Trim(),
                ["hint"] = d.Hint,
                ["group"] = e.ControlType.ToString().ToLowerInvariant(),
                ["unit"] = d.Unit,
                ["encoding"] = EncodingOf(d),
                ["bias"] = IsBiased(d) ? (long)Bias : 0L,
                ["current"] = (long)d.CurrentValue,
                ["default"] = (long)d.DefaultValue,
                ["active"] = (long)d.ActiveValue,
                ["boot"] = (long)boot,
                ["isAuto"] = d.IsAutoOption && d.CurrentValue == AutoValue,
                ["autoOption"] = d.IsAutoOption,
                ["editable"] = d.IsEditable && rule != null,
                ["engineEditable"] = d.IsEditable,
                ["live"] = rule?.Live ?? true,
                ["limits"] = rule == null ? null : new Dictionary<string, object>
                {
                    ["max"] = rule.Max,
                    ["maxAbs"] = rule.MaxAbs,
                    ["maxDeltaFromBoot"] = rule.MaxDeltaFromBoot,
                },
                ["options"] = Options(d),
                ["ranges"] = d.ValueRanges
                    .Where(r => !(r.BaseValue == 0 && r.MaximumValue == 0))
                    .Select(r => new Dictionary<string, object>
                    {
                        ["base"] = (long)r.BaseValue,
                        ["max"] = (long)r.MaximumValue,
                        ["step"] = (long)r.Increment,
                        ["precision"] = (long)(r.Precision == 0 ? 1 : r.Precision),
                    }).ToList(),
            };
        }

        /// <summary>Validates and writes changes, then reads them back. Reboot-only items require <paramref name="allowReboot"/>.</summary>
        public Dictionary<string, object> Apply(IEnumerable<KeyValuePair<int, long>> changes, bool allowReboot)
        {
            Dictionary<int, Entry> current = ReadAll();
            var byType = new Dictionary<OverClockControlTypes, List<SaveTunningConfigData>>();
            var written = new Dictionary<int, uint>();

            foreach (var change in changes)
            {
                if (!Rules.TryGetValue(change.Key, out Rule rule))
                    throw new BridgeException("oc_not_allowed", $"Helm does not write item {change.Key}.");
                if (!current.TryGetValue(change.Key, out Entry entry))
                    throw new BridgeException("oc_item_missing", $"{rule.Label} is not exposed on this platform.");
                if (!entry.Data.IsEditable)
                    throw new BridgeException("oc_not_editable", $"{rule.Label} is locked on this platform.");
                if (!rule.Live && !allowReboot)
                    throw new BridgeException("oc_reboot_required", $"{rule.Label} is written to the BIOS and needs a restart; confirm to continue.");

                if (change.Value < 0 || change.Value > uint.MaxValue)
                    throw new BridgeException("bad_args", $"{rule.Label}: values are raw unsigned 32-bit encodings.");
                uint raw = (uint)change.Value;
                Validate(entry.Data, rule, raw);
                if (raw == entry.Data.CurrentValue) continue;

                if (!byType.TryGetValue(entry.ControlType, out var list)) byType[entry.ControlType] = list = new List<SaveTunningConfigData>();
                list.Add(new SaveTunningConfigData { ApplicationId = change.Key, Type = entry.Data.Type, CurrentValue = raw, IsEditable = true });
                written[change.Key] = raw;
            }

            bool reboot = false;
            foreach (var kv in byType)
            {
                var list = kv.Value;
                _module.SetData(kv.Key, ref list, out bool needsReboot);
                reboot |= needsReboot;
            }

            Dictionary<int, Entry> after = ReadAll();
            var mismatched = written
                .Where(w => !after.TryGetValue(w.Key, out Entry e) || (Rules[w.Key].Live && e.Data.CurrentValue != w.Value))
                .Select(w => Rules[w.Key].Label)
                .ToList();

            return new Dictionary<string, object>
            {
                ["rebootRequired"] = reboot,
                ["changed"] = written.Count,
                ["mismatched"] = mismatched,
                ["items"] = after.Values.Select(Describe).ToList(),
            };
        }

        /// <summary>Writes the values seen when Helm first connected back to every live item that differs.</summary>
        public Dictionary<string, object> RestoreBoot()
        {
            Dictionary<int, Entry> current = ReadAll();
            var changes = _boot
                .Where(b => Rules.TryGetValue(b.Key, out Rule r) && r.Live && current.TryGetValue(b.Key, out Entry e) && e.Data.IsEditable && e.Data.CurrentValue != b.Value)
                .Select(b => new KeyValuePair<int, long>(b.Key, b.Value))
                .ToList();
            // Write the mode before the values that depend on it.
            changes.Sort((a, b) => (b.Key == 32773).CompareTo(a.Key == 32773));
            return Apply(changes, allowReboot: false);
        }

        private void Validate(TuningConfigData d, Rule rule, uint raw)
        {
            if (raw == AutoValue && d.IsAutoOption) return;
            if (Options(d).Any(o => Convert.ToInt64(o["raw"]) == raw)) return;

            bool signed = d.IsSignSymbol && !IsBiased(d);
            TuningValueRange range = d.ValueRanges.FirstOrDefault(r => InRange(r, raw, signed));
            if (range == null)
                throw new BridgeException("oc_out_of_range", $"{rule.Label}: value is outside the engine's allowed range.");

            double real = Real(d, raw, range);
            if (rule.Max.HasValue && real > rule.Max.Value + 1e-9)
                throw new BridgeException("oc_limit", $"{rule.Label} is capped at {rule.Max.Value:0.###} by Helm.");
            if (rule.MaxAbs.HasValue && Math.Abs(real) > rule.MaxAbs.Value + 1e-9)
                throw new BridgeException("oc_limit", $"{rule.Label} is capped at ±{rule.MaxAbs.Value:0.###} by Helm.");
            if (rule.MaxDeltaFromBoot.HasValue && _boot.TryGetValue(d.ApplicationId, out uint boot))
            {
                if (Math.Abs(real - Real(d, boot, range)) > rule.MaxDeltaFromBoot.Value + 1e-9)
                    throw new BridgeException("oc_limit", $"{rule.Label} can only move ±{rule.MaxDeltaFromBoot.Value:0.#} from its boot value without a restart.");
            }
        }

        private static bool InRange(TuningValueRange r, uint raw, bool signed)
        {
            long value = signed ? Signed(raw) : raw;
            long lo = signed ? Signed(r.BaseValue) : r.BaseValue;
            long hi = signed ? Signed(r.MaximumValue) : r.MaximumValue;
            if (value < Math.Min(lo, hi) || value > Math.Max(lo, hi)) return false;
            return r.Increment <= 1 || (value - lo) % r.Increment == 0;
        }

        /// <summary>
        /// DVID offsets are encoded around 0x7FFFFFFF (= 0 V): the engine reports e.g. 0x7FFFFED3..0x8000012B for ±0.3 V.
        /// </summary>
        private const uint Bias = 0x7FFFFFFF;

        private static bool IsBiased(TuningConfigData d) =>
            d.ValueRanges.Any(r => r.BaseValue <= Bias && r.MaximumValue > Bias && r.BaseValue >= 0x7F000000);

        private static string EncodingOf(TuningConfigData d) => IsBiased(d) ? "biased" : d.IsSignSymbol ? "signed" : "unsigned";

        private static double Real(TuningConfigData d, uint raw, TuningValueRange range)
        {
            double precision = range.Precision == 0 ? 1 : range.Precision;
            if (IsBiased(d)) return ((long)raw - Bias) / precision;
            return (d.IsSignSymbol ? Signed(raw) : raw) / precision;
        }

        private static long Signed(uint raw) => unchecked((int)raw);

        /// <summary>
        /// Options from SpecialString. When each option has its own single-value range (e.g. Vcore mode:
        /// "0=Auto,1=Dynamic Offset,2=Static" with ranges 0x80000000/1/2), that range value is what gets written.
        /// </summary>
        private static List<Dictionary<string, object>> Options(TuningConfigData d)
        {
            List<Dictionary<string, object>> options = ParseOptions(d.SpecialString);
            List<TuningValueRange> singles = d.ValueRanges.Where(r => r.BaseValue == r.MaximumValue).ToList();
            bool mapped = singles.Count == options.Count && singles.Count == d.ValueRanges.Count && options.Count > 0;
            for (int i = 0; i < options.Count; i++)
                options[i]["raw"] = mapped ? singles[i].BaseValue : (long)unchecked((uint)Convert.ToInt64(options[i]["value"]));
            return options;
        }

        /// <summary>Parses "0=Disable,1=Profile 1" style option lists.</summary>
        private static List<Dictionary<string, object>> ParseOptions(string special)
        {
            var options = new List<Dictionary<string, object>>();
            if (string.IsNullOrWhiteSpace(special)) return options;
            foreach (string part in special.Split(','))
            {
                int eq = part.IndexOf('=');
                if (eq <= 0) continue;
                if (long.TryParse(part.Substring(0, eq).Trim(), out long value))
                    options.Add(new Dictionary<string, object> { ["value"] = value, ["label"] = part.Substring(eq + 1).Trim() });
            }
            return options;
        }

        public void Dispose()
        {
            try { _module.Dispose(); } catch { }
        }
    }
}
