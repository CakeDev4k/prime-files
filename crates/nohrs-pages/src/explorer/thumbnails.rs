//! Thumbnails for the grid: extracted on worker threads, held per pane for the
//! directory on screen, and handed to GPUI as ready-made `RenderImage`s.

use gpui::{App, Context, RenderImage, Task};
use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use super::ExplorerPane;

/// Worker threads extracting thumbnails. Extraction is mostly waiting on disk
/// and on the system's decoders, so a few run side by side; more would only
/// compete with the listing itself for the disk.
const WORKERS: usize = 3;

enum Entry {
    Loading(#[allow(dead_code)] Task<()>),
    Ready(Arc<RenderImage>),
    Failed,
}

/// The thumbnails one pane has asked for, keyed by path and requested size.
#[derive(Default)]
pub struct Thumbnails {
    directory: String,
    entries: HashMap<(String, u32), Entry>,
}

impl Thumbnails {
    /// Forgets everything held for another directory. Dropping a `Loading`
    /// entry drops its task, which closes the reply channel, so a worker that
    /// has not reached the job yet skips it rather than decoding for nobody.
    pub fn show_directory(&mut self, directory: &str, cx: &mut App) {
        if self.directory == directory {
            return;
        }
        self.directory = directory.to_string();
        for (_, entry) in self.entries.drain() {
            // The sprite atlas keeps its own copy of every image it has drawn;
            // without this each folder visited would stay in GPU memory.
            if let Entry::Ready(image) = entry {
                cx.drop_image(image, None);
            }
        }
    }

    /// The thumbnail of `path` at `size`, asking for it if it was never asked
    /// for. While a larger size is on its way a smaller one already held is
    /// returned, so zooming in does not flash every tile back to an icon.
    pub fn get(
        &mut self,
        path: &str,
        size: u32,
        cx: &mut Context<ExplorerPane>,
    ) -> Option<Arc<RenderImage>> {
        let key = (path.to_string(), size);
        match self.entries.get(&key) {
            Some(Entry::Ready(image)) => return Some(image.clone()),
            Some(Entry::Failed) => return None,
            Some(Entry::Loading(_)) => {}
            None => {
                let reply = service().request(path.to_string(), size);
                let task = cx.spawn({
                    let key = key.clone();
                    async move |this, cx| {
                        let image = reply.recv().await.ok().flatten();
                        this.update(cx, |pane, cx| {
                            // A reply for a key no longer held belongs to a
                            // directory that was left while it was in flight.
                            if let Some(entry) = pane.thumbnails.entries.get_mut(&key) {
                                *entry = match image {
                                    Some(image) => Entry::Ready(image),
                                    None => Entry::Failed,
                                };
                                cx.notify();
                            }
                        })
                        .ok();
                    }
                });
                self.entries.insert(key, Entry::Loading(task));
            }
        }
        self.entries
            .iter()
            .filter(|((held_path, held_size), _)| held_path == path && *held_size < size)
            .max_by_key(|((_, held_size), _)| *held_size)
            .and_then(|(_, entry)| match entry {
                Entry::Ready(image) => Some(image.clone()),
                _ => None,
            })
    }
}

struct Job {
    path: String,
    size: u32,
    reply: async_channel::Sender<Option<Arc<RenderImage>>>,
}

struct Service {
    jobs: Mutex<Vec<Job>>,
    available: Condvar,
}

fn service() -> &'static Service {
    static SERVICE: OnceLock<Arc<Service>> = OnceLock::new();
    SERVICE.get_or_init(|| {
        let service = Arc::new(Service {
            jobs: Mutex::new(Vec::new()),
            available: Condvar::new(),
        });
        for index in 0..WORKERS {
            let service = service.clone();
            let spawned = std::thread::Builder::new()
                .name(format!("nohrs-thumbnails-{index}"))
                .spawn(move || service.work());
            if let Err(error) = spawned {
                tracing::error!("could not start a thumbnail worker: {error}");
            }
        }
        service
    })
}

impl Service {
    fn request(
        &self,
        path: String,
        size: u32,
    ) -> async_channel::Receiver<Option<Arc<RenderImage>>> {
        let (reply, receiver) = async_channel::bounded(1);
        match self.jobs.lock() {
            Ok(mut jobs) => jobs.push(Job { path, size, reply }),
            Err(error) => tracing::error!("thumbnail queue poisoned: {error}"),
        }
        self.available.notify_one();
        receiver
    }

    fn work(&self) {
        platform::init_thread();
        loop {
            let job = {
                let Ok(mut jobs) = self.jobs.lock() else {
                    return;
                };
                loop {
                    // Newest first: the tiles asked for last are the ones the
                    // user has scrolled to, while the oldest may be far off
                    // screen by now.
                    if let Some(job) = jobs.pop() {
                        break job;
                    }
                    jobs = match self.available.wait(jobs) {
                        Ok(jobs) => jobs,
                        Err(_) => return,
                    };
                }
            };
            if job.reply.is_closed() {
                continue;
            }
            let image = platform::extract(&job.path, job.size).map(Arc::new);
            // The pane may have moved on meanwhile; nothing is waiting then.
            job.reply.try_send(image).ok();
        }
    }
}

fn render_image(width: u32, height: u32, bgra: Vec<u8>) -> Option<RenderImage> {
    let buffer = image::RgbaImage::from_raw(width, height, bgra)?;
    Some(RenderImage::new(vec![image::Frame::new(buffer)]))
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod platform {
    //! The Shell's thumbnail extractor. `unsafe` is confined to this module:
    //! COM and GDI have no safe bindings, and the alternative — decoding every
    //! format ourselves — would still leave video frames and program icons out.

    use gpui::RenderImage;
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
        DeleteObject, GetDIBits, GetObjectW, HGDIOBJ,
    };
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
    use windows::Win32::UI::Shell::{
        IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_RESIZETOFIT,
    };
    use windows::core::HSTRING;

    pub fn init_thread() {
        // Some thumbnail handlers are apartment-threaded, so each worker is its
        // own STA, as Explorer's are. The thread lives for the whole run, so it
        // is never uninitialized.
        let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if result.is_err() {
            tracing::warn!("COM init for thumbnails failed: {result:?}");
        }
    }

    pub fn extract(path: &str, size: u32) -> Option<RenderImage> {
        let edge = i32::try_from(size).ok()?;
        let bitmap = unsafe {
            let factory: IShellItemImageFactory =
                SHCreateItemFromParsingName(&HSTRING::from(path), None).ok()?;
            // Without a flag the Shell falls back to the file type's icon when
            // there is no thumbnail, which is what a tile wants for a .dll too.
            factory
                .GetImage(SIZE { cx: edge, cy: edge }, SIIGBF_RESIZETOFIT)
                .ok()?
        };

        let pixels = unsafe {
            let mut info = BITMAP::default();
            let filled = GetObjectW(
                HGDIOBJ(bitmap.0),
                std::mem::size_of::<BITMAP>() as i32,
                Some((&mut info as *mut BITMAP).cast()),
            );
            let read = if filled == 0 || info.bmWidth <= 0 || info.bmHeight <= 0 {
                None
            } else {
                let (width, height) = (info.bmWidth, info.bmHeight);
                let mut header = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: width,
                        // Negative height asks for rows top-down, the order
                        // `RenderImage` stores them in.
                        biHeight: -height,
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB.0,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let mut buffer = vec![0u8; width as usize * height as usize * 4];
                let device = CreateCompatibleDC(None);
                let lines = GetDIBits(
                    device,
                    bitmap,
                    0,
                    height as u32,
                    Some(buffer.as_mut_ptr().cast()),
                    &mut header,
                    DIB_RGB_COLORS,
                );
                DeleteDC(device).ok().ok();
                (lines == height).then_some((width as u32, height as u32, buffer))
            };
            DeleteObject(HGDIOBJ(bitmap.0)).ok().ok();
            read
        }?;

        let (width, height, mut bgra) = pixels;
        // Photo and video thumbnails come back as 32-bit pixels whose alpha
        // byte was never written. Read as written they would be fully
        // transparent, so a bitmap with no alpha anywhere is opaque. Icons do
        // carry alpha, already premultiplied, the form GPUI keeps images in. (A
        // program with only a small icon arrives framed in a dark square; that
        // is the Shell's own rendering, the same one Explorer shows.)
        if bgra.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 0) {
            for pixel in bgra.as_chunks_mut::<4>().0 {
                pixel[3] = 0xFF;
            }
        }
        super::render_image(width, height, bgra)
    }
}

#[cfg(not(windows))]
mod platform {
    //! Elsewhere the formats `image` decodes here (PNG and JPEG) get a scaled
    //! copy; anything else keeps its icon.

    use gpui::RenderImage;

    pub fn init_thread() {}

    pub fn extract(path: &str, size: u32) -> Option<RenderImage> {
        let decoded = image::open(path).ok()?.thumbnail(size, size).into_rgba8();
        let (width, height) = decoded.dimensions();
        let mut bgra = decoded.into_raw();
        for pixel in bgra.as_chunks_mut::<4>().0 {
            let alpha = u16::from(pixel[3]);
            let premultiply = |channel: u8| ((u16::from(channel) * alpha + 127) / 255) as u8;
            let (red, green, blue) = (pixel[0], pixel[1], pixel[2]);
            pixel[0] = premultiply(blue);
            pixel[1] = premultiply(green);
            pixel[2] = premultiply(red);
        }
        super::render_image(width, height, bgra)
    }
}
