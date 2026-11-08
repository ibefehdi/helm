using System;
using System.Collections;
using System.Collections.Generic;
using System.Linq;
using System.Runtime.Remoting;
using System.Security.Principal;
using System.ServiceProcess;
using System.Web.Script.Serialization;

namespace HelmBridge
{
    /// <summary>
    /// Line-delimited JSON RPC over stdio. Requests: {"id":n,"cmd":"...","args":{...}}.
    /// Responses: {"id":n,"ok":true,"data":...} or {"id":n,"ok":false,"error":"...","code":"..."}.
    /// </summary>
    internal static class Host
    {
        private const string BridgeVersion = "1.0.0";

        private static readonly JavaScriptSerializer Json = new JavaScriptSerializer { MaxJsonLength = int.MaxValue };

        private static Engine s_engine;
        private static Overclock s_overclock;
        private static Rgb s_rgb;

        internal static int Run()
        {
            AppDomain.CurrentDomain.ProcessExit += (_, __) => ReleaseForcedFans();
            Console.CancelKeyPress += (_, __) => ReleaseForcedFans();

            string line;
            while ((line = Console.In.ReadLine()) != null)
            {
                if (line.Length == 0) continue;
                Respond(Handle(line));
            }

            // stdin closed: the parent is gone, so never leave a fan pinned at a forced duty cycle.
            ReleaseForcedFans();
            s_engine?.Dispose();
            s_overclock?.Dispose();
            s_rgb?.Dispose();
            return 0;
        }

        private static Dictionary<string, object> Handle(string line)
        {
            object id = null;
            try
            {
                var request = (Dictionary<string, object>)Json.DeserializeObject(line);
                request.TryGetValue("id", out id);
                string cmd = request.TryGetValue("cmd", out object c) ? c as string : null;
                var args = request.TryGetValue("args", out object a) ? a as Dictionary<string, object> : null;
                args = args ?? new Dictionary<string, object>();

                return Ok(id, Dispatch(cmd, args));
            }
            catch (BridgeException ex)
            {
                return Fail(id, ex.Code, ex.Message);
            }
            catch (RemotingException ex)
            {
                return Fail(id, "engine_unreachable", DescribeRemotingFailure(ex));
            }
            catch (UnauthorizedAccessException ex)
            {
                return Fail(id, "access_denied", ex.Message);
            }
            catch (Exception ex)
            {
                Exception inner = ex.InnerException ?? ex;
                if (inner is RemotingException rex) return Fail(id, "engine_unreachable", DescribeRemotingFailure(rex));
                string detail = Environment.GetEnvironmentVariable("HELM_DEBUG") == "1" ? "\n" + ex : "";
                return Fail(id, "error", inner.GetType().Name + ": " + inner.Message + detail);
            }
        }

        private static object Dispatch(string cmd, Dictionary<string, object> args)
        {
            switch (cmd)
            {
                case "hello":
                    return new Dictionary<string, object>
                    {
                        ["bridgeVersion"] = BridgeVersion,
                        ["admin"] = IsAdministrator(),
                        ["serviceState"] = ServiceState(),
                        ["engineDirs"] = Program.SearchDirs,
                    };
                case "connect":
                    EnsureEngine(forceReconnect: Arg.Bool(args, "force", false));
                    return s_engine.Describe();
                case "info":
                    return EnsureEngine().Describe();
                case "sensors":
                    return EnsureEngine().ReadSensors();
                case "fan.get":
                    return EnsureEngine().GetFan(Arg.Int(args, "index"));
                case "fan.getAll":
                    return EnsureEngine().GetAllFans();
                case "fan.set":
                {
                    bool verified = EnsureEngine().SetFan(Arg.Int(args, "index"), args);
                    Dictionary<string, object> fan = EnsureEngine().GetFan(Arg.Int(args, "index"));
                    fan["verified"] = verified;
                    return fan;
                }
                case "fan.force":
                    EnsureEngine().ForceFan(Arg.Int(args, "index"), Arg.Int(args, "percent"));
                    return null;
                case "fan.release":
                    EnsureEngine().ReleaseFans();
                    return null;
                case "board.fwinfo":
                    return Board.FirmwareInfo();
                case "oc.get":
                    return EnsureOverclock().Describe();
                case "oc.apply":
                {
                    var changes = ((IEnumerable)(args.TryGetValue("changes", out object raw) ? raw : throw new BridgeException("bad_args", "Missing argument: changes")))
                        .Cast<Dictionary<string, object>>()
                        .Select(c => new KeyValuePair<int, long>(Arg.Int(c, "appId"), Convert.ToInt64(c["value"])))
                        .ToList();
                    return EnsureOverclock().Apply(changes, Arg.Bool(args, "allowReboot", false));
                }
                case "oc.restore":
                    return EnsureOverclock().RestoreBoot();
                case "rgb.get":
                    return EnsureRgb().Describe();
                case "rgb.set":
                    if (!(args.TryGetValue("zones", out object zones) && zones is object[] list))
                        throw new BridgeException("bad_args", "Missing argument: zones");
                    return EnsureRgb().Set(list);
                default:
                    throw new BridgeException("unknown_command", "Unknown command: " + (cmd ?? "<null>"));
            }
        }

        private static Engine EnsureEngine(bool forceReconnect = false)
        {
            if (forceReconnect && s_engine != null)
            {
                s_engine.Dispose();
                s_engine = null;
            }
            if (s_engine != null) return s_engine;
            RequireEngineAccess();
            s_engine = new Engine();
            return s_engine;
        }

        private static Overclock EnsureOverclock()
        {
            if (s_overclock != null) return s_overclock;
            RequireEngineAccess();
            s_overclock = new Overclock();
            return s_overclock;
        }

        private static Rgb EnsureRgb()
        {
            if (s_rgb != null) return s_rgb;
            if (!IsAdministrator())
                throw new BridgeException("not_admin", "RGB control needs administrator rights.");
            s_rgb = new Rgb();
            return s_rgb;
        }

        private static void RequireEngineAccess()
        {
            if (!IsAdministrator())
                throw new BridgeException("not_admin", "The Gigabyte engine only accepts connections from an elevated (administrator) process.");
            string state = ServiceState();
            if (state != "running")
                throw new BridgeException("service_unavailable", "EasyTuneEngineService is " + state + ".");
        }

        private static void ReleaseForcedFans()
        {
            try { s_engine?.ReleaseFansIfForced(); } catch { }
        }

        private static bool IsAdministrator()
        {
            using (var identity = WindowsIdentity.GetCurrent())
                return new WindowsPrincipal(identity).IsInRole(WindowsBuiltInRole.Administrator);
        }

        private static string ServiceState()
        {
            try
            {
                using (var sc = new ServiceController("EasyTuneEngineService"))
                    return sc.Status.ToString().ToLowerInvariant();
            }
            catch (InvalidOperationException)
            {
                return "missing";
            }
        }

        private static string DescribeRemotingFailure(RemotingException ex)
        {
            if (ex.Message.IndexOf("denied", StringComparison.OrdinalIgnoreCase) >= 0)
                return "Access to the GigabyteEngine pipe was denied. Run Helm as administrator.";
            return ex.Message;
        }

        private static Dictionary<string, object> Ok(object id, object data) =>
            new Dictionary<string, object> { ["id"] = id, ["ok"] = true, ["data"] = data };

        private static Dictionary<string, object> Fail(object id, string code, string message) =>
            new Dictionary<string, object> { ["id"] = id, ["ok"] = false, ["code"] = code, ["error"] = message };

        private static void Respond(Dictionary<string, object> response)
        {
            Console.Out.WriteLine(Json.Serialize(Sanitize(response)));
            Console.Out.Flush();
        }

        // JavaScriptSerializer emits bare NaN/Infinity, which is not valid JSON.
        private static object Sanitize(object value)
        {
            switch (value)
            {
                case float f: return float.IsNaN(f) || float.IsInfinity(f) ? null : (object)f;
                case double d: return double.IsNaN(d) || double.IsInfinity(d) ? null : (object)d;
                case string _: return value;
                case IDictionary<string, object> dict: return dict.ToDictionary(kv => kv.Key, kv => Sanitize(kv.Value));
                case IEnumerable list: return list.Cast<object>().Select(Sanitize).ToList();
                default: return value;
            }
        }
    }

    internal sealed class BridgeException : Exception
    {
        public string Code { get; }

        public BridgeException(string code, string message) : base(message)
        {
            Code = code;
        }
    }

    internal static class Arg
    {
        public static int Int(Dictionary<string, object> args, string name)
        {
            if (!args.TryGetValue(name, out object v) || v == null)
                throw new BridgeException("bad_args", "Missing argument: " + name);
            return Convert.ToInt32(v);
        }

        public static int Int(Dictionary<string, object> args, string name, int fallback) =>
            args.TryGetValue(name, out object v) && v != null ? Convert.ToInt32(v) : fallback;

        public static bool Bool(Dictionary<string, object> args, string name, bool fallback) =>
            args.TryGetValue(name, out object v) && v != null ? Convert.ToBoolean(v) : fallback;

        public static string Str(Dictionary<string, object> args, string name, string fallback) =>
            args.TryGetValue(name, out object v) && v != null ? Convert.ToString(v) : fallback;
    }
}
