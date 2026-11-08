using System;
using System.Collections.Generic;
using System.IO;
using System.Reflection;
using System.Text;
using Microsoft.Win32;

namespace HelmBridge
{
    internal static class Program
    {
        internal const string DefaultEngineDir = @"C:\Program Files (x86)\GIGABYTE\EasyTuneEngineService";

        internal static readonly List<string> SearchDirs = new List<string>();

        private static int Main()
        {
            BuildSearchDirs();
            AppDomain.CurrentDomain.AssemblyResolve += ResolveGigabyteAssembly;
            Console.InputEncoding = new UTF8Encoding(false);
            Console.OutputEncoding = new UTF8Encoding(false);
            // Host must live in a separate method so the JIT doesn't touch Gigabyte types before the resolver is installed.
            return Host.Run();
        }

        private static void BuildSearchDirs()
        {
            AddSearchDir(Environment.GetEnvironmentVariable("HELM_GIGABYTE_DIR"));
            AddSearchDir(EngineDirFromServiceRegistration());
            AddSearchDir(DefaultEngineDir);
            AddSearchDir(@"C:\Program Files\GIGABYTE\Control Center\Lib\MBEasyTune");
        }

        private static void AddSearchDir(string dir)
        {
            if (string.IsNullOrWhiteSpace(dir)) return;
            dir = dir.TrimEnd('\\');
            if (!SearchDirs.Exists(d => string.Equals(d, dir, StringComparison.OrdinalIgnoreCase))) SearchDirs.Add(dir);
        }

        private static string EngineDirFromServiceRegistration()
        {
            try
            {
                using (RegistryKey key = Registry.LocalMachine.OpenSubKey(@"SYSTEM\CurrentControlSet\Services\EasyTuneEngineService"))
                {
                    string imagePath = key?.GetValue("ImagePath") as string;
                    if (string.IsNullOrWhiteSpace(imagePath)) return null;
                    imagePath = imagePath.Trim();
                    if (imagePath.StartsWith("\""))
                    {
                        int end = imagePath.IndexOf('"', 1);
                        imagePath = end > 0 ? imagePath.Substring(1, end - 1) : imagePath.Trim('"');
                    }
                    string dir = Path.GetDirectoryName(imagePath);
                    return Directory.Exists(dir) ? dir : null;
                }
            }
            catch
            {
                return null;
            }
        }

        private static Assembly ResolveGigabyteAssembly(object sender, ResolveEventArgs args)
        {
            string fileName = new AssemblyName(args.Name).Name + ".dll";
            foreach (string dir in SearchDirs)
            {
                string candidate = Path.Combine(dir, fileName);
                if (File.Exists(candidate)) return Assembly.LoadFrom(candidate);
            }
            return null;
        }
    }
}
