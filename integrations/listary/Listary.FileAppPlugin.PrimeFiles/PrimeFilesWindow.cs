using System;
using System.Collections;
using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;

namespace Listary.FileAppPlugin.PrimeFiles
{
    /// <summary>The Prime Files Explorer window, whose tabs each show a folder.</summary>
    public class PrimeFilesWindow : IFileWindow, IGetFileTabs, IOpenFolder, IOpenFolderAndSelectFile
    {
        public PrimeFilesWindow(IntPtr handle)
        {
            Handle = handle;
        }

        public IntPtr Handle { get; }

        public Task<IFileTab> GetCurrentTab()
        {
            var reply = PrimeFilesClient.Send(new Dictionary<string, object> { ["cmd"] = "current" });
            IFileTab tab = PrimeFilesClient.IsOk(reply)
                ? new PrimeFilesTab(Convert.ToUInt64(reply["tab"]), (string)reply["folder"])
                : null;
            return Task.FromResult(tab);
        }

        public Task<IEnumerable<IFileTab>> GetTabs()
        {
            var reply = PrimeFilesClient.Send(new Dictionary<string, object> { ["cmd"] = "tabs" });
            IEnumerable<IFileTab> tabs = Enumerable.Empty<IFileTab>();
            if (PrimeFilesClient.IsOk(reply) && reply["tabs"] is IEnumerable entries)
            {
                tabs = entries
                    .OfType<Dictionary<string, object>>()
                    .Select(entry => (IFileTab)new PrimeFilesTab(Convert.ToUInt64(entry["tab"]), (string)entry["folder"]))
                    .ToList();
            }
            return Task.FromResult(tabs);
        }

        public Task<bool> OpenFolder(string path) => Task.FromResult(PrimeFilesClient.Open(path));

        public Task<bool> OpenFolderAndSelectFile(string path) => Task.FromResult(PrimeFilesClient.Open(path));
    }
}
