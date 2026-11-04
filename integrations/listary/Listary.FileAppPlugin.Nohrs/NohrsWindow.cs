using System;
using System.Collections;
using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;

namespace Listary.FileAppPlugin.Nohrs
{
    /// <summary>The Nohrs Explorer window, whose tabs each show a folder.</summary>
    public class NohrsWindow : IFileWindow, IGetFileTabs, IOpenFolder, IOpenFolderAndSelectFile
    {
        public NohrsWindow(IntPtr handle)
        {
            Handle = handle;
        }

        public IntPtr Handle { get; }

        public Task<IFileTab> GetCurrentTab()
        {
            var reply = NohrsClient.Send(new Dictionary<string, object> { ["cmd"] = "current" });
            IFileTab tab = NohrsClient.IsOk(reply)
                ? new NohrsTab(Convert.ToUInt64(reply["tab"]), (string)reply["folder"])
                : null;
            return Task.FromResult(tab);
        }

        public Task<IEnumerable<IFileTab>> GetTabs()
        {
            var reply = NohrsClient.Send(new Dictionary<string, object> { ["cmd"] = "tabs" });
            IEnumerable<IFileTab> tabs = Enumerable.Empty<IFileTab>();
            if (NohrsClient.IsOk(reply) && reply["tabs"] is IEnumerable entries)
            {
                tabs = entries
                    .OfType<Dictionary<string, object>>()
                    .Select(entry => (IFileTab)new NohrsTab(Convert.ToUInt64(entry["tab"]), (string)entry["folder"]))
                    .ToList();
            }
            return Task.FromResult(tabs);
        }

        public Task<bool> OpenFolder(string path) => Task.FromResult(NohrsClient.Open(path));

        public Task<bool> OpenFolderAndSelectFile(string path) => Task.FromResult(NohrsClient.Open(path));
    }
}
