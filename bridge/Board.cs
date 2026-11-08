using System;
using System.Collections.Generic;
using System.Management;
using Microsoft.Win32;

namespace HelmBridge
{
    /// <summary>Board identity used for BIOS/driver lookups (same ACPI-WMI source as GCC's GBT_MB_Update).</summary>
    internal static class Board
    {
        public static Dictionary<string, object> FirmwareInfo()
        {
            var info = new Dictionary<string, object>();
            using (RegistryKey bios = Registry.LocalMachine.OpenSubKey(@"HARDWARE\DESCRIPTION\System\BIOS"))
            {
                info["model"] = bios?.GetValue("BaseBoardProduct") as string;
                info["biosVersion"] = bios?.GetValue("BIOSVersion") as string;
                info["biosDate"] = bios?.GetValue("BIOSReleaseDate") as string;
                info["systemVersion"] = bios?.GetValue("SystemVersion") as string;
                info["systemFamily"] = bios?.GetValue("SystemFamily") as string;
                info["manufacturer"] = bios?.GetValue("BaseBoardManufacturer") as string;
            }

            try
            {
                using (var gsa = new ManagementObject(@"root\WMI", @"GSA1_ACPIMethod.InstanceName='ACPI\PNP0C14\GSADEV0_0'", null))
                {
                    info["partNumber"] = Invoke(gsa, "GsaGetFWTagString");
                    string version = Invoke(gsa, "GsaGetFWVerString");
                    string model = Invoke(gsa, "GsaGetFWPrjString");
                    if (!string.IsNullOrWhiteSpace(version)) info["biosVersion"] = version;
                    if (!string.IsNullOrWhiteSpace(model)) info["model"] = model;
                    info["source"] = "gsa";
                }
            }
            catch (Exception ex) when (ex is ManagementException || ex is UnauthorizedAccessException || ex is System.Runtime.InteropServices.COMException)
            {
                info["partNumber"] = null;
                info["source"] = "registry";
                info["gsaError"] = ex.Message;
            }
            return info;
        }

        private static string Invoke(ManagementObject target, string method)
        {
            ManagementBaseObject result = target.InvokeMethod(method, null, null);
            return Convert.ToString(result?["Value"])?.Trim();
        }
    }
}
