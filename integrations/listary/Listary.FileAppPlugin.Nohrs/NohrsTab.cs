using System.Collections.Generic;
using System.Threading.Tasks;

namespace Listary.FileAppPlugin.Nohrs
{
    /// <summary>
    /// One Nohrs tab. The folder is the one it showed when Listary asked; the id
    /// is what jumping it elsewhere addresses, so a tab moved or renamed in the
    /// meantime is still the one that goes.
    /// </summary>
    public class NohrsTab : IFileTab, IGetFolder, IOpenFolder, IOpenFolderAndSelectFile
    {
        private readonly ulong _id;
        private readonly string _folder;

        public NohrsTab(ulong id, string folder)
        {
            _id = id;
            _folder = folder;
        }

        public Task<string> GetCurrentFolder() => Task.FromResult(_folder);

        public Task<bool> OpenFolder(string path) => Task.FromResult(Navigate(path));

        public Task<bool> OpenFolderAndSelectFile(string path) => Task.FromResult(Navigate(path));

        private bool Navigate(string path) =>
            NohrsClient.IsOk(NohrsClient.Send(new Dictionary<string, object>
            {
                ["cmd"] = "navigate",
                ["tab"] = _id,
                ["path"] = path,
            }));
    }
}
