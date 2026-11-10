//! Reusable GPUI building blocks for Prime Files.
//!
//! `prime-files-ui` hosts the toolkit-level pieces — theme, assets, window chrome, and
//! presentational components — deliberately kept free of app- or page-specific
//! wiring so they can be reused (and eventually published) independently of the
//! Prime Files binary. It depends only on `prime-files-core` and `prime-files-models`; the app
//! shell that orchestrates pages lives in the `prime-files` binary crate.

/// Embedded crate-local assets (icons, fonts) exposed as a GPUI `AssetSource`.
pub mod assets;
/// Presentational UI components built on GPUI.
pub mod components;
/// Theme tokens (colors) shared across components.
pub mod theme;
/// Window construction helpers, including unified toolbar window options.
pub mod window;

pub use components::file_list;
