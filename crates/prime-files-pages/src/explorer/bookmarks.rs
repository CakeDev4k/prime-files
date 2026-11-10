//! What the sidebar keeps of its own, for the whole app: folders bookmarked
//! in groups (One Commander's favorites, File Pilot's bookmarks) along with
//! any other place of the sidebar put in them (This PC, the Recycle Bin), the
//! folders visited last, and which of the sidebar's groups are folded.
//! Windows' Quick access is the Shell's and stays in `places.rs`.

use std::sync::Arc;

use gpui::{App, AppContext as _, BorrowAppContext, Global, Task};
use prime_files_core::telemetry::LogErr;
use prime_files_store::{KvKey, KvStore, kv_key};
use serde::{Deserialize, Serialize};

use super::places::Place;

const BOOKMARKS_KEY: KvKey = kv_key!("explorer.sidebar");

// More than any style lists (`recent_count` goes to 50): the setting can
// grow without the history starting over.
const RECENT_KEPT: usize = 50;

/// A bookmarked folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Bookmark {
    pub path: String,
    /// The name shown instead of the folder's, as One Commander's "Change
    /// alias": the folder itself is not renamed.
    #[serde(default)]
    pub alias: Option<String>,
}

impl Bookmark {
    pub(crate) fn name(&self) -> String {
        if let Some(alias) = self.alias.as_ref().filter(|alias| !alias.trim().is_empty()) {
            return alias.clone();
        }
        folder_name(&self.path)
    }

    /// Whether this is a place of the Shell with no folder behind it (This
    /// PC, the Recycle Bin, a phone), kept by its parsing name.
    pub(crate) fn is_virtual(&self) -> bool {
        self.path.starts_with("::")
    }

    /// The bookmark as a place of the sidebar, opened and drawn as one.
    pub(crate) fn place(&self) -> Place {
        Place {
            name: self.name(),
            path: (!self.is_virtual()).then(|| self.path.clone()),
            parsing_name: self.path.clone(),
            pinned: false,
        }
    }
}

/// The last part of `path`, or the drive (`C:\`) when it is one.
pub(crate) fn folder_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

/// A named group of bookmarks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BookmarkGroup {
    pub id: u32,
    pub name: String,
    pub items: Vec<Bookmark>,
}

#[derive(Default, Serialize, Deserialize)]
struct Stored {
    groups: Vec<BookmarkGroup>,
    next_id: u32,
    recent: Vec<String>,
    /// The groups folded by a click on their name, by key (a section's
    /// spelling, or `bookmarks-<id>`). Kept across runs: File Pilot's
    /// reviewers disliked that its folds came back open after a restart.
    folded: Vec<String>,
}

#[derive(Default)]
pub(crate) struct Bookmarks {
    stored: Stored,
    store: Option<Arc<dyn KvStore>>,
    save: Option<Task<()>>,
}

impl Global for Bookmarks {}

fn same(left: &str, right: &str) -> bool {
    left.trim_end_matches(['\\', '/'])
        .eq_ignore_ascii_case(right.trim_end_matches(['\\', '/']))
}

impl Bookmarks {
    /// Loads what was kept and makes it the app's. A first start gets one
    /// empty group to drop folders on.
    pub(crate) fn install(store: Option<Arc<dyn KvStore>>, cx: &mut App) {
        if cx.has_global::<Self>() {
            return;
        }
        let stored = store
            .as_ref()
            .and_then(|store| store.get(&BOOKMARKS_KEY).log_err().flatten())
            .and_then(|bytes| serde_json::from_slice::<Stored>(&bytes).log_err());
        let mut bookmarks = Self {
            stored: stored.unwrap_or_default(),
            store,
            save: None,
        };
        if bookmarks.stored.groups.is_empty() {
            bookmarks.push_group(prime_files_core::t!("Marcadores").to_string());
        }
        cx.set_global(bookmarks);
    }

    fn push_group(&mut self, name: String) -> u32 {
        let id = self.stored.next_id;
        self.stored.next_id += 1;
        self.stored.groups.push(BookmarkGroup {
            id,
            name,
            items: Vec::new(),
        });
        id
    }

    /// Every group, in the order shown.
    pub(crate) fn groups(cx: &App) -> Vec<BookmarkGroup> {
        cx.try_global::<Self>()
            .map(|bookmarks| bookmarks.stored.groups.clone())
            .unwrap_or_default()
    }

    pub(crate) fn group(id: u32, cx: &App) -> Option<BookmarkGroup> {
        cx.try_global::<Self>()?
            .stored
            .groups
            .iter()
            .find(|group| group.id == id)
            .cloned()
    }

    /// Adds each folder of `paths` to group `id`; files and folders already
    /// there are skipped. How many were added.
    #[allow(clippy::disallowed_methods)]
    pub(crate) fn add(id: u32, paths: &[String], cx: &mut App) -> usize {
        let folders: Vec<String> = paths
            .iter()
            .filter(|path| std::path::Path::new(path).is_dir())
            .cloned()
            .collect();
        let mut added = 0;
        Self::change(cx, |bookmarks| {
            if let Some(group) = bookmarks
                .stored
                .groups
                .iter_mut()
                .find(|group| group.id == id)
            {
                for path in folders {
                    if !group.items.iter().any(|item| same(&item.path, &path)) {
                        group.items.push(Bookmark { path, alias: None });
                        added += 1;
                    }
                }
            }
        });
        added
    }

    /// Puts any place of the sidebar in group `id`, at `at` or at the end:
    /// a folder, a drive, This PC, the Recycle Bin, a phone. It keeps the
    /// name the sidebar gave it ("Área de Trabalho", not "Desktop"). Whether
    /// it went in; one already in the group stays where it is.
    pub(crate) fn add_place(id: u32, place: &Place, at: Option<usize>, cx: &mut App) -> bool {
        let path = place.path.clone().unwrap_or_else(|| place.parsing_name.clone());
        let mut bookmark = Bookmark { path, alias: None };
        if bookmark.name() != place.name {
            bookmark.alias = Some(place.name.clone());
        }
        let mut added = false;
        Self::change(cx, |bookmarks| {
            if let Some(group) = bookmarks.stored.groups.iter_mut().find(|group| group.id == id)
                && !group.items.iter().any(|item| same(&item.path, &bookmark.path))
            {
                let at = at.unwrap_or(group.items.len()).min(group.items.len());
                group.items.insert(at, bookmark);
                added = true;
            }
        });
        added
    }

    /// Moves the bookmark at `from_index` of `from_group` to `to_index` of
    /// `to_group`, the place it was dropped on. Into a group that has it
    /// already, it only leaves the first.
    pub(crate) fn move_item_to(from_group: u32, from_index: usize, to_group: u32, to_index: usize, cx: &mut App) {
        Self::change(cx, |bookmarks| {
            let groups = &mut bookmarks.stored.groups;
            let Some(source) = groups.iter_mut().find(|group| group.id == from_group) else {
                return;
            };
            if from_index >= source.items.len() {
                return;
            }
            let item = source.items.remove(from_index);
            // Taken out above the place it goes to, the rows under it moved up.
            let to_index = if from_group == to_group && from_index < to_index { to_index - 1 } else { to_index };
            if let Some(target) = groups.iter_mut().find(|group| group.id == to_group)
                && !target.items.iter().any(|kept| same(&kept.path, &item.path))
            {
                let at = to_index.min(target.items.len());
                target.items.insert(at, item);
            }
        });
    }

    /// Moves group `id` to where group `before` is, as a group dragged onto
    /// another's name.
    pub(crate) fn move_group_to(id: u32, before: u32, cx: &mut App) {
        if id == before {
            return;
        }
        Self::change(cx, |bookmarks| {
            let groups = &mut bookmarks.stored.groups;
            let (Some(from), Some(to)) = (
                groups.iter().position(|group| group.id == id),
                groups.iter().position(|group| group.id == before),
            ) else {
                return;
            };
            let group = groups.remove(from);
            groups.insert(to.min(groups.len()), group);
        });
    }

    pub(crate) fn remove(id: u32, path: &str, cx: &mut App) {
        Self::change(cx, |bookmarks| {
            if let Some(group) = bookmarks
                .stored
                .groups
                .iter_mut()
                .find(|group| group.id == id)
            {
                group.items.retain(|item| !same(&item.path, path));
            }
        });
    }

    /// Names the bookmark at `index` of group `id`; an empty name gives the
    /// folder's back.
    pub(crate) fn set_alias(id: u32, index: usize, alias: String, cx: &mut App) {
        Self::change(cx, |bookmarks| {
            if let Some(item) = bookmarks
                .stored
                .groups
                .iter_mut()
                .find(|group| group.id == id)
                .and_then(|group| group.items.get_mut(index))
            {
                let alias = alias.trim().to_string();
                item.alias =
                    (!alias.is_empty() && alias != folder_name(&item.path)).then_some(alias);
            }
        });
    }

    /// Moves the bookmark of `path` in group `id` one place up or down.
    pub(crate) fn move_item(id: u32, path: &str, up: bool, cx: &mut App) {
        Self::change(cx, |bookmarks| {
            if let Some(group) = bookmarks
                .stored
                .groups
                .iter_mut()
                .find(|group| group.id == id)
                && let Some(index) = group.items.iter().position(|item| same(&item.path, path))
            {
                swap_toward(&mut group.items, index, up);
            }
        });
    }

    /// A new group, at the end. Its id.
    pub(crate) fn create_group(name: String, cx: &mut App) -> u32 {
        let mut id = 0;
        Self::change(cx, |bookmarks| id = bookmarks.push_group(name));
        id
    }

    pub(crate) fn rename_group(id: u32, name: String, cx: &mut App) {
        let name = name.trim().to_string();
        if name.is_empty() {
            return;
        }
        Self::change(cx, |bookmarks| {
            if let Some(group) = bookmarks
                .stored
                .groups
                .iter_mut()
                .find(|group| group.id == id)
            {
                group.name = name;
            }
        });
    }

    pub(crate) fn delete_group(id: u32, cx: &mut App) {
        Self::change(cx, |bookmarks| {
            bookmarks.stored.groups.retain(|group| group.id != id)
        });
    }

    pub(crate) fn move_group(id: u32, up: bool, cx: &mut App) {
        Self::change(cx, |bookmarks| {
            if let Some(index) = bookmarks
                .stored
                .groups
                .iter()
                .position(|group| group.id == id)
            {
                swap_toward(&mut bookmarks.stored.groups, index, up);
            }
        });
    }

    /// The folders opened last, newest first.
    pub(crate) fn recent(cx: &App) -> Vec<String> {
        cx.try_global::<Self>()
            .map(|bookmarks| bookmarks.stored.recent.clone())
            .unwrap_or_default()
    }

    /// `path` was opened: it goes to the top of the recent folders, as One
    /// Commander's history moves a folder opened again. No window is
    /// redrawn for it: the pane that opened it is redrawing already.
    pub(crate) fn visit(path: &str, cx: &mut App) {
        if !cx.has_global::<Self>() {
            return;
        }
        let path = path.to_string();
        cx.update_global(|bookmarks: &mut Self, cx| {
            let recent = &mut bookmarks.stored.recent;
            if recent.first().is_some_and(|first| same(first, &path)) {
                return;
            }
            recent.retain(|kept| !same(kept, &path));
            recent.insert(0, path);
            recent.truncate(RECENT_KEPT);
            bookmarks.persist(cx);
        });
    }

    pub(crate) fn forget_recent(path: &str, cx: &mut App) {
        Self::change(cx, |bookmarks| {
            bookmarks.stored.recent.retain(|kept| !same(kept, path))
        });
    }

    pub(crate) fn clear_recent(cx: &mut App) {
        Self::change(cx, |bookmarks| bookmarks.stored.recent.clear());
    }

    pub(crate) fn is_folded(key: &str, cx: &App) -> bool {
        cx.try_global::<Self>()
            .is_some_and(|bookmarks| bookmarks.stored.folded.iter().any(|folded| folded == key))
    }

    pub(crate) fn toggle_folded(key: &str, cx: &mut App) {
        let key = key.to_string();
        Self::change(cx, |bookmarks| {
            let folded = &mut bookmarks.stored.folded;
            if let Some(index) = folded.iter().position(|kept| *kept == key) {
                folded.remove(index);
            } else {
                folded.push(key);
            }
        });
    }

    fn persist(&mut self, cx: &mut App) {
        let (Some(store), Ok(bytes)) = (self.store.clone(), serde_json::to_vec(&self.stored))
        else {
            return;
        };
        self.save = Some(cx.background_spawn(async move {
            store.put(&BOOKMARKS_KEY, &bytes).log_err();
        }));
    }

    fn change(cx: &mut App, edit: impl FnOnce(&mut Self)) {
        if !cx.has_global::<Self>() {
            return;
        }
        cx.update_global(|bookmarks: &mut Self, cx| {
            edit(bookmarks);
            bookmarks.persist(cx);
        });
        cx.refresh_windows();
    }
}

fn swap_toward<T>(items: &mut [T], index: usize, up: bool) {
    let other = if up {
        index.checked_sub(1)
    } else {
        Some(index + 1)
    };
    if let Some(other) = other.filter(|other| *other < items.len()) {
        items.swap(index, other);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alias_falls_back_to_the_folder_name() {
        let mut bookmark = Bookmark {
            path: r"D:\Projetos\Cliente".to_string(),
            alias: None,
        };
        assert_eq!(bookmark.name(), "Cliente");
        bookmark.alias = Some("  ".to_string());
        assert_eq!(bookmark.name(), "Cliente");
        bookmark.alias = Some("Trabalho".to_string());
        assert_eq!(bookmark.name(), "Trabalho");
        assert_eq!(folder_name(r"C:\"), r"C:\");
    }

    #[test]
    fn swap_stays_inside_the_list() {
        let mut items = vec![1, 2, 3];
        swap_toward(&mut items, 0, true);
        assert_eq!(items, vec![1, 2, 3]);
        swap_toward(&mut items, 2, false);
        assert_eq!(items, vec![1, 2, 3]);
        swap_toward(&mut items, 1, true);
        assert_eq!(items, vec![2, 1, 3]);
    }

    // One Commander's groups: any place goes in, keeping the sidebar's
    // name; a row dragged to another group or row lands there; a group
    // dragged onto another's name takes its place.
    #[gpui::test]
    fn groups_hold_any_place_and_rearrange_by_drag(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            Bookmarks::install(None, cx);
            let first = Bookmarks::groups(cx)[0].id;
            let second = Bookmarks::create_group("Projetos".into(), cx);
            let this_pc = Place {
                name: "Este Computador".into(),
                path: None,
                parsing_name: "::{20D04FE0-3AEA-1069-A2D8-08002B30309D}".into(),
                pinned: false,
            };
            let desktop = Place {
                name: "Área de Trabalho".into(),
                path: Some(r"C:\Users\x\Desktop".into()),
                parsing_name: r"C:\Users\x\Desktop".into(),
                pinned: false,
            };
            assert!(Bookmarks::add_place(second, &this_pc, None, cx));
            assert!(Bookmarks::add_place(second, &desktop, Some(0), cx));
            assert!(!Bookmarks::add_place(second, &desktop, None, cx), "once per group");
            let group = Bookmarks::group(second, cx).expect("group");
            assert_eq!(group.items[0].name(), "Área de Trabalho");
            assert_eq!(group.items[1].place(), this_pc);

            // Desktop to the end of its own group, then into the first one.
            Bookmarks::move_item_to(second, 0, second, 2, cx);
            assert_eq!(Bookmarks::group(second, cx).expect("group").items[1].place().name, "Área de Trabalho");
            Bookmarks::move_item_to(second, 1, first, 0, cx);
            assert_eq!(Bookmarks::group(first, cx).expect("group").items.len(), 1);
            assert_eq!(Bookmarks::group(second, cx).expect("group").items.len(), 1);

            Bookmarks::move_group_to(second, first, cx);
            let order: Vec<u32> = Bookmarks::groups(cx).iter().map(|group| group.id).collect();
            assert_eq!(order, vec![second, first]);
        });
    }

    #[gpui::test]
    fn recent_moves_a_folder_opened_again_to_the_top(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            Bookmarks::install(None, cx);
            Bookmarks::visit(r"C:\a", cx);
            Bookmarks::visit(r"C:\b", cx);
            Bookmarks::visit(r"c:\A\", cx);
            assert_eq!(
                Bookmarks::recent(cx),
                vec![r"c:\A\".to_string(), r"C:\b".to_string()]
            );
            Bookmarks::toggle_folded("recent", cx);
            assert!(Bookmarks::is_folded("recent", cx));
            Bookmarks::toggle_folded("recent", cx);
            assert!(!Bookmarks::is_folded("recent", cx));
            assert_eq!(Bookmarks::groups(cx).len(), 1);
        });
    }
}
