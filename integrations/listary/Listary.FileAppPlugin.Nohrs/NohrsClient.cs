using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Net.Sockets;
using System.Runtime.InteropServices;
using System.Text;
using System.Web.Script.Serialization;

namespace Listary.FileAppPlugin.Nohrs
{
    /// <summary>
    /// Talks to a running Nohrs over the local channel it describes in
    /// %LOCALAPPDATA%\nohrs\ipc.json (see crates/nohrs-pages/src/ipc.rs): one JSON
    /// line out, one JSON line back, the per-run token in every request.
    /// </summary>
    internal static class NohrsClient
    {
        private static readonly JavaScriptSerializer Json = new JavaScriptSerializer();

        internal sealed class Endpoint
        {
            public int Port;
            public string Token;
            public int Pid;
            public long Hwnd;
            public string Exe;
        }

        private static string EndpointFile =>
            Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                "nohrs",
                "ipc.json");

        /// <summary>The endpoint the last Nohrs to start left behind, if any.</summary>
        internal static Endpoint ReadEndpoint()
        {
            try
            {
                var fields = Json.Deserialize<Dictionary<string, object>>(File.ReadAllText(EndpointFile));
                return new Endpoint
                {
                    Port = Convert.ToInt32(fields["port"]),
                    Token = (string)fields["token"],
                    Pid = Convert.ToInt32(fields["pid"]),
                    Hwnd = Convert.ToInt64(fields["hwnd"]),
                    Exe = fields.TryGetValue("exe", out var exe) ? exe as string : null,
                };
            }
            catch (Exception)
            {
                return null;
            }
        }

        /// <summary>
        /// Sends a request and returns the reply, or null when no Nohrs answered
        /// (not running, or the endpoint file is from an earlier run).
        /// </summary>
        internal static Dictionary<string, object> Send(Dictionary<string, object> request)
        {
            var endpoint = ReadEndpoint();
            if (endpoint == null)
            {
                return null;
            }
            try
            {
                using (var client = new TcpClient())
                {
                    if (!client.ConnectAsync("127.0.0.1", endpoint.Port).Wait(500))
                    {
                        return null;
                    }
                    client.ReceiveTimeout = 3000;
                    request["token"] = endpoint.Token;
                    // Nohrs brings its window forward for what it was asked to
                    // show; Windows lets it only if the foreground process
                    // (Listary, where this runs) allows it.
                    AllowSetForegroundWindow(endpoint.Pid);
                    using (var stream = client.GetStream())
                    using (var writer = new StreamWriter(stream, new UTF8Encoding(false)) { AutoFlush = true })
                    using (var reader = new StreamReader(stream, Encoding.UTF8))
                    {
                        writer.Write(Json.Serialize(request) + "\n");
                        var line = reader.ReadLine();
                        return line == null ? null : Json.Deserialize<Dictionary<string, object>>(line);
                    }
                }
            }
            catch (Exception)
            {
                return null;
            }
        }

        internal static bool IsOk(Dictionary<string, object> reply) =>
            reply != null && reply.TryGetValue("ok", out var ok) && ok is bool value && value;

        /// <summary>
        /// Shows a folder, or a file selected in its folder, in a new tab of the
        /// running Nohrs; starts Nohrs with the path when none is running.
        /// </summary>
        internal static bool Open(string path)
        {
            if (IsOk(Send(new Dictionary<string, object> { ["cmd"] = "open", ["path"] = path })))
            {
                return true;
            }
            var exe = ReadEndpoint()?.Exe;
            if (string.IsNullOrEmpty(exe) || !File.Exists(exe))
            {
                return false;
            }
            try
            {
                // A fresh instance takes its path from the command line. A
                // trailing backslash is doubled: on its own it would escape the
                // closing quote, and dropping it would turn C:\ into C:, the
                // current directory of drive C.
                var argument = path.EndsWith("\\") ? path + "\\" : path;
                Process.Start(new ProcessStartInfo(exe, "\"" + argument + "\"")
                {
                    UseShellExecute = false,
                    WorkingDirectory = Path.GetDirectoryName(exe),
                });
                return true;
            }
            catch (Exception)
            {
                return false;
            }
        }

        [DllImport("user32.dll")]
        private static extern bool AllowSetForegroundWindow(int processId);
    }
}
