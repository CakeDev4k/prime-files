using System;
using System.Runtime.InteropServices;
using System.Threading.Tasks;

namespace Listary.FileAppPlugin.Nohrs
{
    /// <summary>
    /// Makes Nohrs a file manager Listary knows: its folders appear in Quick
    /// Switch and the opened-folders menu, Listary can send it to a folder, and
    /// it can be the app Listary opens folders in.
    /// </summary>
    public class NohrsPlugin : IFileAppPlugin, IOpenFolder, IOpenFolderAndSelectFile
    {
        public bool IsOpenedFolderProvider => true;

        public bool IsQuickSwitchTarget => true;

        public bool IsSharedAcrossApplications => false;

        public SearchBarType SearchBarType => SearchBarType.Floating;

        public Task<bool> Initialize(IFileAppPluginHost host) => Task.FromResult(true);

        public IFileWindow BindFileWindow(IntPtr hWnd)
        {
            // The Explorer window is the one Nohrs recorded. Its launcher window
            // belongs to the same process and is not a file window, so matching
            // on the process alone would bind that too. The process check keeps
            // a stale endpoint file from claiming a handle Windows has reused.
            var endpoint = NohrsClient.ReadEndpoint();
            if (endpoint == null || endpoint.Hwnd != hWnd.ToInt64())
            {
                return null;
            }
            GetWindowThreadProcessId(hWnd, out var processId);
            return processId == endpoint.Pid ? new NohrsWindow(hWnd) : null;
        }

        public Task<bool> OpenFolder(string path) => Task.FromResult(NohrsClient.Open(path));

        public Task<bool> OpenFolderAndSelectFile(string path) => Task.FromResult(NohrsClient.Open(path));

        [DllImport("user32.dll")]
        private static extern uint GetWindowThreadProcessId(IntPtr hWnd, out int processId);
    }
}
