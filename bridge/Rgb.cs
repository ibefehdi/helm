using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Threading;
using System.Web.Script.Serialization;

namespace HelmBridge
{
    /// <summary>
    /// Motherboard RGB through GIGABYTE's RgbMotherboard.dll (ITE USB/SMBus LED controllers via GHidApi.dll).
    /// It runs in its own AppDomain whose base directory is a Helm-owned folder: the library looks for
    /// rgbcfg.xml / usdata2.xml / sp.xml there and rewrites usdata2.xml, so GCC's install is never touched.
    /// </summary>
    internal sealed class Rgb : IDisposable
    {
        internal const string GccRoot = @"C:\Program Files\GIGABYTE\Control Center";
        private static readonly string[] ConfigFiles = { "rgbcfg.xml", "usdata2.xml", "sp.xml" };

        private readonly AppDomain _domain;
        private readonly RgbWorker _worker;
        private static readonly JavaScriptSerializer Json = new JavaScriptSerializer { MaxJsonLength = int.MaxValue };

        public Rgb()
        {
            string lib = Path.Combine(GccRoot, @"Lib\GBT_rgbMotherboard_UC\RgbMotherboard.dll");
            if (!File.Exists(lib))
                throw new BridgeException("rgb_unavailable", "GIGABYTE's RGB Fusion module is not installed with Control Center.");

            string work = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData), @"Helm\gcc-rgb");
            Directory.CreateDirectory(work);
            foreach (string file in ConfigFiles)
            {
                string source = Path.Combine(GccRoot, file);
                string target = Path.Combine(work, file);
                if (!File.Exists(target) && File.Exists(source)) File.Copy(source, target);
            }

            // RgbMotherboard builds paths as BaseDirectory + "file.xml", so the trailing separator matters.
            _domain = AppDomain.CreateDomain("helm-rgb", null, new AppDomainSetup { ApplicationBase = work + Path.DirectorySeparatorChar });
            _worker = (RgbWorker)_domain.CreateInstanceFromAndUnwrap(typeof(RgbWorker).Assembly.Location, typeof(RgbWorker).FullName);
            _worker.Init(GccRoot);
        }

        public static bool GccRunning() => Process.GetProcessesByName("GCC").Length > 0;

        public Dictionary<string, object> Describe()
        {
            var state = (Dictionary<string, object>)Json.DeserializeObject(_worker.Describe());
            state["gccRunning"] = GccRunning();
            return state;
        }

        public Dictionary<string, object> Set(object[] zones)
        {
            int hr = _worker.Set(Json.Serialize(zones));
            if (hr != 0) throw new BridgeException("rgb_write_failed", $"The LED controller rejected the update (0x{hr:X8}).");
            return Describe();
        }

        public void Dispose()
        {
            try { _worker.Stop(); } catch { }
            try { AppDomain.Unload(_domain); } catch { }
        }
    }

    /// <summary>Lives inside the RGB AppDomain. Only strings and ints cross the boundary.</summary>
    public sealed class RgbWorker : MarshalByRefObject
    {
        /// <summary>Firmware-driven effects only: these keep running without a host thread.</summary>
        private static readonly HashSet<int> AllowedModes = new HashSet<int> { 0, 1, 4, 8, 9, 11, 72 };
        private const int ApplyAdvanced = 3;
        private const int ApplyStopHostEffects = 7;

        [DllImport("kernel32", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern bool SetDllDirectory(string path);

        // RGB_Motherboard is used late-bound: the library and its dependencies only resolve inside this domain.
        private object _board;
        private Type _type;
        private readonly JavaScriptSerializer _json = new JavaScriptSerializer();

        public override object InitializeLifetimeService() => null;

        public void Init(string gccRoot)
        {
            SetDllDirectory(gccRoot);
            string[] dirs = { Path.Combine(gccRoot, @"Lib\GBT_rgbMotherboard_UC"), Path.Combine(gccRoot, @"Lib\COMMDLL"), gccRoot };
            AppDomain.CurrentDomain.AssemblyResolve += (s, e) =>
            {
                string name = new AssemblyName(e.Name).Name + ".dll";
                foreach (string dir in dirs)
                {
                    string path = Path.Combine(dir, name);
                    if (File.Exists(path)) return Assembly.LoadFrom(path);
                }
                return null;
            };

            Assembly lib = Assembly.LoadFrom(Path.Combine(dirs[0], "RgbMotherboard.dll"));
            _type = lib.GetType("RgbMotherboard.RGB_Motherboard", throwOnError: true);
            _board = Activator.CreateInstance(_type);
            if (DevId == 0) return;
            for (int i = 0; i < 20 && Call<int>("GetGen2ScanState") != 1; i++) Thread.Sleep(200);
        }

        private int DevId => _board == null ? 0 : (int)_type.GetProperty("DevId").GetValue(_board);

        private T Call<T>(string method, params object[] args)
        {
            Type[] types = args.Select(a => a.GetType()).ToArray();
            MethodInfo m = _type.GetMethod(method, types) ?? throw new MissingMethodException(_type.FullName, method);
            try { return (T)m.Invoke(_board, args); }
            catch (TargetInvocationException ex) when (ex.InnerException != null) { throw ex.InnerException; }
        }

        private void Call(string method, params object[] args) => Call<object>(method, args);

        public string Describe()
        {
            if (DevId == 0)
                return _json.Serialize(new Dictionary<string, object> { ["supported"] = false });

            var layout = ParseList(Call<string>("GetLedLayoutInfo"));
            var param = ParseList(Call<string>("GetLedSetting"));
            var zones = new List<Dictionary<string, object>>();
            for (int i = 0; i < param.Count; i++)
            {
                Dictionary<string, object> p = param[i];
                Dictionary<string, object> l = layout.FirstOrDefault(x => Convert.ToInt32(x["zIdx"]) == i);
                int ledType = l != null ? Convert.ToInt32(l["led_type"]) : Convert.ToInt32(p["ledType"]);
                int mapTo = l != null ? Convert.ToInt32(l["mapTo"]) : -1;
                zones.Add(new Dictionary<string, object>
                {
                    ["index"] = i,
                    ["ledType"] = ledType,
                    ["mapTo"] = mapTo,
                    ["mode"] = Convert.ToInt32(p["ledMode"]),
                    ["color"] = Convert.ToInt32(p["ledColor0"]) & 0xFFFFFF,
                    ["speed"] = Convert.ToInt32(p["spLv"]) + 1,
                    ["brightness"] = Convert.ToInt32(p["briLv"]) + 1,
                });
            }

            return _json.Serialize(new Dictionary<string, object>
            {
                ["supported"] = true,
                ["devId"] = DevId,
                ["mcu"] = Call<int>("GetMcuType"),
                ["model"] = _type.GetProperty("DeviceName").GetValue(_board) as string,
                ["advanced"] = DevId != 16777218,
                ["domain"] = AppDomain.CurrentDomain.FriendlyName,
                ["zones"] = zones,
            });
        }

        /// <summary>Zones: [{index, mode, color (0xRRGGBB), speed 1-9, brightness 1-9}]. Unlisted zones keep their setting.</summary>
        public int Set(string zonesJson)
        {
            if (DevId == 0) return unchecked((int)0x80004005);

            var param = ParseList(Call<string>("GetLedSetting"));
            foreach (Dictionary<string, object> z in ParseList(zonesJson))
            {
                int index = Convert.ToInt32(z["index"]);
                int mode = Convert.ToInt32(z["mode"]);
                if (index < 0 || index >= param.Count || !AllowedModes.Contains(mode)) return unchecked((int)0x80070057);
                Dictionary<string, object> p = param[index];
                p["ledMode"] = mode;
                p["ledColor0"] = Convert.ToInt32(z["color"]) & 0xFFFFFF;
                p["spLv"] = Clamp(Convert.ToInt32(z["speed"]), 1, 9) - 1;
                p["briLv"] = Clamp(Convert.ToInt32(z["brightness"]), 1, 9) - 1;
            }

            Call<int>("Apply", ApplyStopHostEffects);
            _type.GetProperty("sInfo").SetValue(_board, _json.Serialize(param));
            int hr = Call<int>("Apply", ApplyAdvanced);
            if (hr == 0) Call("SaveSetting", 1);
            return hr;
        }

        public void Stop()
        {
            if (DevId != 0) Call<int>("Apply", ApplyStopHostEffects);
        }

        private List<Dictionary<string, object>> ParseList(string json)
        {
            if (string.IsNullOrWhiteSpace(json)) return new List<Dictionary<string, object>>();
            return ((object[])_json.DeserializeObject(json)).Cast<Dictionary<string, object>>().ToList();
        }

        private static int Clamp(int v, int lo, int hi) => Math.Max(lo, Math.Min(hi, v));
    }
}
