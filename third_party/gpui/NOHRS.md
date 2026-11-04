# gpui 0.2.2, patched for Nohrs

A copy of gpui 0.2.2 from crates.io with two changes. The first, in
`src/platform/windows/window.rs`, `DragEnter`: the CF_HDROP `HGLOBAL` is passed
to `DragQueryFileW` as the `HDROP` it is, instead of being dereferenced as a
pointer to one.

The original only worked for moveable memory, which is what OLE hands over when
the drag comes from another process. A drag that starts in Nohrs and passes over
its own window carries the Shell's fixed memory; the dereference read the start
of the DROPFILES block as a handle and `DragQueryFile` crashed with an access
violation in `windows.storage.dll` (seen 04/11/2026, dump `nohrs.exe.19032.dmp`).

When gpui is upgraded: check whether `DragEnter` still dereferences the handle.
If it does not, delete this folder and the `[patch.crates-io]` entry in the
workspace `Cargo.toml`.

The second, in `src/platform/windows/events.rs`, `handle_nc_mouse_down_msg`:
a press on a `WindowControlArea::Drag` (`HTCAPTION`) always reaches
`DefWindowProc`. gpui first dispatches it as a `MouseDown`, and every element
with `track_focus` calls `prevent_default` on one; the result counted as
handled and the message was swallowed, so a window whose root tracks focus
(Nohrs's does) could not be moved by its title bar at all (04/11/2026).

The same goes for the caption buttons (`HTMINBUTTON`, `HTMAXBUTTON`,
`HTCLOSE`), on the press and on the release: the press records the button in
`nc_button_pressed`, the release acts on it, and a "handled" dispatch skipped
both, so minimize, maximize and close did nothing whenever the root under them
saw the press (seen 04/11/2026; it only worked where an element that occludes
hid the root from the pointer).

When gpui is upgraded: check whether a caption or caption-button press, or a
caption-button release, still returns `Some(0)` when the dispatch was
"handled" (the lines are marked `NOHRS`).
