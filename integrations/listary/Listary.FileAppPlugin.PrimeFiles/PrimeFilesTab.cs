using System.Collections.Generic;
using System.Threading.Tasks;

namespace Listary.FileAppPlugin.PrimeFiles
{
    /// <summary>
    /// One Prime Files tab. The folder is the one it showed when Listary asked; the id
    /// is what jumping it elsewhere addresses, so a tab moved or renamed in the
    /// meantime is still the one that goes.
    /// </summary>
    public class PrimeFilesTab : IFileTab, IGetFolder, IOpenFolder, IOpenFolderAndSelectFile
    {
        private readonly ulong _id;
        private readonly string _folder;

        public PrimeFilesTab(ulong id, string folder)
        {
            _id = id;
            _folder = folder;
        }

        public Task<string> GetCurrentFolder() => Task.FromResult(_folder);

        public Task<bool> OpenFolder(string path) => Task.FromResult(Navigate(path));

        public Task<bool> OpenFolderAndSelectFile(string path) => Task.FromResult(Navigate(path));

        private bool Navigate(string path) =>
            PrimeFilesClient.IsOk(PrimeFilesClient.Send(new Dictionary<string, object>
            {
                ["cmd"] = "navigate",
                ["tab"] = _id,
                ["path"] = path,
            }));
    }
}
