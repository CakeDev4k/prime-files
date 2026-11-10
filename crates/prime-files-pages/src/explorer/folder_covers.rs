//! A folder in the icons view drawn as the first picture or video inside it,
//! full size, instead of Windows' folder with a small preview tucked in
//! (issue #39, what FolderThumbnailFix does for Explorer). On with
//! `ui.folder_previews`.
//!
//! Which file stands for a folder is read off the UI thread, once per folder,
//! and kept; the picture itself is the file's ordinary thumbnail.

use gpui::{AppContext as _, Context, WeakEntity};
use std::collections::{HashMap, HashSet};

use super::ExplorerPane;

/// Files looked at in a folder before giving up: a folder of tens of
/// thousands is not read whole to find its cover.
const SCAN_LIMIT: usize = 5000;
/// Folders remembered before the memory starts over.
const KEPT: usize = 5000;

/// The kinds a cover is taken from: what the Shell makes thumbnails of.
const PICTURES: [&str; 13] = [
    "jpg", "jpeg", "png", "webp", "bmp", "gif", "heic", "tif", "tiff", "mp4", "mov", "mkv", "webm",
];

/// Which file stands for each folder, by path; `None` for one with nothing
/// to show.
#[derive(Default)]
pub(crate) struct FolderCovers {
    found: HashMap<String, Option<String>>,
    asked: HashSet<String>,
}

impl ExplorerPane {
    /// The file to draw `folder` as, once known; asks for it the first time.
    pub(crate) fn folder_cover(&mut self, folder: &str, cx: &mut Context<Self>) -> Option<String> {
        if let Some(found) = self.folder_covers.found.get(folder) {
            return found.clone();
        }
        if !self.folder_covers.asked.insert(folder.to_string()) {
            return None;
        }
        let wanted = folder.to_string();
        let read = cx.background_spawn({
            let folder = wanted.clone();
            async move { first_picture(std::path::Path::new(&folder)) }
        });
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let cover = read.await;
            this.update(cx, |page, cx| {
                let covers = &mut page.folder_covers;
                if covers.found.len() >= KEPT {
                    covers.found.clear();
                    covers.asked.clear();
                }
                covers.asked.remove(&wanted);
                covers.found.insert(wanted, cover.clone());
                if cover.is_some() {
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        None
    }

    /// Forgets the covers, so a folder whose pictures changed is read again
    /// (a refresh of the folder on screen).
    pub(crate) fn forget_folder_covers(&mut self) {
        self.folder_covers = FolderCovers::default();
    }
}

// The first picture or video in `folder` by name, as the folder lists it.
fn first_picture(folder: &std::path::Path) -> Option<String> {
    std::fs::read_dir(folder)
        .ok()?
        .flatten()
        .take(SCAN_LIMIT)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter(|entry| {
            entry
                .path()
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| PICTURES.contains(&extension.to_ascii_lowercase().as_str()))
        })
        .min_by_key(|entry| entry.file_name().to_string_lossy().to_lowercase())
        .map(|entry| entry.path().to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::first_picture;

    #[test]
    fn the_cover_is_the_first_picture_by_name() {
        let folder = std::env::temp_dir().join(format!("prime-files-covers-{}", std::process::id()));
        std::fs::create_dir_all(folder.join("sub.jpg")).ok();
        for name in ["notes.txt", "b.PNG", "a.mp4", "c.jpg"] {
            std::fs::write(folder.join(name), b"x").ok();
        }
        let cover = first_picture(&folder);
        std::fs::remove_dir_all(&folder).ok();
        assert_eq!(cover.as_deref().and_then(|path| path.rsplit(['\\', '/']).next()), Some("a.mp4"));
    }

    #[test]
    fn a_folder_without_pictures_has_no_cover() {
        let folder = std::env::temp_dir().join(format!("prime-files-covers-none-{}", std::process::id()));
        std::fs::create_dir_all(&folder).ok();
        std::fs::write(folder.join("readme.md"), b"x").ok();
        let cover = first_picture(&folder);
        std::fs::remove_dir_all(&folder).ok();
        assert_eq!(cover, None);
    }
}
