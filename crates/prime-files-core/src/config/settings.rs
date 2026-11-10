//! User configuration: the P1 minimal `config.toml` schema, its 4-layer
//! override/merge strategy, lenient validation, JSON Schema generation, and the
//! on-disk template.
//!
//! Parsing is deliberately forward-compatible: unknown keys and out-of-range
//! values are downgraded to warnings rather than hard errors so a single typo
//! never prevents the app from starting. The only fatal case is TOML that does
//! not parse at all, which the caller surfaces in the UI status bar while
//! falling back to [`Config::default`].

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// Schema version understood by this build. Bumped only when the on-disk
/// schema changes in a way that requires migration.
pub const CURRENT_SCHEMA_VERSION: u64 = 1;

/// URL advertised in the `#:schema` header of `config.toml` so TOML-aware
/// editors can offer completion and validation.
pub const SCHEMA_URL: &str =
    "https://raw.githubusercontent.com/CakeDev4k/prime-files/main/docs/config.schema.json";

/// The fully merged, validated configuration.
///
/// Models the P1 surface (`theme` / `ui`) plus the P2 sections: `[indexing]`,
/// `[search]`, `[launcher]`, and `[diagnostics]`. `[keybindings]` and
/// `[plugins]` are reserved here as forward-compatible drafts — their types are
/// defined and parsed leniently so files can adopt them now, but the values do
/// not drive behaviour yet (keybindings land in P3, plugins in P4; see
/// `docs/config.md` §2/§5).
///
/// Only `theme`, `ui`, and `diagnostics` are consumed at runtime today. The
/// remaining sections (`keybindings`, `plugins`, `indexing`, `search`,
/// `launcher`) are accepted and validated now so files and editor completion can
/// adopt them, but editing them has no effect until the matching subsystem is
/// wired in a later phase — see the per-section notes and `docs/config.md` §5.
///
/// Every field is `#[serde(default)]`: the loader starts from
/// [`Config::default`] and overlays only the keys present in the file, so a
/// missing section is filled with its default rather than rejected. The
/// generated schema therefore has no `required` properties, matching the
/// loader (an empty `config.toml` is valid).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Config {
    /// On-disk schema version. Must be [`CURRENT_SCHEMA_VERSION`] for this build.
    pub schema_version: u64,
    /// Appearance settings.
    pub theme: Theme,
    /// Explorer / view settings.
    pub ui: Ui,
    /// Split-view and pane settings.
    pub explorer: Explorer,
    /// Compact mode: the tab row hidden until the pointer reaches it.
    pub compact: Compact,
    /// Where the sidebar, preview and terminal dock, and the tabs' layout.
    pub layout: Layout,
    /// What the sidebar lists and how it looks.
    pub sidebar: Sidebar,
    /// What a file's right-click menu shows, and in which order.
    pub menu: Menu,
    /// Automatic updates from the project's releases.
    pub updates: Updates,
    /// Keybinding overrides (draft; full support in P3). Empty means the
    /// built-in keymap is used.
    pub keybindings: Keybindings,
    /// Plugin selection (reserved; full support in P4).
    pub plugins: Plugins,
    /// Filesystem indexing strategy and exclusions.
    pub indexing: Indexing,
    /// Search backend selection.
    pub search: Search,
    /// Launcher window settings.
    pub launcher: Launcher,
    /// Diagnostics / performance-logging settings.
    pub diagnostics: Diagnostics,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            theme: Theme::default(),
            ui: Ui::default(),
            explorer: Explorer::default(),
            compact: Compact::default(),
            layout: Layout::default(),
            sidebar: Sidebar::default(),
            menu: Menu::default(),
            updates: Updates::default(),
            keybindings: Keybindings::default(),
            plugins: Plugins::default(),
            indexing: Indexing::default(),
            search: Search::default(),
            launcher: Launcher::default(),
            diagnostics: Diagnostics::default(),
        }
    }
}

/// Appearance settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Theme {
    /// Light/dark selection, or following the OS.
    pub mode: ThemeMode,
    /// Accent colour: one of the named colours the settings page offers
    /// (`orange`, `blue`, `teal`, `green`, `purple`, `pink`, `red`) or a
    /// `#rrggbb` hex string. Anything else falls back to orange.
    pub accent: String,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            mode: ThemeMode::Dark,
            accent: "orange".to_string(),
        }
    }
}

/// Explorer / view settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Ui {
    /// Interface language: `"auto"` follows the Windows regional format
    /// (then the country), or one of `"pt-BR"`, `"en"`, `"es"`, `"fr"`, `"de"`,
    /// `"it"`, `"ru"`, `"ja"`, `"zh-CN"`, `"ar"`, `"hu"`.
    pub language: String,
    /// Column the listing is sorted by on load.
    pub default_sort: SortOrder,
    /// Whether that default runs from Z to A, largest or newest first.
    pub default_sort_descending: bool,
    /// Whether dotfiles are shown in listings.
    pub show_hidden: bool,
    /// Identifier of the icon pack used to render entries.
    pub icon_pack: String,
    /// Most lines a name takes under an icon in the icons view, the last
    /// one ending in "…" when the name needs more; 0 = no limit.
    pub name_lines: u32,
    /// How smoothly the mouse wheel scrolls the listing, 0..=100: 0 moves
    /// in steps, 100 glides for longest.
    pub scroll_smoothness: u32,
    /// Whether stepping past one end of a listing continues from the other.
    pub infinite_scroll: bool,
    /// Whether each tab shows a folder icon before its name.
    pub tab_icons: bool,
    /// Whether a letter typed on the listing jumps to the first item whose
    /// name starts with it, as in Explorer, instead of filtering.
    pub type_to_select: bool,
    /// Whether resting the pointer on an item shows its details (type,
    /// size, dimensions, length), as Explorer's pop-up descriptions do.
    pub info_tips: bool,
    /// Whether a folder in the icons view is drawn as the first picture or
    /// video inside it, full size, instead of Windows' folder icon.
    pub folder_previews: bool,
    /// Whether a row of buttons for the common file commands sits under the
    /// path, as Windows 11's Explorer has.
    pub command_bar: bool,
    /// Whether sound files show their waveform instead of their icon in the
    /// icons view.
    pub audio_waveforms: bool,
    /// Whether a video or sound starts playing as soon as the preview opens it.
    pub preview_autoplay: bool,
    /// Whether the welcome screen was gone through; false shows it on start.
    pub welcome_done: bool,
    /// Whether resting the mouse on a sound file in the listing plays it.
    pub hover_audio: bool,
    /// How long the mouse rests on a sound file before it plays, in ms.
    pub hover_audio_delay_ms: u32,
    /// Folder a new tab starts in (Ctrl+T and the `+`). Empty follows the
    /// home folder. A folder that no longer exists falls back to home.
    pub new_tab_path: String,
    /// What Ctrl+T and the `+` open first: `box` (the default, the box that
    /// asks) or `folder` (a tab at `new_tab_path` at once). Shift+click on
    /// the `+`, and Ctrl+Shift+T, always open the box.
    pub new_tab_mode: NewTabMode,
}

impl Default for Ui {
    fn default() -> Self {
        Self {
            language: "auto".to_string(),
            default_sort: SortOrder::Name,
            default_sort_descending: false,
            show_hidden: false,
            icon_pack: "default".to_string(),
            name_lines: 0,
            scroll_smoothness: 50,
            infinite_scroll: false,
            tab_icons: false,
            type_to_select: false,
            info_tips: true,
            folder_previews: false,
            command_bar: false,
            audio_waveforms: false,
            preview_autoplay: false,
            welcome_done: false,
            hover_audio: true,
            hover_audio_delay_ms: 400,
            new_tab_path: String::new(),
            new_tab_mode: NewTabMode::Box,
        }
    }
}

/// Split-view and pane settings (see `docs/explorer-essentials.md` §3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Explorer {
    /// Orientation a fresh split uses unless overridden by the split shortcut.
    pub split_direction: SplitDirection,
    /// When enabled, navigating in one pane mirrors the path into the others.
    pub synced_panes: bool,
    /// When enabled, the previous session's tabs are restored on restart (§4).
    pub restore_tabs: bool,
    /// Whether "show in folder" from other programs (Premiere's Reveal in
    /// Explorer, a browser's Show in folder) opens here instead of Explorer.
    pub reveal_from_other_apps: bool,
    /// Whether a tab opened with the middle button stays behind the current
    /// one, as in a browser (issue #6). Off by default since 07/11/2026: the
    /// owner wants to land on the tab just opened. "Open in new tab" in the
    /// menu always shows the tab it opens.
    pub background_tabs: bool,
    /// Where a folder another program sends (Premiere's Reveal, a browser's
    /// Show in folder, the Desktop, Win+E) opens: a new tab of the window
    /// used last, the tab on screen, or a window of its own. RML asked to
    /// choose (07/11/2026): a new tab behind, then torn off into a window,
    /// was two steps for what they wanted in one.
    pub open_from_other_apps: OpenFromOtherApps,
}

impl Default for Explorer {
    fn default() -> Self {
        Self {
            split_direction: SplitDirection::Vertical,
            synced_panes: false,
            restore_tabs: true,
            reveal_from_other_apps: true,
            background_tabs: false,
            open_from_other_apps: OpenFromOtherApps::NewTab,
        }
    }
}

/// See [`Explorer::open_from_other_apps`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum OpenFromOtherApps {
    /// A new tab of the window used last, as browsers do with links.
    #[default]
    NewTab,
    /// The tab on screen goes to the folder.
    CurrentTab,
    /// A window of its own, with just that folder.
    NewWindow,
}

impl OpenFromOtherApps {
    /// Parse the `config.toml` spelling (`new-tab`/`current-tab`/`new-window`).
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "new-tab" => Some(Self::NewTab),
            "current-tab" => Some(Self::CurrentTab),
            "new-window" => Some(Self::NewWindow),
            _ => None,
        }
    }
}

/// A file's right-click menu, as the person arranged it. Ids the build does
/// not know are dropped when the menu is drawn, so an older build reading a
/// newer file still shows a menu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Menu {
    /// The icons in the row on top, in order: `cut`, `copy`, `copy_path`,
    /// `rename`, `delete`, `new_tab`, `properties`.
    pub quick: Vec<String>,
    /// The entries under the row, in order: `new_tab`, `copy_path`, `tags`,
    /// `windows` (every entry Windows and its programs add), `more_options`,
    /// `customize`. Leaving `windows` out gives a short menu of Prime Files'
    /// own entries.
    pub items: Vec<String>,
    /// Windows entries hidden by their label, as the menu shows it.
    pub hidden: Vec<String>,
    /// The right click opens the classic Windows menu (Windows 10's, with
    /// Rename, Copy, Paste written out) straight away, instead of Prime
    /// Files' menu with "Show more options" at the bottom.
    pub classic: bool,
}

/// The icons the menu's row can hold, in the order of a fresh config.
pub const MENU_QUICK_DEFAULT: &[&str] = &["cut", "copy", "copy_path", "rename", "delete"];
/// The menu's entries, in the order of a fresh config.
pub const MENU_ITEMS_DEFAULT: &[&str] = &["new_tab", "windows", "copy_path", "tags", "more_options", "customize"];
/// Every id `quick` may hold.
pub const MENU_QUICK_ALL: &[&str] = &["cut", "copy", "copy_path", "rename", "delete", "new_tab", "properties"];
/// Every id `items` may hold.
pub const MENU_ITEMS_ALL: &[&str] = &["new_tab", "windows", "copy_path", "tags", "more_options", "customize", "properties"];

impl Default for Menu {
    fn default() -> Self {
        Self {
            quick: MENU_QUICK_DEFAULT.iter().map(|id| id.to_string()).collect(),
            items: MENU_ITEMS_DEFAULT.iter().map(|id| id.to_string()).collect(),
            hidden: Vec::new(),
            classic: false,
        }
    }
}

/// Compact mode, as in Zen Browser: the chosen bars stay hidden and slide in
/// while the pointer rests at their edge of the window, leaving the listing
/// alone on screen. Off by default, so the window keeps its full chrome until
/// someone asks for less.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Compact {
    /// Whether the chosen bars hide.
    pub enabled: bool,
    /// The tab row (top edge).
    pub hide_tabs: bool,
    /// The address bar: back, forward, path, refresh, view buttons (top
    /// edge, under the tabs).
    pub hide_header: bool,
    /// The sidebar with the places (left edge).
    pub hide_sidebar: bool,
    /// The preview (right edge).
    pub hide_preview: bool,
    /// The bottom bar: filter, counts, zoom (bottom edge).
    pub hide_bottom_bar: bool,
    /// When shown, the bars float over the listing (`true`), so it does not
    /// move, or push it aside (`false`), so nothing of it is covered.
    pub overlay: bool,
    /// How long the pointer rests at an edge before its bars come, in ms.
    pub reveal_delay_ms: u32,
    /// How long the bars stay after the pointer leaves them, in ms.
    pub hide_delay_ms: u32,
    /// Length of the slide in and out, in ms; 0 = no animation.
    pub animation_ms: u32,
    /// Depth of the strip along each edge that calls its bars, in px.
    pub edge_px: u32,
    /// Width of the frame around the listing, which then sits in a card with
    /// rounded corners, as Zen's compact mode does; 0 = no frame.
    pub frame_px: u32,
}

impl Default for Compact {
    fn default() -> Self {
        Self {
            enabled: false,
            hide_tabs: true,
            hide_header: true,
            hide_sidebar: true,
            hide_preview: true,
            hide_bottom_bar: true,
            overlay: true,
            reveal_delay_ms: 100,
            hide_delay_ms: 400,
            animation_ms: 220,
            edge_px: 8,
            frame_px: 8,
        }
    }
}

/// Where the window's panels sit and how the tabs are laid out. Each panel
/// docks to one side of what it accompanies: the sidebar and the preview to a
/// side of the listing, the terminal to a side of the panes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Layout {
    /// The side the sidebar with the places docks to.
    pub sidebar: DockSide,
    /// The side the preview docks to.
    pub preview: DockSide,
    /// The side the terminal docks to.
    pub terminal: DockSide,
    /// Tabs in a column down the left of the window, as in Zen Browser,
    /// instead of a row along the top.
    pub vertical_tabs: bool,
    /// A grip in the middle of each panel's edge that carries the panel to
    /// another side. Off by default: dragging it was not obvious, and the
    /// sides are also chosen here and from the bubbles of the listing's edges.
    pub move_grips: bool,
    /// The tags' group in the sidebar. Off hides it; the tags stay on the
    /// files and in their menu.
    pub sidebar_tags: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            sidebar: DockSide::Left,
            preview: DockSide::Right,
            terminal: DockSide::Bottom,
            vertical_tabs: false,
            move_grips: false,
            sidebar_tags: true,
        }
    }
}

/// A side of the area a panel docks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DockSide {
    /// A column on the left.
    Left,
    /// A column on the right.
    Right,
    /// A band along the top.
    Top,
    /// A band along the bottom.
    Bottom,
}

impl DockSide {
    /// Parse the `config.toml` spelling (`left`/`right`/`top`/`bottom`).
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "top" => Some(Self::Top),
            "bottom" => Some(Self::Bottom),
            _ => None,
        }
    }

    /// The `config.toml` spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Top => "top",
            Self::Bottom => "bottom",
        }
    }

    /// Whether a panel on this side is a column (left or right), sized by
    /// its width, rather than a band sized by its height.
    pub fn is_column(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
}

/// The sidebar: a style that copies another explorer's, and the parts of it
/// that can be changed one by one. A part left out takes the style's value,
/// so changing the style changes everything not set by hand.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Sidebar {
    /// Whose sidebar to copy.
    pub style: SidebarStyle,
    /// The groups shown, top to bottom.
    pub sections: Option<Vec<SidebarSection>>,
    /// Drives open into their folders, as a tree; off, they are links.
    pub tree: Option<bool>,
    /// Lines down each level of the tree.
    pub tree_lines: Option<bool>,
    /// The tree opens by itself down to the folder on screen, as Explorer's
    /// "Expand to open folder".
    pub follow_folder: Option<bool>,
    /// Every group under its name, which a click folds; off, groups are
    /// apart by a line.
    pub headers: Option<bool>,
    /// How full each drive is: a bar beside its name, under it, or none.
    pub drive_usage: Option<DriveUsage>,
    /// How a drive is named.
    pub drive_label: Option<DriveLabel>,
    /// How tall the rows are.
    pub density: Option<SidebarDensity>,
    /// A narrow strip of icons, the names in their tooltips.
    pub icons_only: Option<bool>,
    /// How many folders the recent group lists, 1..=50.
    pub recent_count: Option<u32>,
    /// Each group in a card of its own, as One Commander's.
    pub cards: Option<bool>,
}

/// Whose sidebar the style copies.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SidebarStyle {
    /// Windows Explorer: Quick access, then This PC as a tree.
    #[default]
    Explorer,
    /// File Pilot: recent folders, bookmarks, drives with their usage, the
    /// user's folders.
    FilePilot,
    /// One Commander: drives, groups of bookmarks, the folders visited.
    OneCommander,
    /// macOS Finder: the user's folders, bookmarks, locations, tags.
    Finder,
    /// Directory Opus and XYplorer: bookmarks over a tree with lines.
    Classic,
    /// A strip of icons.
    Minimal,
}

impl SidebarStyle {
    /// Every style, in the order the settings page offers them.
    pub const ALL: [SidebarStyle; 6] = [
        SidebarStyle::Explorer,
        SidebarStyle::FilePilot,
        SidebarStyle::OneCommander,
        SidebarStyle::Finder,
        SidebarStyle::Classic,
        SidebarStyle::Minimal,
    ];

    /// The `config.toml` spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Explorer => "explorer",
            Self::FilePilot => "filepilot",
            Self::OneCommander => "onecommander",
            Self::Finder => "finder",
            Self::Classic => "classic",
            Self::Minimal => "minimal",
        }
    }

    /// The sidebar as this style draws it.
    pub fn look(self) -> SidebarLook {
        use SidebarSection::*;
        let base = SidebarLook {
            sections: Vec::new(),
            tree: false,
            tree_lines: false,
            follow_folder: false,
            headers: true,
            drive_usage: DriveUsage::None,
            drive_label: DriveLabel::Name,
            density: SidebarDensity::Normal,
            icons_only: false,
            recent_count: 10,
            cards: false,
        };
        match self {
            Self::Explorer => SidebarLook {
                sections: vec![QuickAccess, Favorites, ThisPc, Network, Tags],
                tree: true,
                follow_folder: true,
                headers: false,
                drive_usage: DriveUsage::Beside,
                ..base
            },
            Self::FilePilot => SidebarLook {
                sections: vec![Recent, Bookmarks, Drives, UserFolders, Tags],
                drive_usage: DriveUsage::Below,
                density: SidebarDensity::Compact,
                recent_count: 8,
                ..base
            },
            Self::OneCommander => SidebarLook {
                sections: vec![Drives, Bookmarks, Recent, Tags],
                drive_usage: DriveUsage::Below,
                drive_label: DriveLabel::LetterFirst,
                cards: true,
                ..base
            },
            Self::Finder => SidebarLook {
                sections: vec![UserFolders, Bookmarks, Drives, Network, Tags],
                ..base
            },
            Self::Classic => SidebarLook {
                sections: vec![Bookmarks, QuickAccess, ThisPc, Network, Tags],
                tree: true,
                tree_lines: true,
                follow_folder: true,
                drive_usage: DriveUsage::Beside,
                drive_label: DriveLabel::LetterFirst,
                density: SidebarDensity::Compact,
                ..base
            },
            Self::Minimal => SidebarLook {
                sections: vec![QuickAccess, Bookmarks, Drives, Tags],
                headers: false,
                drive_label: DriveLabel::Letter,
                icons_only: true,
                ..base
            },
        }
    }
}

/// A group of the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SidebarSection {
    /// The folders opened last, newest first.
    Recent,
    /// Windows' Quick access: pinned and frequent folders.
    QuickAccess,
    /// Files added with Windows' "Add to favorites".
    Favorites,
    /// Folders bookmarked here, in groups of their own.
    Bookmarks,
    /// Home, Desktop, Documents, Downloads, Pictures, Music, Videos.
    UserFolders,
    /// The drives, each a link.
    Drives,
    /// This PC with the drives under it.
    ThisPc,
    /// Network, the places added by hand, and Linux.
    Network,
    /// The tags.
    Tags,
}

impl SidebarSection {
    /// Every section, in the order the settings page lists them.
    pub const ALL: [SidebarSection; 9] = [
        SidebarSection::Recent,
        SidebarSection::QuickAccess,
        SidebarSection::Favorites,
        SidebarSection::Bookmarks,
        SidebarSection::UserFolders,
        SidebarSection::Drives,
        SidebarSection::ThisPc,
        SidebarSection::Network,
        SidebarSection::Tags,
    ];

    /// The `config.toml` spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Recent => "recent",
            Self::QuickAccess => "quick-access",
            Self::Favorites => "favorites",
            Self::Bookmarks => "bookmarks",
            Self::UserFolders => "user-folders",
            Self::Drives => "drives",
            Self::ThisPc => "this-pc",
            Self::Network => "network",
            Self::Tags => "tags",
        }
    }
}

/// Where a drive's usage bar goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DriveUsage {
    /// No bar.
    None,
    /// A short bar after the name, as Prime Files always drew it.
    Beside,
    /// A bar the width of the row under the name, as File Pilot's.
    Below,
}

/// How a drive is named in the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DriveLabel {
    /// Explorer's name: "Disco Local (C:)".
    Name,
    /// The letter, then the label: "C: Disco Local".
    LetterFirst,
    /// The letter alone: "C:".
    Letter,
}

/// How tall the sidebar's rows are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SidebarDensity {
    /// 22 px.
    Compact,
    /// 28 px, Explorer's.
    Normal,
    /// 34 px.
    Comfortable,
}

/// The sidebar with every part decided: the style's values under the ones
/// set by hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarLook {
    /// See [`Sidebar::sections`].
    pub sections: Vec<SidebarSection>,
    /// See [`Sidebar::tree`].
    pub tree: bool,
    /// See [`Sidebar::tree_lines`].
    pub tree_lines: bool,
    /// See [`Sidebar::follow_folder`].
    pub follow_folder: bool,
    /// See [`Sidebar::headers`].
    pub headers: bool,
    /// See [`Sidebar::drive_usage`].
    pub drive_usage: DriveUsage,
    /// See [`Sidebar::drive_label`].
    pub drive_label: DriveLabel,
    /// See [`Sidebar::density`].
    pub density: SidebarDensity,
    /// See [`Sidebar::icons_only`].
    pub icons_only: bool,
    /// See [`Sidebar::recent_count`].
    pub recent_count: u32,
    /// See [`Sidebar::cards`].
    pub cards: bool,
}

impl Default for SidebarLook {
    fn default() -> Self {
        SidebarStyle::default().look()
    }
}

impl Sidebar {
    /// The sidebar to draw.
    pub fn look(&self) -> SidebarLook {
        let style = self.style.look();
        SidebarLook {
            sections: self.sections.clone().unwrap_or(style.sections),
            tree: self.tree.unwrap_or(style.tree),
            tree_lines: self.tree_lines.unwrap_or(style.tree_lines),
            follow_folder: self.follow_folder.unwrap_or(style.follow_folder),
            headers: self.headers.unwrap_or(style.headers),
            drive_usage: self.drive_usage.unwrap_or(style.drive_usage),
            drive_label: self.drive_label.unwrap_or(style.drive_label),
            density: self.density.unwrap_or(style.density),
            icons_only: self.icons_only.unwrap_or(style.icons_only),
            recent_count: self.recent_count.unwrap_or(style.recent_count),
            cards: self.cards.unwrap_or(style.cards),
        }
    }

    /// The keys a style's choice clears, so the new style shows whole.
    pub const OVERRIDES: [&'static str; 11] = [
        "sections",
        "tree",
        "tree_lines",
        "follow_folder",
        "headers",
        "drive_usage",
        "drive_label",
        "density",
        "icons_only",
        "recent_count",
        "cards",
    ];
}

/// Automatic updates. Only an installed copy updates itself; one run from a
/// build folder never does, whatever this says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Updates {
    /// Look for a new version when the app starts, download it in the
    /// background and install it when the app closes.
    pub automatic: bool,
}

impl Default for Updates {
    fn default() -> Self {
        Self { automatic: true }
    }
}

/// How a 2-way split arranges its panes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SplitDirection {
    /// Panes side by side, separated by a vertical divider (`Cmd+\`).
    #[default]
    Vertical,
    /// Panes stacked, separated by a horizontal divider (`Cmd+Shift+\`).
    Horizontal,
}

impl SplitDirection {
    /// Parse the `config.toml` spelling (`vertical`/`horizontal`).
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "vertical" => Some(Self::Vertical),
            "horizontal" => Some(Self::Horizontal),
            _ => None,
        }
    }
}

/// What a new tab opens first: the folder at once, or the box that asks for
/// a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum NewTabMode {
    /// A tab at `new_tab_path` at once, as Explorer does.
    Folder,
    /// The box that searches the computer or takes a typed path.
    #[default]
    Box,
}

impl NewTabMode {
    /// Parse the `config.toml` spelling (`folder`/`box`).
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "folder" => Some(Self::Folder),
            "box" => Some(Self::Box),
            _ => None,
        }
    }
}

/// Keybinding overrides (config.md §2, draft for P3).
///
/// A draft surface: a map of action identifier to key chord (e.g.
/// `"quit" = "ctrl-q"`). An empty map — the default — means the built-in keymap
/// is used. The action vocabulary and live re-binding are defined in P3; until
/// then values are stored and validated but not applied (hot reload is a
/// restart, config.md §5), so arbitrary action names are accepted without
/// warning to keep forward compatibility.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Keybindings {
    /// Action identifier → key chord. Empty means "use the built-in keymap".
    #[serde(flatten)]
    pub bindings: BTreeMap<String, String>,
}

/// Plugin selection (config.md §2, reserved for P4).
///
/// Type definition / reservation only: the lists are parsed and stored, but no
/// plugin host loads them yet (that lands in P4).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Plugins {
    /// Built-in (first-party) plugins to enable, by id (e.g. `"git"`).
    pub core: Vec<String>,
    /// Community plugins to enable, as `user/repo` or a URL.
    pub community: Vec<String>,
}

/// Filesystem indexing settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Indexing {
    /// When the background index is built and maintained.
    pub mode: IndexingMode,
    /// Paths and globs excluded from indexing.
    pub exclude: IndexingExclude,
}

/// When the filesystem index is built and kept up to date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum IndexingMode {
    /// Index on demand and keep warm while the app is in use (the default).
    #[default]
    Auto,
    /// Continuously index in the background, even while idle.
    AlwaysOn,
    /// Only index when explicitly requested.
    Manual,
}

impl IndexingMode {
    /// Parse the `config.toml` spelling (`auto`/`always-on`/`manual`).
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(Self::Auto),
            "always-on" => Some(Self::AlwaysOn),
            "manual" => Some(Self::Manual),
            _ => None,
        }
    }
}

/// Paths and globs excluded from indexing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct IndexingExclude {
    /// Literal paths (absolute or relative to the indexed root) to skip.
    pub paths: Vec<String>,
    /// Glob patterns (e.g. `**/target/**`) to skip.
    pub globs: Vec<String>,
}

/// Search backend selection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Search {
    /// Which engine answers content searches.
    pub backend: SearchBackend,
}

/// The engine used to answer content searches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SearchBackend {
    /// Pick the best available backend at runtime (the default).
    #[default]
    Auto,
    /// The Tantivy index.
    Tantivy,
    /// External `ripgrep`.
    Ripgrep,
}

impl SearchBackend {
    /// Parse the `config.toml` spelling (`tantivy`/`ripgrep`/`auto`).
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "tantivy" => Some(Self::Tantivy),
            "ripgrep" => Some(Self::Ripgrep),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }
}

/// Launcher window settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Launcher {
    /// The shortcut that opens the quick search (Prime Files' own Listary)
    /// from any program: `off`, `double-ctrl`, `double-shift`, `double-alt`,
    /// a combination such as `ctrl+space`, or `auto`: Ctrl twice, or Shift
    /// twice when Listary is installed, so that the two do not open together.
    pub hotkey: String,
    /// Whether the launcher reopens at its last on-screen position.
    pub position_remember: bool,
}

impl Default for Launcher {
    fn default() -> Self {
        Self {
            hotkey: "auto".to_string(),
            position_remember: true,
        }
    }
}

/// Diagnostics / performance-logging settings (see `docs/persistence.md` §5).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Diagnostics {
    /// Performance logging for the persistence layer (`prime-files-store`).
    pub store: DiagnosticsStore,
}

/// Performance logging for the SQLite / redb store. All off by default, so a
/// default config adds no overhead. Output goes through `tracing` (targets
/// `prime_files_store::sql` / `prime_files_store::redb`) and is filterable with `RUST_LOG`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct DiagnosticsStore {
    /// Log every SQL query at `debug` (verbose).
    pub log_all_queries: bool,
    /// Log SQL queries slower than this many milliseconds at `warn`. Zero (the
    /// default) disables slow-query logging.
    pub slow_query_ms: u64,
    /// Log redb `get`/`put`/`delete`/`batch` operations and their durations.
    pub log_redb_ops: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
/// Light/dark appearance selection.
pub enum ThemeMode {
    /// Follow the operating system's light/dark preference.
    #[default]
    System,
    /// Always use the light appearance.
    Light,
    /// Always use the dark appearance.
    Dark,
}

impl ThemeMode {
    /// Parse the `config.toml` / CLI / env spelling (`system`/`light`/`dark`).
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "system" => Some(Self::System),
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
/// The column a directory listing is ordered by.
pub enum SortOrder {
    /// Sort alphabetically by entry name.
    #[default]
    Name,
    /// Sort by last-modified time.
    Modified,
    /// Sort by file size.
    Size,
    /// Sort by entry kind (file/directory/...).
    Kind,
}

impl SortOrder {
    /// Parse the `config.toml` / CLI / env spelling
    /// (`name`/`modified`/`size`/`kind`).
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "name" => Some(Self::Name),
            "modified" => Some(Self::Modified),
            "size" => Some(Self::Size),
            "kind" => Some(Self::Kind),
            _ => None,
        }
    }
}

/// Severity of a configuration [`Diagnostic`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticLevel {
    /// Recoverable: a value was ignored and a default used. The app keeps running.
    Warning,
    /// Fatal for the file: it could not be parsed (or targets an unknown schema
    /// version), so defaults are used. Surfaced in the UI status bar.
    Error,
}

/// A single message produced while loading configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Severity of the message.
    pub level: DiagnosticLevel,
    /// Human-readable description of the problem.
    pub message: String,
}

impl Diagnostic {
    fn warn(message: impl Into<String>) -> Self {
        Self {
            level: DiagnosticLevel::Warning,
            message: message.into(),
        }
    }

    fn error(message: impl Into<String>) -> Self {
        Self {
            level: DiagnosticLevel::Error,
            message: message.into(),
        }
    }
}

/// A sparse set of overrides applied on top of a loaded [`Config`]. Used for the
/// environment-variable and CLI-argument layers, where only the keys that were
/// actually provided should take effect.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigOverride {
    /// Override for [`Theme::mode`].
    pub theme_mode: Option<ThemeMode>,
    /// Override for [`Theme::accent`].
    pub theme_accent: Option<String>,
    /// Override for [`Ui::default_sort`].
    pub ui_default_sort: Option<SortOrder>,
    /// Override for [`Ui::show_hidden`].
    pub ui_show_hidden: Option<bool>,
    /// Override for [`Ui::icon_pack`].
    pub ui_icon_pack: Option<String>,
}

impl ConfigOverride {
    /// Build the environment-variable layer (`PRIME_FILES_*`). Invalid values are
    /// logged and skipped rather than aborting startup.
    pub fn from_env() -> Self {
        let mut over = ConfigOverride::default();

        if let Ok(value) = std::env::var("PRIME_FILES_THEME") {
            match ThemeMode::parse(value.trim()) {
                Some(mode) => over.theme_mode = Some(mode),
                None => tracing::warn!("ignoring invalid PRIME_FILES_THEME={value:?}"),
            }
        }
        if let Ok(value) = std::env::var("PRIME_FILES_ACCENT") {
            over.theme_accent = Some(value);
        }
        if let Ok(value) = std::env::var("PRIME_FILES_DEFAULT_SORT") {
            match SortOrder::parse(value.trim()) {
                Some(sort) => over.ui_default_sort = Some(sort),
                None => tracing::warn!("ignoring invalid PRIME_FILES_DEFAULT_SORT={value:?}"),
            }
        }
        if let Ok(value) = std::env::var("PRIME_FILES_SHOW_HIDDEN") {
            match parse_bool(value.trim()) {
                Some(flag) => over.ui_show_hidden = Some(flag),
                None => tracing::warn!("ignoring invalid PRIME_FILES_SHOW_HIDDEN={value:?}"),
            }
        }
        if let Ok(value) = std::env::var("PRIME_FILES_ICON_PACK") {
            over.ui_icon_pack = Some(value);
        }

        over
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

impl Config {
    /// Apply a sparse override layer in place. Only `Some` fields take effect.
    pub fn apply_override(&mut self, over: &ConfigOverride) {
        if let Some(mode) = over.theme_mode {
            self.theme.mode = mode;
        }
        if let Some(accent) = &over.theme_accent {
            self.theme.accent = accent.clone();
        }
        if let Some(sort) = over.ui_default_sort {
            self.ui.default_sort = sort;
        }
        if let Some(show_hidden) = over.ui_show_hidden {
            self.ui.show_hidden = show_hidden;
        }
        if let Some(icon_pack) = &over.ui_icon_pack {
            self.ui.icon_pack = icon_pack.clone();
        }
    }

    /// Parse `config.toml` contents into a [`Config`], collecting non-fatal
    /// problems as warnings. A TOML syntax error or an unknown `schema_version`
    /// yields [`Config::default`] plus an error diagnostic.
    ///
    /// This walks the parsed [`toml::Table`] field by field rather than using a
    /// strict `Deserialize`, so a single bad value (e.g. `mode = "neon"`) only
    /// resets that field instead of discarding the whole file.
    pub fn from_toml_str(contents: &str) -> (Config, Vec<Diagnostic>) {
        let table: toml::Table = match contents.parse() {
            Ok(table) => table,
            Err(error) => {
                return (
                    Config::default(),
                    vec![Diagnostic::error(format!(
                        "failed to parse config.toml: {error}"
                    ))],
                );
            }
        };
        Config::from_table(&table)
    }

    fn from_table(table: &toml::Table) -> (Config, Vec<Diagnostic>) {
        let mut config = Config::default();
        let mut diagnostics = Vec::new();

        // Reject configs from a future schema before reading anything else, so
        // we never half-apply a layout this build does not understand.
        if let Some(value) = table.get("schema_version") {
            match value.as_integer() {
                Some(version) if version as u64 == CURRENT_SCHEMA_VERSION => {}
                _ => {
                    diagnostics.push(Diagnostic::error(format!(
                        "config.toml targets schema_version {value}, but this Prime Files build \
                         supports {CURRENT_SCHEMA_VERSION}; a newer version may be required. \
                         Using defaults."
                    )));
                    return (Config::default(), diagnostics);
                }
            }
        }

        if let Some(theme) = table.get("theme") {
            match theme.as_table() {
                Some(theme) => read_theme(theme, &mut config.theme, &mut diagnostics),
                None => diagnostics.push(Diagnostic::warn("[theme] is not a table; ignoring")),
            }
        }
        if let Some(ui) = table.get("ui") {
            match ui.as_table() {
                Some(ui) => read_ui(ui, &mut config.ui, &mut diagnostics),
                None => diagnostics.push(Diagnostic::warn("[ui] is not a table; ignoring")),
            }
        }
        if let Some(value) = table.get("explorer") {
            match value.as_table() {
                Some(explorer) => read_explorer(explorer, &mut config.explorer, &mut diagnostics),
                None => diagnostics.push(Diagnostic::warn("[explorer] is not a table; ignoring")),
            }
        }
        if let Some(value) = table.get("updates") {
            match value.as_table() {
                Some(updates) => read_updates(updates, &mut config.updates, &mut diagnostics),
                None => diagnostics.push(Diagnostic::warn("[updates] is not a table; ignoring")),
            }
        }
        if let Some(value) = table.get("compact") {
            match value.as_table() {
                Some(compact) => read_compact(compact, &mut config.compact, &mut diagnostics),
                None => diagnostics.push(Diagnostic::warn("[compact] is not a table; ignoring")),
            }
        }
        if let Some(value) = table.get("layout") {
            match value.as_table() {
                Some(layout) => read_layout(layout, &mut config.layout, &mut diagnostics),
                None => diagnostics.push(Diagnostic::warn("[layout] is not a table; ignoring")),
            }
        }
        if let Some(value) = table.get("sidebar") {
            match value.as_table() {
                Some(sidebar) => read_sidebar(sidebar, &mut config.sidebar, &mut diagnostics),
                None => diagnostics.push(Diagnostic::warn("[sidebar] is not a table; ignoring")),
            }
        }
        if let Some(value) = table.get("menu") {
            match value.as_table() {
                Some(menu) => read_menu(menu, &mut config.menu, &mut diagnostics),
                None => diagnostics.push(Diagnostic::warn("[menu] is not a table; ignoring")),
            }
        }
        if let Some(value) = table.get("keybindings") {
            match value.as_table() {
                Some(keys) => read_keybindings(keys, &mut config.keybindings, &mut diagnostics),
                None => {
                    diagnostics.push(Diagnostic::warn("[keybindings] is not a table; ignoring"))
                }
            }
        }
        if let Some(value) = table.get("plugins") {
            match value.as_table() {
                Some(plugins) => read_plugins(plugins, &mut config.plugins, &mut diagnostics),
                None => diagnostics.push(Diagnostic::warn("[plugins] is not a table; ignoring")),
            }
        }
        if let Some(value) = table.get("indexing") {
            match value.as_table() {
                Some(indexing) => read_indexing(indexing, &mut config.indexing, &mut diagnostics),
                None => diagnostics.push(Diagnostic::warn("[indexing] is not a table; ignoring")),
            }
        }
        if let Some(value) = table.get("search") {
            match value.as_table() {
                Some(search) => read_search(search, &mut config.search, &mut diagnostics),
                None => diagnostics.push(Diagnostic::warn("[search] is not a table; ignoring")),
            }
        }
        if let Some(value) = table.get("launcher") {
            match value.as_table() {
                Some(launcher) => read_launcher(launcher, &mut config.launcher, &mut diagnostics),
                None => diagnostics.push(Diagnostic::warn("[launcher] is not a table; ignoring")),
            }
        }
        if let Some(value) = table.get("diagnostics") {
            match value.as_table() {
                Some(diag) => read_diagnostics(diag, &mut config.diagnostics, &mut diagnostics),
                None => {
                    diagnostics.push(Diagnostic::warn("[diagnostics] is not a table; ignoring"))
                }
            }
        }

        warn_unknown_keys(
            table,
            &[
                "schema_version",
                "theme",
                "ui",
                "explorer",
                "compact",
                "layout",
                "sidebar",
                "menu",
                "updates",
                "keybindings",
                "plugins",
                "indexing",
                "search",
                "launcher",
                "diagnostics",
            ],
            "",
            &mut diagnostics,
        );

        (config, diagnostics)
    }

    /// Serialize the merged config as pretty JSON (used by `prime-files config show`).
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }

    /// The default `config.toml` written on first run or `reset`, including the
    /// `#:schema` header. Every value matches [`Config::default`], so a freshly
    /// written file round-trips to defaults with no diagnostics.
    pub fn default_toml_template() -> String {
        format!(
            "#:schema {SCHEMA_URL}\n\
             schema_version = {CURRENT_SCHEMA_VERSION}\n\
             \n\
             [theme]\n\
             mode = \"dark\"   # \"system\" | \"light\" | \"dark\"\n\
             accent = \"orange\"   # \"orange\" | \"blue\" | \"teal\" | \"green\" | \"purple\" | \"pink\" | \"red\" | hex\n\
             \n\
             [ui]\n\
             language = \"auto\"   # \"auto\" (Windows region) | \"pt-BR\" | \"en\" | \"es\" | \"fr\" | \"de\" | \"it\" | \"ru\" | \"ja\" | \"zh-CN\" | \"ar\" | \"hu\"\n\
             default_sort =\"name\"   # \"name\" | \"modified\" | \"size\" | \"kind\"\n\
             default_sort_descending = false  # true = Z to A, largest or newest first\n\
             show_hidden = false\n\
             icon_pack = \"default\"\n\
             \n\
             [explorer]\n\
             split_direction = \"vertical\"   # \"vertical\" (left/right) | \"horizontal\" (top/bottom)\n\
             synced_panes = false            # when true, panes mirror the same path\n\
             restore_tabs = true             # restore the previous session's tabs on restart\n\
             \n\
             [compact]\n\
             enabled = false        # hide the bars below until the pointer reaches their edge\n\
             hide_tabs = true       # top edge\n\
             hide_header = true     # address bar: back, forward, path, refresh (top edge)\n\
             hide_sidebar = true    # left edge\n\
             hide_preview = true    # right edge\n\
             hide_bottom_bar = true # filter, counts, zoom (bottom edge)\n\
             overlay = true         # bars float over the listing; false = push it aside\n\
             reveal_delay_ms = 100  # rest at the edge this long before the bars come\n\
             hide_delay_ms = 400    # bars stay this long after the pointer leaves\n\
             animation_ms = 220     # slide length; 0 = no animation\n\
             edge_px = 8            # depth of the strip along each edge that calls its bars\n\
             frame_px = 8           # frame around the listing, rounded like Zen; 0 = none\n\
             \n\
             [updates]\n\
             automatic = true       # look for, download and install new versions (installed copies only)\n\
             \n\
             # The sections below are parsed and validated, but not yet applied at\n\
             # runtime — they take effect when their subsystem is wired in a later\n\
             # phase (docs/config.md §5). They are here so files and editor\n\
             # completion can adopt the shape early.\n\
             [indexing]\n\
             mode = \"auto\"   # \"auto\" | \"always-on\" | \"manual\"\n\
             \n\
             [indexing.exclude]\n\
             paths = []\n\
             globs = []\n\
             \n\
             [search]\n\
             backend = \"auto\"   # \"tantivy\" | \"ripgrep\" | \"auto\"\n\
             \n\
             [launcher]\n\
             hotkey = \"auto\"   # quick search: \"off\" | \"double-ctrl\" | \"double-shift\" | \"double-alt\" | \"ctrl+space\" | \"auto\"\n\
             position_remember = true\n\
             \n\
             # Store performance logging for analysis; all off by default.\n\
             # Output via tracing (RUST_LOG), see docs/persistence.md §5.\n\
             [diagnostics.store]\n\
             log_all_queries = false   # log every SQL query at debug\n\
             slow_query_ms = 0         # warn on SQL slower than this (0 = off)\n\
             log_redb_ops = false      # log redb get/put/delete/batch timings\n\
             \n\
             # Draft sections; defaults are built-in, so these stay empty until\n\
             # keybindings (P3) and plugins (P4) are wired up.\n\
             [keybindings]\n\
             # quit = \"ctrl-q\"\n\
             \n\
             [plugins]\n\
             # core = [\"git\"]\n\
             # community = [\"user/repo\"]\n"
        )
    }
}

fn read_theme(table: &toml::Table, theme: &mut Theme, diagnostics: &mut Vec<Diagnostic>) {
    if let Some(value) = table.get("mode") {
        match value.as_str().and_then(ThemeMode::parse) {
            Some(mode) => theme.mode = mode,
            None => diagnostics.push(Diagnostic::warn(format!(
                "invalid theme.mode {value}; using {:?}",
                theme.mode
            ))),
        }
    }
    if let Some(value) = table.get("accent") {
        match value.as_str() {
            Some(accent) if !accent.trim().is_empty() => theme.accent = accent.to_string(),
            _ => diagnostics.push(Diagnostic::warn(format!(
                "invalid theme.accent {value}; using {:?}",
                theme.accent
            ))),
        }
    }
    warn_unknown_keys(table, &["mode", "accent"], "theme.", diagnostics);
}

/// Reads the `[ui]` table into `ui`: the language, the listing defaults, the
/// hover-audio behaviour and the new-tab settings. A value the wrong type gets
/// a warning and leaves the field as it was.
fn read_ui(table: &toml::Table, ui: &mut Ui, diagnostics: &mut Vec<Diagnostic>) {
    if let Some(value) = table.get("language") {
        match value.as_str().map(str::trim) {
            Some(language) if language.eq_ignore_ascii_case("auto") => ui.language = "auto".to_string(),
            Some(language) if crate::i18n::Lang::parse(language).is_some() => ui.language = language.to_string(),
            _ => diagnostics.push(Diagnostic::warn(format!(
                "invalid ui.language {value}; using {:?}",
                ui.language
            ))),
        }
    }
    if let Some(value) = table.get("default_sort") {
        match value.as_str().and_then(SortOrder::parse) {
            Some(sort) => ui.default_sort = sort,
            None => diagnostics.push(Diagnostic::warn(format!(
                "invalid ui.default_sort {value}; using {:?}",
                ui.default_sort
            ))),
        }
    }
    if let Some(value) = table.get("default_sort_descending") {
        match value.as_bool() {
            Some(flag) => ui.default_sort_descending = flag,
            None => diagnostics.push(Diagnostic::warn(format!(
                "invalid ui.default_sort_descending {value}; using {}",
                ui.default_sort_descending
            ))),
        }
    }
    if let Some(value) = table.get("show_hidden") {
        match value.as_bool() {
            Some(flag) => ui.show_hidden = flag,
            None => diagnostics.push(Diagnostic::warn(format!(
                "invalid ui.show_hidden {value}; using {}",
                ui.show_hidden
            ))),
        }
    }
    if let Some(value) = table.get("icon_pack") {
        match value.as_str() {
            Some(pack) if !pack.trim().is_empty() => ui.icon_pack = pack.to_string(),
            _ => diagnostics.push(Diagnostic::warn(format!(
                "invalid ui.icon_pack {value}; using {:?}",
                ui.icon_pack
            ))),
        }
    }
    if let Some(value) = table.get("name_lines") {
        match value.as_integer().and_then(|lines| u32::try_from(lines).ok()).filter(|&lines| lines <= 20) {
            Some(lines) => ui.name_lines = lines,
            None => diagnostics.push(Diagnostic::warn(format!(
                "invalid ui.name_lines {value}; using {}",
                ui.name_lines
            ))),
        }
    }
    if let Some(value) = table.get("scroll_smoothness") {
        match value.as_integer().and_then(|v| u32::try_from(v).ok()).filter(|&v| v <= 100) {
            Some(smoothness) => ui.scroll_smoothness = smoothness,
            None => diagnostics.push(Diagnostic::warn(format!(
                "invalid ui.scroll_smoothness {value}; using {}",
                ui.scroll_smoothness
            ))),
        }
    }
    if let Some(value) = table.get("command_bar") {
        match value.as_bool() {
            Some(flag) => ui.command_bar = flag,
            None => diagnostics.push(Diagnostic::warn(format!("invalid ui.command_bar {value}; using {}", ui.command_bar))),
        }
    }
    if let Some(value) = table.get("folder_previews") {
        match value.as_bool() {
            Some(flag) => ui.folder_previews = flag,
            None => diagnostics.push(Diagnostic::warn(format!(
                "invalid ui.folder_previews {value}; using {}",
                ui.folder_previews
            ))),
        }
    }
    if let Some(value) = table.get("info_tips") {
        match value.as_bool() {
            Some(flag) => ui.info_tips = flag,
            None => diagnostics.push(Diagnostic::warn(format!("invalid ui.info_tips {value}; using {}", ui.info_tips))),
        }
    }
    if let Some(value) = table.get("type_to_select") {
        match value.as_bool() {
            Some(flag) => ui.type_to_select = flag,
            None => diagnostics.push(Diagnostic::warn(format!(
                "invalid ui.type_to_select {value}; using {}",
                ui.type_to_select
            ))),
        }
    }
    if let Some(value) = table.get("tab_icons") {
        match value.as_bool() {
            Some(flag) => ui.tab_icons = flag,
            None => diagnostics.push(Diagnostic::warn(format!("invalid ui.tab_icons {value}; using {}", ui.tab_icons))),
        }
    }
    if let Some(value) = table.get("infinite_scroll") {
        match value.as_bool() {
            Some(flag) => ui.infinite_scroll = flag,
            None => diagnostics.push(Diagnostic::warn(format!(
                "invalid ui.infinite_scroll {value}; using {}",
                ui.infinite_scroll
            ))),
        }
    }
    if let Some(value) = table.get("audio_waveforms") {
        match value.as_bool() {
            Some(flag) => ui.audio_waveforms = flag,
            None => diagnostics.push(Diagnostic::warn(format!(
                "invalid ui.audio_waveforms {value}; using {}",
                ui.audio_waveforms
            ))),
        }
    }
    if let Some(value) = table.get("preview_autoplay") {
        match value.as_bool() {
            Some(flag) => ui.preview_autoplay = flag,
            None => diagnostics.push(Diagnostic::warn(format!(
                "invalid ui.preview_autoplay {value}; using {}",
                ui.preview_autoplay
            ))),
        }
    }
    if let Some(value) = table.get("welcome_done") {
        match value.as_bool() {
            Some(flag) => ui.welcome_done = flag,
            None => diagnostics.push(Diagnostic::warn(format!(
                "invalid ui.welcome_done {value}; using {}",
                ui.welcome_done
            ))),
        }
    }
    if let Some(value) = table.get("hover_audio") {
        match value.as_bool() {
            Some(flag) => ui.hover_audio = flag,
            None => diagnostics.push(Diagnostic::warn(format!(
                "invalid ui.hover_audio {value}; using {}",
                ui.hover_audio
            ))),
        }
    }
    if let Some(value) = table.get("hover_audio_delay_ms") {
        match value.as_integer().and_then(|delay| u32::try_from(delay).ok()).filter(|&delay| delay <= 10_000) {
            Some(delay) => ui.hover_audio_delay_ms = delay,
            None => diagnostics.push(Diagnostic::warn(format!(
                "invalid ui.hover_audio_delay_ms {value}; using {}",
                ui.hover_audio_delay_ms
            ))),
        }
    }
    if let Some(value) = table.get("new_tab_path") {
        match value.as_str() {
            // Trimmed here so a quoted path with stray spaces still opens.
            Some(path) => ui.new_tab_path = path.trim().to_string(),
            None => diagnostics.push(Diagnostic::warn(format!(
                "invalid ui.new_tab_path {value}; expected a folder path"
            ))),
        }
    }
    if let Some(value) = table.get("new_tab_mode") {
        match value.as_str().and_then(NewTabMode::parse) {
            Some(mode) => ui.new_tab_mode = mode,
            None => diagnostics.push(Diagnostic::warn(format!(
                "invalid ui.new_tab_mode {value}; using {:?}",
                ui.new_tab_mode
            ))),
        }
    }
    warn_unknown_keys(
        table,
        &[
            "language",
            "default_sort",
            "default_sort_descending",
            "show_hidden",
            "icon_pack",
            "name_lines",
            "scroll_smoothness",
            "infinite_scroll",
            "tab_icons",
            "type_to_select",
            "info_tips",
            "folder_previews",
            "command_bar",
            "audio_waveforms",
            "preview_autoplay",
            "hover_audio",
            "hover_audio_delay_ms",
            "welcome_done",
            "new_tab_path",
            "new_tab_mode",
        ],
        "ui.",
        diagnostics,
    );
}

fn read_explorer(table: &toml::Table, explorer: &mut Explorer, diagnostics: &mut Vec<Diagnostic>) {
    if let Some(value) = table.get("split_direction") {
        match value.as_str().and_then(SplitDirection::parse) {
            Some(direction) => explorer.split_direction = direction,
            None => diagnostics.push(Diagnostic::warn(format!(
                "invalid explorer.split_direction {value}; using {:?}",
                explorer.split_direction
            ))),
        }
    }
    if let Some(value) = table.get("synced_panes") {
        match value.as_bool() {
            Some(flag) => explorer.synced_panes = flag,
            None => diagnostics.push(Diagnostic::warn(format!(
                "invalid explorer.synced_panes {value}; using {}",
                explorer.synced_panes
            ))),
        }
    }
    if let Some(value) = table.get("restore_tabs") {
        match value.as_bool() {
            Some(flag) => explorer.restore_tabs = flag,
            None => diagnostics.push(Diagnostic::warn(format!(
                "invalid explorer.restore_tabs {value}; using {}",
                explorer.restore_tabs
            ))),
        }
    }
    if let Some(value) = table.get("reveal_from_other_apps") {
        match value.as_bool() {
            Some(flag) => explorer.reveal_from_other_apps = flag,
            None => diagnostics.push(Diagnostic::warn(format!(
                "invalid explorer.reveal_from_other_apps {value}; using {}",
                explorer.reveal_from_other_apps
            ))),
        }
    }
    if let Some(value) = table.get("background_tabs") {
        match value.as_bool() {
            Some(flag) => explorer.background_tabs = flag,
            None => diagnostics.push(Diagnostic::warn(format!(
                "invalid explorer.background_tabs {value}; using {}",
                explorer.background_tabs
            ))),
        }
    }
    if let Some(value) = table.get("open_from_other_apps") {
        match value.as_str().and_then(OpenFromOtherApps::parse) {
            Some(place) => explorer.open_from_other_apps = place,
            None => diagnostics.push(Diagnostic::warn(format!(
                "invalid explorer.open_from_other_apps {value}; using {:?}",
                explorer.open_from_other_apps
            ))),
        }
    }
    warn_unknown_keys(
        table,
        &[
            "split_direction",
            "synced_panes",
            "restore_tabs",
            "reveal_from_other_apps",
            "background_tabs",
            "open_from_other_apps",
        ],
        "explorer.",
        diagnostics,
    );
}

fn read_menu(table: &toml::Table, menu: &mut Menu, diagnostics: &mut Vec<Diagnostic>) {
    for (key, slot) in [("quick", &mut menu.quick), ("items", &mut menu.items), ("hidden", &mut menu.hidden)] {
        let Some(value) = table.get(key) else { continue };
        let strings: Option<Vec<String>> = value
            .as_array()
            .and_then(|array| array.iter().map(|item| item.as_str().map(str::to_string)).collect());
        match strings {
            Some(strings) => *slot = strings,
            None => diagnostics.push(Diagnostic::warn(format!("invalid menu.{key} {value}; expected a list of strings"))),
        }
    }
    if let Some(value) = table.get("classic") {
        match value.as_bool() {
            Some(flag) => menu.classic = flag,
            None => diagnostics.push(Diagnostic::warn(format!("invalid menu.classic {value}; using {}", menu.classic))),
        }
    }
    warn_unknown_keys(table, &["quick", "items", "hidden", "classic"], "menu.", diagnostics);
}

fn read_updates(table: &toml::Table, updates: &mut Updates, diagnostics: &mut Vec<Diagnostic>) {
    if let Some(value) = table.get("automatic") {
        match value.as_bool() {
            Some(flag) => updates.automatic = flag,
            None => diagnostics.push(Diagnostic::warn(format!(
                "invalid updates.automatic {value}; using {}",
                updates.automatic
            ))),
        }
    }
    warn_unknown_keys(table, &["automatic"], "updates.", diagnostics);
}

fn read_compact(table: &toml::Table, compact: &mut Compact, diagnostics: &mut Vec<Diagnostic>) {
    let mut read_bool = |key: &str, slot: &mut bool| {
        if let Some(value) = table.get(key) {
            match value.as_bool() {
                Some(flag) => *slot = flag,
                None => diagnostics.push(Diagnostic::warn(format!(
                    "invalid compact.{key} {value}; using {slot}"
                ))),
            }
        }
    };
    read_bool("enabled", &mut compact.enabled);
    read_bool("hide_tabs", &mut compact.hide_tabs);
    read_bool("hide_header", &mut compact.hide_header);
    read_bool("hide_sidebar", &mut compact.hide_sidebar);
    read_bool("hide_preview", &mut compact.hide_preview);
    read_bool("hide_bottom_bar", &mut compact.hide_bottom_bar);
    read_bool("overlay", &mut compact.overlay);
    let mut read_number = |key: &str, max: u32, slot: &mut u32| {
        if let Some(value) = table.get(key) {
            match value.as_integer().and_then(|v| u32::try_from(v).ok()).filter(|&v| v <= max) {
                Some(number) => *slot = number,
                None => diagnostics.push(Diagnostic::warn(format!(
                    "invalid compact.{key} {value} (0..={max}); using {slot}"
                ))),
            }
        }
    };
    read_number("reveal_delay_ms", 5_000, &mut compact.reveal_delay_ms);
    read_number("hide_delay_ms", 10_000, &mut compact.hide_delay_ms);
    read_number("animation_ms", 2_000, &mut compact.animation_ms);
    read_number("edge_px", 64, &mut compact.edge_px);
    read_number("frame_px", 32, &mut compact.frame_px);
    warn_unknown_keys(
        table,
        &[
            "enabled",
            "hide_tabs",
            "hide_header",
            "hide_sidebar",
            "hide_preview",
            "hide_bottom_bar",
            "overlay",
            "reveal_delay_ms",
            "hide_delay_ms",
            "animation_ms",
            "edge_px",
            "frame_px",
        ],
        "compact.",
        diagnostics,
    );
}

fn read_layout(table: &toml::Table, layout: &mut Layout, diagnostics: &mut Vec<Diagnostic>) {
    for (key, slot) in [
        ("sidebar", &mut layout.sidebar),
        ("preview", &mut layout.preview),
        ("terminal", &mut layout.terminal),
    ] {
        if let Some(value) = table.get(key) {
            match value.as_str().and_then(DockSide::parse) {
                Some(side) => *slot = side,
                None => diagnostics.push(Diagnostic::warn(format!(
                    "invalid layout.{key} {value}; using {:?}",
                    slot
                ))),
            }
        }
    }
    for (key, slot) in [
        ("vertical_tabs", &mut layout.vertical_tabs),
        ("move_grips", &mut layout.move_grips),
        ("sidebar_tags", &mut layout.sidebar_tags),
    ] {
        if let Some(value) = table.get(key) {
            match value.as_bool() {
                Some(flag) => *slot = flag,
                None => diagnostics.push(Diagnostic::warn(format!("invalid layout.{key} {value}; using {slot}"))),
            }
        }
    }
    warn_unknown_keys(
        table,
        &["sidebar", "preview", "terminal", "vertical_tabs", "move_grips", "sidebar_tags"],
        "layout.",
        diagnostics,
    );
}

fn read_sidebar(table: &toml::Table, sidebar: &mut Sidebar, diagnostics: &mut Vec<Diagnostic>) {
    // The enums' spellings are their serde names; a value that is not one
    // keeps the style's.
    fn read_enum<T: serde::de::DeserializeOwned>(
        table: &toml::Table,
        key: &str,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Option<T> {
        let value = table.get(key)?;
        match value.clone().try_into::<T>() {
            Ok(parsed) => Some(parsed),
            Err(_) => {
                diagnostics.push(Diagnostic::warn(format!(
                    "invalid sidebar.{key} {value}; using the style's"
                )));
                None
            }
        }
    }
    if let Some(value) = table.get("style") {
        match value.clone().try_into::<SidebarStyle>() {
            Ok(style) => sidebar.style = style,
            Err(_) => diagnostics.push(Diagnostic::warn(format!(
                "invalid sidebar.style {value}; using {}",
                sidebar.style.as_str()
            ))),
        }
    }
    if let Some(value) = table.get("sections")
        && let Some(names) = read_string_array(value, "sidebar.sections", diagnostics)
    {
        let mut sections = Vec::new();
        for name in names {
            match toml::Value::String(name.clone()).try_into::<SidebarSection>() {
                Ok(section) if !sections.contains(&section) => sections.push(section),
                Ok(_) => {}
                Err(_) => diagnostics.push(Diagnostic::warn(format!(
                    "unknown sidebar section {name:?}; ignoring"
                ))),
            }
        }
        sidebar.sections = Some(sections);
    }
    for (key, slot) in [
        ("tree", &mut sidebar.tree),
        ("tree_lines", &mut sidebar.tree_lines),
        ("follow_folder", &mut sidebar.follow_folder),
        ("headers", &mut sidebar.headers),
        ("icons_only", &mut sidebar.icons_only),
        ("cards", &mut sidebar.cards),
    ] {
        if let Some(value) = table.get(key) {
            match value.as_bool() {
                Some(flag) => *slot = Some(flag),
                None => diagnostics.push(Diagnostic::warn(format!(
                    "invalid sidebar.{key} {value}; using the style's"
                ))),
            }
        }
    }
    if let Some(usage) = read_enum(table, "drive_usage", diagnostics) {
        sidebar.drive_usage = Some(usage);
    }
    if let Some(label) = read_enum(table, "drive_label", diagnostics) {
        sidebar.drive_label = Some(label);
    }
    if let Some(density) = read_enum(table, "density", diagnostics) {
        sidebar.density = Some(density);
    }
    if let Some(value) = table.get("recent_count") {
        match value
            .as_integer()
            .and_then(|count| u32::try_from(count).ok())
            .filter(|count| (1..=50).contains(count))
        {
            Some(count) => sidebar.recent_count = Some(count),
            None => diagnostics.push(Diagnostic::warn(format!(
                "invalid sidebar.recent_count {value} (1..=50); using the style's"
            ))),
        }
    }
    let mut known = vec!["style"];
    known.extend(Sidebar::OVERRIDES);
    warn_unknown_keys(table, &known, "sidebar.", diagnostics);
}

fn read_keybindings(
    table: &toml::Table,
    keybindings: &mut Keybindings,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Draft: accept any action name → chord. Unknown actions are not warned
    // about (the action vocabulary is defined in P3); only non-string values are.
    for (action, value) in table {
        match value.as_str() {
            Some(chord) => {
                keybindings
                    .bindings
                    .insert(action.clone(), chord.to_string());
            }
            None => diagnostics.push(Diagnostic::warn(format!(
                "invalid keybindings.{action} {value}; expected a string, ignoring"
            ))),
        }
    }
}

fn read_plugins(table: &toml::Table, plugins: &mut Plugins, diagnostics: &mut Vec<Diagnostic>) {
    if let Some(value) = table.get("core")
        && let Some(core) = read_string_array(value, "plugins.core", diagnostics)
    {
        plugins.core = core;
    }
    if let Some(value) = table.get("community")
        && let Some(community) = read_string_array(value, "plugins.community", diagnostics)
    {
        plugins.community = community;
    }
    warn_unknown_keys(table, &["core", "community"], "plugins.", diagnostics);
}

fn read_indexing(table: &toml::Table, indexing: &mut Indexing, diagnostics: &mut Vec<Diagnostic>) {
    if let Some(value) = table.get("mode") {
        match value.as_str().and_then(IndexingMode::parse) {
            Some(mode) => indexing.mode = mode,
            None => diagnostics.push(Diagnostic::warn(format!(
                "invalid indexing.mode {value}; using {:?}",
                indexing.mode
            ))),
        }
    }
    if let Some(value) = table.get("exclude") {
        match value.as_table() {
            Some(exclude) => read_indexing_exclude(exclude, &mut indexing.exclude, diagnostics),
            None => diagnostics.push(Diagnostic::warn(
                "[indexing.exclude] is not a table; ignoring",
            )),
        }
    }
    warn_unknown_keys(table, &["mode", "exclude"], "indexing.", diagnostics);
}

fn read_indexing_exclude(
    table: &toml::Table,
    exclude: &mut IndexingExclude,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Some(value) = table.get("paths")
        && let Some(paths) = read_string_array(value, "indexing.exclude.paths", diagnostics)
    {
        exclude.paths = paths;
    }
    if let Some(value) = table.get("globs")
        && let Some(globs) = read_string_array(value, "indexing.exclude.globs", diagnostics)
    {
        exclude.globs = globs;
    }
    warn_unknown_keys(table, &["paths", "globs"], "indexing.exclude.", diagnostics);
}

fn read_search(table: &toml::Table, search: &mut Search, diagnostics: &mut Vec<Diagnostic>) {
    if let Some(value) = table.get("backend") {
        match value.as_str().and_then(SearchBackend::parse) {
            Some(backend) => search.backend = backend,
            None => diagnostics.push(Diagnostic::warn(format!(
                "invalid search.backend {value}; using {:?}",
                search.backend
            ))),
        }
    }
    warn_unknown_keys(table, &["backend"], "search.", diagnostics);
}

fn read_launcher(table: &toml::Table, launcher: &mut Launcher, diagnostics: &mut Vec<Diagnostic>) {
    if let Some(value) = table.get("hotkey") {
        match value.as_str() {
            Some(hotkey) if !hotkey.trim().is_empty() => launcher.hotkey = hotkey.to_string(),
            _ => diagnostics.push(Diagnostic::warn(format!(
                "invalid launcher.hotkey {value}; using {:?}",
                launcher.hotkey
            ))),
        }
    }
    if let Some(value) = table.get("position_remember") {
        match value.as_bool() {
            Some(flag) => launcher.position_remember = flag,
            None => diagnostics.push(Diagnostic::warn(format!(
                "invalid launcher.position_remember {value}; using {}",
                launcher.position_remember
            ))),
        }
    }
    warn_unknown_keys(
        table,
        &["hotkey", "position_remember"],
        "launcher.",
        diagnostics,
    );
}

/// Read a TOML array of strings, warning about (and skipping) any non-string
/// element. Returns `None` — with a warning — when the value is not an array,
/// so the caller keeps its current value.
fn read_string_array(
    value: &toml::Value,
    field: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Vec<String>> {
    match value.as_array() {
        Some(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item.as_str() {
                    Some(entry) => out.push(entry.to_string()),
                    None => diagnostics.push(Diagnostic::warn(format!(
                        "ignoring non-string entry {item} in {field}"
                    ))),
                }
            }
            Some(out)
        }
        None => {
            diagnostics.push(Diagnostic::warn(format!(
                "{field} is not an array; ignoring"
            )));
            None
        }
    }
}

fn read_diagnostics(
    table: &toml::Table,
    diag: &mut Diagnostics,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Some(value) = table.get("store") {
        match value.as_table() {
            Some(store) => read_diagnostics_store(store, &mut diag.store, diagnostics),
            None => diagnostics.push(Diagnostic::warn(
                "[diagnostics.store] is not a table; ignoring",
            )),
        }
    }
    warn_unknown_keys(table, &["store"], "diagnostics.", diagnostics);
}

fn read_diagnostics_store(
    table: &toml::Table,
    store: &mut DiagnosticsStore,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Some(value) = table.get("log_all_queries") {
        match value.as_bool() {
            Some(flag) => store.log_all_queries = flag,
            None => diagnostics.push(Diagnostic::warn(format!(
                "invalid diagnostics.store.log_all_queries {value}; using {}",
                store.log_all_queries
            ))),
        }
    }
    if let Some(value) = table.get("slow_query_ms") {
        match value.as_integer() {
            Some(ms) if ms >= 0 => store.slow_query_ms = ms as u64,
            _ => diagnostics.push(Diagnostic::warn(format!(
                "invalid diagnostics.store.slow_query_ms {value}; using {}",
                store.slow_query_ms
            ))),
        }
    }
    if let Some(value) = table.get("log_redb_ops") {
        match value.as_bool() {
            Some(flag) => store.log_redb_ops = flag,
            None => diagnostics.push(Diagnostic::warn(format!(
                "invalid diagnostics.store.log_redb_ops {value}; using {}",
                store.log_redb_ops
            ))),
        }
    }
    warn_unknown_keys(
        table,
        &["log_all_queries", "slow_query_ms", "log_redb_ops"],
        "diagnostics.store.",
        diagnostics,
    );
}

/// Emit a warning for every key in `table` that is not a known key. `prefix` is
/// prepended for nested tables (e.g. `"theme."`) so messages point at the
/// offending path.
fn warn_unknown_keys(
    table: &toml::Table,
    known: &[&str],
    prefix: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for key in table.keys() {
        if !known.contains(&key.as_str()) {
            diagnostics.push(Diagnostic::warn(format!(
                "unknown config key {prefix}{key}; ignoring"
            )));
        }
    }
}

/// Generate the JSON Schema for [`Config`] as pretty JSON. Backs both
/// `prime-files config schema` and the committed `docs/config.schema.json`.
///
/// The schema is normalized so the output is portable and byte-for-byte stable:
///
/// * Object keys are sorted, so the result does not depend on whether
///   `serde_json`'s `preserve_order` feature is enabled (it is in the GUI binary
///   but not in a `prime-files-core`-only build, via Cargo feature unification);
///   otherwise the committed file and `prime-files config schema` would disagree.
/// * Schemars' Rust-specific integer `format`s (e.g. `uint64`) are dropped:
///   they are not standard JSON Schema draft 2020-12 formats, and `type` +
///   `minimum` already capture the constraint. Standard string formats are kept.
///
/// There are no `required` arrays: every field is `#[serde(default)]`, so the
/// loader (and a conforming editor) accepts a file that omits any section.
/// Array element order (enum variants) is left untouched.
pub fn json_schema_string() -> serde_json::Result<String> {
    let schema = schemars::schema_for!(Config);
    let value = serde_json::to_value(&schema)?;
    serde_json::to_string_pretty(&normalize_schema(value))
}

/// Rust integer `format` values schemars emits that are not standard JSON Schema.
const RUST_INT_FORMATS: &[&str] = &[
    "uint", "uint8", "uint16", "uint32", "uint64", "uint128", "int", "int8", "int16", "int32",
    "int64", "int128",
];

fn normalize_schema(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut entries: Vec<_> = map.into_iter().collect();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));
            let mut normalized = serde_json::Map::new();
            for (key, child) in entries {
                if key == "format"
                    && child
                        .as_str()
                        .is_some_and(|format| RUST_INT_FORMATS.contains(&format))
                {
                    continue;
                }
                normalized.insert(key, normalize_schema(child));
            }
            serde_json::Value::Object(normalized)
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(normalize_schema).collect())
        }
        other => other,
    }
}

/// Read `path` and parse it, returning [`Config::default`] with no diagnostics
/// when the file does not exist (a missing config is normal, not an error).
pub fn load_from_path(path: &Path) -> (Config, Vec<Diagnostic>) {
    // Config is read synchronously at startup, before the GPUI foreground loop
    // runs, so blocking I/O here cannot stall rendering.
    #[allow(clippy::disallowed_methods)]
    match std::fs::read_to_string(path) {
        Ok(contents) => Config::from_toml_str(&contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            (Config::default(), Vec::new())
        }
        Err(error) => (
            Config::default(),
            vec![Diagnostic::error(format!(
                "could not read {}: {error}",
                path.display()
            ))],
        ),
    }
}

/// Log all diagnostics and return the first error message (if any) so the caller
/// can surface it in the UI. Warnings are logged but never returned.
pub fn report_diagnostics(diagnostics: &[Diagnostic]) -> Option<String> {
    let mut first_error = None;
    for diagnostic in diagnostics {
        match diagnostic.level {
            DiagnosticLevel::Warning => tracing::warn!("config: {}", diagnostic.message),
            DiagnosticLevel::Error => {
                tracing::error!("config: {}", diagnostic.message);
                if first_error.is_none() {
                    first_error = Some(diagnostic.message.clone());
                }
            }
        }
    }
    first_error
}

#[cfg(test)]
// Rust 2024 made `std::env::set_var` unsafe; these tests must mutate the
// process environment to exercise env-var config overrides.
#[allow(clippy::unwrap_used, clippy::disallowed_methods, unsafe_code)]
mod tests {
    use super::*;
    use crate::config::test_env::env_lock;

    fn errors(diagnostics: &[Diagnostic]) -> Vec<&str> {
        diagnostics
            .iter()
            .filter(|d| d.level == DiagnosticLevel::Error)
            .map(|d| d.message.as_str())
            .collect()
    }

    #[test]
    fn defaults_match_spec() {
        let config = Config::default();
        assert_eq!(config.schema_version, 1);
        assert_eq!(config.theme.mode, ThemeMode::Dark);
        assert_eq!(config.theme.accent, "orange");
        assert_eq!(config.ui.default_sort, SortOrder::Name);
        assert!(!config.ui.show_hidden);
        assert_eq!(config.ui.icon_pack, "default");
    }

    #[test]
    fn parses_full_config() {
        let toml = r##"
            schema_version = 1
            [theme]
            mode = "dark"
            accent = "#ff8800"
            [ui]
            default_sort = "modified"
            default_sort_descending = true
            show_hidden = true
            icon_pack = "nerd"
        "##;
        let (config, diagnostics) = Config::from_toml_str(toml);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(config.theme.mode, ThemeMode::Dark);
        assert_eq!(config.theme.accent, "#ff8800");
        assert_eq!(config.ui.default_sort, SortOrder::Modified);
        assert!(config.ui.default_sort_descending);
        assert!(config.ui.show_hidden);
        assert_eq!(config.ui.icon_pack, "nerd");
    }

    #[test]
    fn parses_diagnostics_store() {
        let toml = r#"
            [diagnostics.store]
            log_all_queries = true
            slow_query_ms = 50
            log_redb_ops = true
        "#;
        let (config, diagnostics) = Config::from_toml_str(toml);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert!(config.diagnostics.store.log_all_queries);
        assert_eq!(config.diagnostics.store.slow_query_ms, 50);
        assert!(config.diagnostics.store.log_redb_ops);
    }

    #[test]
    fn parses_p2_sections() {
        let toml = r#"
            [indexing]
            mode = "always-on"
            [indexing.exclude]
            paths = ["/tmp", "node_modules"]
            globs = ["**/target/**"]
            [search]
            backend = "tantivy"
            [launcher]
            hotkey = "Ctrl+Space"
            position_remember = false
            [plugins]
            core = ["git", "calculator"]
            community = ["user/repo"]
            [keybindings]
            quit = "ctrl-q"
            search = "ctrl-f"
        "#;
        let (config, diagnostics) = Config::from_toml_str(toml);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(config.indexing.mode, IndexingMode::AlwaysOn);
        assert_eq!(config.indexing.exclude.paths, ["/tmp", "node_modules"]);
        assert_eq!(config.indexing.exclude.globs, ["**/target/**"]);
        assert_eq!(config.search.backend, SearchBackend::Tantivy);
        assert_eq!(config.launcher.hotkey, "Ctrl+Space");
        assert!(!config.launcher.position_remember);
        assert_eq!(config.plugins.core, ["git", "calculator"]);
        assert_eq!(config.plugins.community, ["user/repo"]);
        assert_eq!(
            config.keybindings.bindings.get("quit").map(String::as_str),
            Some("ctrl-q")
        );
        assert_eq!(
            config
                .keybindings
                .bindings
                .get("search")
                .map(String::as_str),
            Some("ctrl-f")
        );
    }

    #[test]
    fn p2_sections_reject_bad_values_leniently() {
        let toml = r#"
            [indexing]
            mode = "turbo"
            [indexing.exclude]
            paths = "not-an-array"
            globs = [1, "ok"]
            [search]
            backend = "grep"
            [launcher]
            hotkey = ""
            position_remember = "yes"
            [plugins]
            core = "git"
            [keybindings]
            quit = 7
        "#;
        let (config, diagnostics) = Config::from_toml_str(toml);
        // No fatal errors: bad fields fall back to defaults.
        assert!(errors(&diagnostics).is_empty());
        assert_eq!(config.indexing.mode, IndexingMode::Auto);
        assert!(config.indexing.exclude.paths.is_empty());
        // The non-string array entry is skipped; the valid one is kept.
        assert_eq!(config.indexing.exclude.globs, ["ok"]);
        assert_eq!(config.search.backend, SearchBackend::Auto);
        assert_eq!(config.launcher.hotkey, Config::default().launcher.hotkey);
        assert!(config.launcher.position_remember);
        assert!(config.plugins.core.is_empty());
        assert!(config.keybindings.bindings.is_empty());
        // mode, paths(not array), globs(one bad entry), backend, hotkey,
        // position_remember, plugins.core(not array), keybindings.quit.
        assert_eq!(diagnostics.len(), 8);
    }

    #[test]
    fn diagnostics_store_rejects_bad_values_leniently() {
        let toml = r#"
            [diagnostics.store]
            slow_query_ms = -5
            log_all_queries = "yes"
        "#;
        let (config, diagnostics) = Config::from_toml_str(toml);
        assert!(errors(&diagnostics).is_empty());
        // Bad values fall back to defaults.
        assert_eq!(config.diagnostics.store.slow_query_ms, 0);
        assert!(!config.diagnostics.store.log_all_queries);
        assert_eq!(diagnostics.len(), 2);
    }

    #[test]
    fn explorer_section_parses_direction_and_sync() {
        let toml = r#"
            [explorer]
            split_direction = "horizontal"
            synced_panes = true
        "#;
        let (config, diagnostics) = Config::from_toml_str(toml);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(config.explorer.split_direction, SplitDirection::Horizontal);
        assert!(config.explorer.synced_panes);
    }

    #[test]
    fn explorer_section_rejects_bad_values_leniently() {
        let toml = r#"
            [explorer]
            split_direction = "diagonal"
            synced_panes = "maybe"
        "#;
        let (config, diagnostics) = Config::from_toml_str(toml);
        assert!(errors(&diagnostics).is_empty());
        // Bad values fall back to defaults.
        assert_eq!(config.explorer.split_direction, SplitDirection::Vertical);
        assert!(!config.explorer.synced_panes);
        assert_eq!(diagnostics.len(), 2);
    }

    #[test]
    fn syntax_error_falls_back_without_panicking() {
        let (config, diagnostics) = Config::from_toml_str("this = = broken");
        assert_eq!(config, Config::default());
        assert_eq!(errors(&diagnostics).len(), 1);
    }

    #[test]
    fn out_of_range_value_warns_and_keeps_default() {
        let toml = r#"
            [theme]
            mode = "neon"
            [ui]
            default_sort = 42
            show_hidden = "yes"
        "#;
        let (config, diagnostics) = Config::from_toml_str(toml);
        // No errors: the file still loads.
        assert!(errors(&diagnostics).is_empty());
        // Bad fields fall back to defaults; others are untouched.
        assert_eq!(config.theme.mode, ThemeMode::Dark);
        assert_eq!(config.ui.default_sort, SortOrder::Name);
        assert!(!config.ui.show_hidden);
        // Three warnings: mode, default_sort, show_hidden.
        assert_eq!(diagnostics.len(), 3);
    }

    #[test]
    fn unknown_keys_warn_but_load() {
        let toml = r#"
            mystery = 1
            [theme]
            mode = "light"
            shade = "x"
            [keybindings]
            quit = "ctrl-q"
        "#;
        let (config, diagnostics) = Config::from_toml_str(toml);
        assert!(errors(&diagnostics).is_empty());
        assert_eq!(config.theme.mode, ThemeMode::Light);
        // `mystery` and `theme.shade` warn; `[keybindings]` is reserved and silent.
        let warnings: Vec<_> = diagnostics.iter().map(|d| d.message.as_str()).collect();
        assert!(warnings.iter().any(|m| m.contains("mystery")));
        assert!(warnings.iter().any(|m| m.contains("theme.shade")));
        assert!(!warnings.iter().any(|m| m.contains("keybindings")));
    }

    #[test]
    fn future_schema_version_falls_back() {
        let (config, diagnostics) = Config::from_toml_str("schema_version = 99");
        assert_eq!(config, Config::default());
        assert_eq!(errors(&diagnostics).len(), 1);
        assert!(errors(&diagnostics)[0].contains("newer version"));
    }

    #[test]
    fn env_layer_overrides_file() {
        let (mut config, _) = Config::from_toml_str("[theme]\nmode = \"light\"");
        config.apply_override(&ConfigOverride {
            theme_mode: Some(ThemeMode::Dark),
            ..Default::default()
        });
        assert_eq!(config.theme.mode, ThemeMode::Dark);
    }

    #[test]
    fn override_is_sparse() {
        let mut config = Config::default();
        config.theme.accent = "green".into();
        config.apply_override(&ConfigOverride {
            ui_show_hidden: Some(true),
            ..Default::default()
        });
        // Only the provided field changes.
        assert!(config.ui.show_hidden);
        assert_eq!(config.theme.accent, "green");
    }

    #[test]
    fn parse_bool_accepts_common_spellings() {
        assert_eq!(parse_bool("TRUE"), Some(true));
        assert_eq!(parse_bool("off"), Some(false));
        assert_eq!(parse_bool("maybe"), None);
    }

    #[test]
    fn default_template_round_trips() {
        let (config, diagnostics) = Config::from_toml_str(&Config::default_toml_template());
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(config, Config::default());
    }

    /// `new_tab_mode` and `new_tab_path` are read, trimmed, and known keys.
    #[test]
    fn ui_new_tab_keys_are_read() {
        let (config, diagnostics) = Config::from_toml_str(
            "[ui]\nnew_tab_mode = \"folder\"\nnew_tab_path = \"  C:\\\\Work  \"\n",
        );
        assert_eq!(config.ui.new_tab_mode, NewTabMode::Folder);
        assert_eq!(config.ui.new_tab_path, r"C:\Work", "the path is trimmed");
        assert!(diagnostics.is_empty(), "both keys are known: {diagnostics:?}");
    }

    /// A mode that is not `folder` or `box` warns and keeps the default.
    #[test]
    fn ui_new_tab_mode_rejects_nonsense() {
        let (config, diagnostics) = Config::from_toml_str("[ui]\nnew_tab_mode = \"nope\"\n");
        assert_eq!(config.ui.new_tab_mode, NewTabMode::Box);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    }

    #[test]
    fn json_schema_is_generatable() {
        let schema = json_schema_string().unwrap();
        assert!(schema.contains("schema_version"));
        assert!(schema.contains("default_sort"));
    }

    #[test]
    fn schema_has_no_required_so_partial_configs_validate() {
        // The loader defaults every field (it overlays a parsed table onto
        // `Config::default`), so no section is required. The schema must agree,
        // otherwise editors reject files the app accepts.
        let schema = json_schema_string().unwrap();
        assert!(
            !schema.contains("\"required\""),
            "schema must not mark any field required"
        );
        let (config, diagnostics) = Config::from_toml_str("");
        assert_eq!(config, Config::default());
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn committed_schema_is_up_to_date() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/config.schema.json");
        // Git may check the file out with CRLF (`core.autocrlf` on Windows);
        // the generator always writes LF.
        let committed = std::fs::read_to_string(path).unwrap().replace("\r\n", "\n");
        let generated = json_schema_string().unwrap();
        assert_eq!(
            committed.trim_end(),
            generated.trim_end(),
            "docs/config.schema.json is stale; regenerate with `prime-files config schema > docs/config.schema.json`"
        );
    }

    // Snapshot of the field-by-field parser output, including the warning emitted
    // for an invalid value. Review/refresh with `cargo insta review` when the
    // parser's behavior intentionally changes.
    #[test]
    fn from_toml_str_output_snapshot() {
        let toml = r#"
            schema_version = 1
            [theme]
            mode = "neon"
            accent = "teal"
            [ui]
            show_hidden = true
            unknown_key = 1
        "#;
        let parsed = Config::from_toml_str(toml);
        insta::assert_debug_snapshot!(parsed);
    }

    #[test]
    fn menu_section_reads_lists_and_rejects_other_values() {
        let toml = r#"
            [menu]
            quick = ["copy_path", "delete"]
            items = "windows"
            hidden = ["Enviar para"]
            [explorer]
            background_tabs = false
        "#;
        let (config, diagnostics) = Config::from_toml_str(toml);
        assert_eq!(config.menu.quick, vec!["copy_path".to_string(), "delete".to_string()]);
        assert_eq!(config.menu.items, Menu::default().items, "a string is not a list");
        assert_eq!(config.menu.hidden, vec!["Enviar para".to_string()]);
        assert!(!config.explorer.background_tabs);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    }

    // The settings page wrote `classic = true` and the reader called it an
    // unknown key, so the switch never moved (06/11/2026).
    #[test]
    fn menu_classic_is_read() {
        let (config, diagnostics) = Config::from_toml_str("[menu]\nclassic = true\n");
        assert!(config.menu.classic);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn sidebar_style_fills_what_was_not_set_by_hand() {
        let toml = r#"
            [sidebar]
            style = "filepilot"
            tree_lines = true
            sections = ["drives", "nonsense", "drives", "tags"]
            density = "huge"
            recent_count = 0
        "#;
        let (config, diagnostics) = Config::from_toml_str(toml);
        let look = config.sidebar.look();
        assert_eq!(config.sidebar.style, SidebarStyle::FilePilot);
        assert_eq!(
            look.sections,
            vec![SidebarSection::Drives, SidebarSection::Tags]
        );
        assert!(look.tree_lines);
        assert_eq!(look.drive_usage, DriveUsage::Below);
        assert_eq!(look.density, SidebarStyle::FilePilot.look().density);
        assert_eq!(
            look.recent_count,
            SidebarStyle::FilePilot.look().recent_count
        );
        // The unknown section, the bad density and the bad count.
        assert_eq!(diagnostics.len(), 3, "{diagnostics:?}");
    }

    // Every style lists the tags; `layout.sidebar_tags` turns them off in
    // all of them at once (the owner, 08/11/2026).
    #[test]
    fn every_style_lists_the_tags() {
        for style in SidebarStyle::ALL {
            assert!(style.look().sections.contains(&SidebarSection::Tags), "{}", style.as_str());
        }
    }

    #[test]
    fn default_sidebar_is_explorers() {
        let look = Config::default().sidebar.look();
        assert_eq!(look, SidebarStyle::Explorer.look());
        assert!(look.tree);
        assert!(look.sections.contains(&SidebarSection::ThisPc));
    }

    #[test]
    fn every_style_and_section_reads_back_from_its_spelling() {
        for style in SidebarStyle::ALL {
            let (config, diagnostics) =
                Config::from_toml_str(&format!("[sidebar]\nstyle = \"{}\"\n", style.as_str()));
            assert_eq!(config.sidebar.style, style);
            assert!(diagnostics.is_empty(), "{diagnostics:?}");
        }
        let names: Vec<String> = SidebarSection::ALL
            .iter()
            .map(|section| format!("\"{}\"", section.as_str()))
            .collect();
        let (config, diagnostics) =
            Config::from_toml_str(&format!("[sidebar]\nsections = [{}]\n", names.join(", ")));
        assert_eq!(config.sidebar.sections, Some(SidebarSection::ALL.to_vec()));
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn compact_section_reads_values_and_rejects_out_of_range() {
        let toml = r#"
            [compact]
            enabled = true
            hide_sidebar = false
            overlay = false
            animation_ms = 90
            edge_px = 999
            hide_delay_ms = "long"
        "#;
        let (config, diagnostics) = Config::from_toml_str(toml);
        assert!(config.compact.enabled);
        assert!(!config.compact.hide_sidebar);
        assert!(config.compact.hide_preview);
        assert!(!config.compact.overlay);
        assert_eq!(config.compact.animation_ms, 90);
        assert_eq!(config.compact.edge_px, Compact::default().edge_px);
        assert_eq!(config.compact.hide_delay_ms, Compact::default().hide_delay_ms);
        assert_eq!(diagnostics.len(), 2);
    }

    #[test]
    fn theme_mode_parse_accepts_only_known_spellings() {
        assert_eq!(ThemeMode::parse("system"), Some(ThemeMode::System));
        assert_eq!(ThemeMode::parse("light"), Some(ThemeMode::Light));
        assert_eq!(ThemeMode::parse("dark"), Some(ThemeMode::Dark));
        assert_eq!(ThemeMode::parse("Dark"), None);
        assert_eq!(ThemeMode::parse(""), None);
        assert_eq!(ThemeMode::parse("neon"), None);
    }

    #[test]
    fn sort_order_parse_accepts_only_known_spellings() {
        assert_eq!(SortOrder::parse("name"), Some(SortOrder::Name));
        assert_eq!(SortOrder::parse("modified"), Some(SortOrder::Modified));
        assert_eq!(SortOrder::parse("size"), Some(SortOrder::Size));
        assert_eq!(SortOrder::parse("kind"), Some(SortOrder::Kind));
        assert_eq!(SortOrder::parse("Size"), None);
        assert_eq!(SortOrder::parse("created"), None);
    }

    #[test]
    fn empty_config_is_defaults_without_diagnostics() {
        let (config, diagnostics) = Config::from_toml_str("");
        assert_eq!(config, Config::default());
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn only_schema_version_loads_clean() {
        let (config, diagnostics) = Config::from_toml_str("schema_version = 1");
        assert_eq!(config, Config::default());
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn non_table_sections_warn_and_keep_defaults() {
        let (config, diagnostics) = Config::from_toml_str("theme = \"x\"\nui = 1");
        assert_eq!(config, Config::default());
        let warnings: Vec<_> = diagnostics.iter().map(|d| d.message.as_str()).collect();
        assert!(warnings.iter().any(|m| m.contains("[theme]")));
        assert!(warnings.iter().any(|m| m.contains("[ui]")));
    }

    #[test]
    fn empty_accent_warns_and_keeps_default() {
        let toml = "[theme]\naccent = \"\"";
        let (config, diagnostics) = Config::from_toml_str(toml);
        assert_eq!(config.theme.accent, Config::default().theme.accent);
        assert!(diagnostics.iter().any(|d| d.message.contains("accent")));
    }

    #[test]
    fn apply_override_sets_every_field() {
        let mut config = Config::default();
        config.apply_override(&ConfigOverride {
            theme_mode: Some(ThemeMode::Dark),
            theme_accent: Some("crimson".into()),
            ui_default_sort: Some(SortOrder::Size),
            ui_show_hidden: Some(true),
            ui_icon_pack: Some("nerd".into()),
        });
        assert_eq!(config.theme.mode, ThemeMode::Dark);
        assert_eq!(config.theme.accent, "crimson");
        assert_eq!(config.ui.default_sort, SortOrder::Size);
        assert!(config.ui.show_hidden);
        assert_eq!(config.ui.icon_pack, "nerd");
    }

    #[test]
    fn empty_override_is_a_no_op() {
        let mut config = Config::default();
        let before = config.clone();
        config.apply_override(&ConfigOverride::default());
        assert_eq!(config, before);
    }

    #[test]
    fn to_json_round_trips_to_equal_config() {
        let config = Config::default();
        let json = config.to_json().unwrap();
        assert!(json.contains("schema_version"));
        let parsed: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, config);
    }

    #[test]
    fn from_env_reads_valid_vars() {
        let _guard = env_lock();
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("PRIME_FILES_THEME", "dark") };
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("PRIME_FILES_ACCENT", "violet") };
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("PRIME_FILES_DEFAULT_SORT", "size") };
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("PRIME_FILES_SHOW_HIDDEN", "on") };
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("PRIME_FILES_ICON_PACK", "nerd") };
        let over = ConfigOverride::from_env();
        for key in [
            "PRIME_FILES_THEME",
            "PRIME_FILES_ACCENT",
            "PRIME_FILES_DEFAULT_SORT",
            "PRIME_FILES_SHOW_HIDDEN",
            "PRIME_FILES_ICON_PACK",
        ] {
            // TODO: Audit that the environment access only happens in single-threaded code.
            unsafe { std::env::remove_var(key) };
        }
        assert_eq!(over.theme_mode, Some(ThemeMode::Dark));
        assert_eq!(over.theme_accent.as_deref(), Some("violet"));
        assert_eq!(over.ui_default_sort, Some(SortOrder::Size));
        assert_eq!(over.ui_show_hidden, Some(true));
        assert_eq!(over.ui_icon_pack.as_deref(), Some("nerd"));
    }

    #[test]
    fn from_env_skips_invalid_values() {
        let _guard = env_lock();
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("PRIME_FILES_THEME", "neon") };
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("PRIME_FILES_SHOW_HIDDEN", "maybe") };
        let over = ConfigOverride::from_env();
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("PRIME_FILES_THEME") };
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("PRIME_FILES_SHOW_HIDDEN") };
        assert_eq!(over.theme_mode, None);
        assert_eq!(over.ui_show_hidden, None);
    }
}
