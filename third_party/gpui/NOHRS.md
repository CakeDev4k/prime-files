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

The third, in `src/taffy.rs`, `to_grid_repeat`: the literals in
`minmax(length(0.0), fr(1.0))` are written `0.0_f32` and `1.0_f32`. Without
the suffix rustc 1.9x warns that the float falls back to `f32` through a
trait bound it does not satisfy, a fallback it is phasing out and will make
an error (rust-lang/rust#154024). Upstream may already have fixed it.

When gpui is upgraded: check whether a caption or caption-button press, or a
caption-button release, still returns `Some(0)` when the dispatch was
"handled" (the lines are marked `NOHRS`).

## Right-to-left text (05/11/2026)

`src/platform/windows/direct_write.rs`, `DrawGlyphRun`: glyphs are placed
from the `baselineOriginX` DirectWrite hands each run, and a run with an odd
`bidiLevel` is laid from its right edge leftwards. The original laid every
run end to end from the left in logical order, so Arabic (added as an
interface language) came out with its letters reversed. `StringIndexConverter`
rewinds when a run starts before the previous one, which visual order makes
possible. The paragraph stays left to right: the text box is infinitely wide,
and a right-to-left paragraph would be aligned to its far end.

When gpui is upgraded: check whether `DrawGlyphRun` still ignores the origin
and the bidi level.

## Default font fallbacks (05/11/2026)

`direct_write.rs`, where a font is loaded: a font asked for without
fallbacks gets Segoe UI and Tahoma ahead of the system's list. gpui takes the
UI font from the icon-title font of Windows (`SPI_GETICONTITLELOGFONT`); a
user with that set to a font without Arabic saw Arabic file names as boxes
(Prime Files issue #1). Not reproduced here: on this machine the font is
Segoe UI and Arabic already showed.

Another, for files dragged in from another program (07/11/2026). gpui
answered Windows "Copy" in `DragEnter`, `DragOver` and `Drop` whatever the
app was going to do, so the cursor said "Copy" over a folder where the drop
moved, and it ignored the `grfKeyState` OLE passes: the window is usually not
the focused one then, so `Window::modifiers()` kept whatever it was when it
last had the keyboard, and Shift-to-move did nothing. Now:

- `src/platform/windows/window.rs`, `WindowsDragDropHandler`: each of the
  three reports the keys as a `ModifiersChanged` before the move it
  dispatches (`report_drag_keys`), and `DragEnter`/`DragOver` answer the
  effect the app set during that move (`dispatch_drag_move`), "Copy" when it
  set none. `Drop` still answers "Copy" when the app moves: the app moves the
  files itself, and "Move" would have Explorer delete the originals too.
- `src/interactive.rs`: `FileDropEffect`. `src/platform.rs`:
  `PlatformWindow::set_file_drop_effect`, a no-op except on Windows.
  `src/window.rs`: `Window::set_file_drop_effect`, and the mouse events made
  from a file drop carry `self.modifiers` instead of none.

When gpui is upgraded: check whether its drop target still answers a fixed
`DROPEFFECT_COPY` and drops `grfKeyState` (the lines are marked `NOHRS`).

## Startup: one Direct3D device, the cached font list (07/11/2026)

Two calls made before the first frame, timed in probes that make the same
calls in a fresh process, 5 rounds alternated, on the owner's machine
(RTX 5050 driving the monitor, 1,530 fonts installed):

- `src/platform/windows/directx_devices.rs`, `get_adapter`: upstream created a
  full Direct3D 11 device on each adapter only to see that it could, dropped
  it, and created it again in `DirectXDevices::new`. Now the device made while
  choosing the adapter is the one kept. Both: 317 ms (median); one: 181 ms.
- `src/platform/windows/direct_write.rs`, `DirectWriteTextSystem::new`:
  `GetSystemFontCollection(.., checkForUpdates)` passed `true`, which makes
  DirectWrite look for fonts installed since its cache last did: 134 ms
  against 1.8 ms with `false`, the same 388 families either way.
  `update_system_font_collection` still passes `true`, so a font missing at
  lookup is still looked for.

When gpui is upgraded: check whether `get_adapter` still makes a throwaway
device and the first `GetSystemFontCollection` still checks for updates (the
lines are marked `NOHRS`).
