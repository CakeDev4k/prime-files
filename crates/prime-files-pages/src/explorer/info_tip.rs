//! The note that comes up when the pointer rests on a file or folder, with
//! what Explorer's says there (issue #48): the type, size and dates, the
//! dimensions of a picture, the length of a video or song, the size and
//! contents of a folder.
//!
//! The text is the Shell's own (`IQueryInfo::GetInfoTip`), so it is what
//! Explorer shows, in the system's language, with whatever the installed
//! property handlers add. Asking for it can take a while (a folder's size is
//! counted, a video's header read), so it is fetched on a thread of its own
//! and the note shows the size the listing already knows until it arrives.

use gpui::{AnyView, App, AppContext as _, Context, IntoElement, ParentElement, Render, Styled, Task, Window, div, px, rgb};
use prime_files_ui::theme::theme;
use std::cell::RefCell;
use std::collections::HashMap;

/// Notes kept, by path, size and date: rest the pointer on the same file
/// again and the note is there at once. Past this many it starts over.
const CACHE_CAP: usize = 2000;

thread_local! {
    static CACHE: RefCell<HashMap<(String, u64, u64), String>> = RefCell::new(HashMap::new());
}

/// The note for one item, built by a tooltip when it opens.
pub(crate) struct InfoTip {
    text: String,
    _fetch: Option<Task<()>>,
}

impl InfoTip {
    /// The note for `path` (of `size` bytes, changed at `modified`), with
    /// `meanwhile` shown until the Shell's text arrives.
    pub(crate) fn build(path: String, size: u64, modified: u64, meanwhile: String, cx: &mut App) -> AnyView {
        let key = (path, size, modified);
        let cached = CACHE.with(|cache| cache.borrow().get(&key).cloned());
        cx.new(|cx: &mut Context<Self>| match cached {
            Some(text) => Self { text, _fetch: None },
            None => {
                let path = key.0.clone();
                let fetched = cx.background_spawn(async move { tip_of(&path) });
                let fetch = cx.spawn(async move |this, cx| {
                    let Some(text) = fetched.await else {
                        return;
                    };
                    CACHE.with(|cache| {
                        let mut cache = cache.borrow_mut();
                        if cache.len() >= CACHE_CAP {
                            cache.clear();
                        }
                        cache.insert(key, text.clone());
                    });
                    this.update(cx, |tip, cx| {
                        tip.text = text;
                        tip._fetch = None;
                        cx.notify();
                    })
                    .ok();
                });
                Self { text: meanwhile, _fetch: Some(fetch) }
            }
        })
        .into()
    }
}

impl Render for InfoTip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .max_w(px(420.0))
            .px(px(10.0))
            .py(px(6.0))
            .flex()
            .flex_col()
            .gap(px(2.0))
            .rounded(px(6.0))
            .border_1()
            .border_color(rgb(theme::border()))
            .bg(rgb(theme::bg_secondary()))
            .shadow_md()
            .text_xs()
            .text_color(rgb(theme::fg()))
            .children(
                self.text
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .map(|line| div().child(line.to_string())),
            )
    }
}

// The Shell's note for `path`, on a thread with COM of its own.
fn tip_of(path: &str) -> Option<String> {
    let path = path.to_string();
    std::thread::Builder::new()
        .name("prime-files-info-tip".into())
        .spawn(move || platform::tip(&path))
        .ok()?
        .join()
        .ok()
        .flatten()
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod platform {
    //! The Shell's info tips have no safe binding.

    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize};
    use windows::Win32::UI::Shell::{BHID_SFUIObject, IQueryInfo, IShellItem, QITIPF_USESLOWTIP, SHCreateItemFromParsingName};
    use windows::core::HSTRING;

    pub fn tip(path: &str) -> Option<String> {
        let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
        let text = read(path);
        if initialized {
            unsafe { CoUninitialize() };
        }
        text
    }

    fn read(path: &str) -> Option<String> {
        let item: IShellItem = unsafe { SHCreateItemFromParsingName(&HSTRING::from(path), None) }.ok()?;
        let info: IQueryInfo = unsafe { item.BindToHandler(None, &BHID_SFUIObject) }.ok()?;
        // The slow tip is the full one: a folder's size and contents.
        let tip = unsafe { info.GetInfoTip(QITIPF_USESLOWTIP) }.ok()?;
        if tip.is_null() {
            return None;
        }
        let text = unsafe { tip.to_string() }.ok();
        unsafe { CoTaskMemFree(Some(tip.0 as *const _)) };
        text.filter(|text| !text.trim().is_empty())
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn tip(_path: &str) -> Option<String> {
        None
    }
}
