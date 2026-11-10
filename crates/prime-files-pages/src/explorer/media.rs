//! Video and audio in the preview pane, played by Windows' own Media
//! Foundation engine (what the Films & TV app uses), so every format Windows
//! plays plays here, hardware decoding included.
//!
//! The engine runs on a thread of its own in frame-server mode: it decodes,
//! and each new video frame is copied into a bitmap the size of the preview
//! and handed to gpui as an image. No window of the engine's is put over
//! the app, so menus and dialogs still draw on top of the video.

use gpui::{Context, RenderImage, Task};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;

use super::ExplorerPane;

// The preview's volume (an f64's bits) and mute, for the whole app.
static MEDIA_VOLUME: AtomicU64 = AtomicU64::new(0x3FF0_0000_0000_0000);
static MEDIA_MUTED: AtomicBool = AtomicBool::new(false);
// The playback speed (an f64's bits), kept from one file to the next for the
// run: someone listening through a folder at 1.5× wants the next one at it too.
static MEDIA_RATE: AtomicU64 = AtomicU64::new(0x3FF0_0000_0000_0000);

/// The speeds the player offers.
pub(crate) const RATES: [f64; 6] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0];

/// Largest frame handed to the UI, in pixels. Frames are scaled down by the
/// engine to fit, so a 4K video costs what a 1080p one does.
const MAX_FRAME: (u32, u32) = (1280, 720);

/// What the preview's controls ask of the player.
pub(crate) enum Command {
    Play,
    Pause,
    /// Seconds from the start.
    Seek(f64),
    /// 0..=1.
    Volume(f64),
    /// Playback speed, 1 the file's own.
    Rate(f64),
    /// The engine's indices of the audio streams to hear, mixed.
    AudioStreams(Vec<u32>),
}

/// One audio stream of the file, as the player's list shows it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AudioStream {
    /// The engine's index of the stream among all of the file's.
    pub index: u32,
    pub name: Option<String>,
    pub language: Option<String>,
    pub channels: Option<u32>,
    pub enabled: bool,
}

/// Which of the player bar's menus is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MediaMenu {
    Speed,
    AudioStreams,
}

/// A video or audio file playing (or paused) in the preview.
pub(crate) struct MediaPreview {
    commands: mpsc::Sender<Command>,
    /// The frame on screen; `None` for audio and until the first arrives.
    pub frame: Option<Arc<RenderImage>>,
    /// Seconds.
    pub position: f64,
    /// Seconds; 0 until the file's length is known.
    pub duration: f64,
    pub playing: bool,
    pub has_video: bool,
    pub error: Option<String>,
    /// While a video Windows cannot play is converted for the preview
    /// (`video_proxy`): how far, 0..=1, or `Some(None)` with no length known.
    pub preparing: Option<Option<f32>>,
    /// Loudest sample of each `PEAK_BLOCKS_PER_SECOND`th of a second of an
    /// audio file, 0..=1, filled from the start as the file is read. Empty
    /// for video. An `Arc` so a frame's paint can hold it without a copy.
    pub peaks: Arc<Vec<f32>>,
    /// Loudest of `peaks`, which the waveform is scaled to.
    pub peak_max: f32,
    /// Playback speed.
    pub rate: f64,
    /// The file's audio streams once known; the bar lists them only when
    /// there is more than one.
    pub audio_streams: Vec<AudioStream>,
    pub menu: Option<MediaMenu>,
    _task: Task<()>,
}

/// Resolution of the waveform: a peak per 10 ms, fine enough for a bar per
/// pixel of a 3-minute song across a wide pane.
pub(crate) const PEAK_BLOCKS_PER_SECOND: usize = 100;

enum Update {
    Frame(RenderImage),
    State { position: f64, duration: f64, playing: bool, has_video: bool },
    /// The next stretch of `MediaPreview::peaks`.
    Peaks(Vec<f32>),
    /// Converting with FFmpeg (`Some`), or done with it (`None`).
    Preparing(Option<Option<f32>>),
    /// The file's audio streams, and which play.
    AudioStreams(Vec<AudioStream>),
    Failed(String),
}

/// `1×`, `1.25×`, `0.5×`.
pub(crate) fn rate_label(rate: f64) -> String {
    format!("{rate}×")
}

/// How the list names a stream: `Faixa 2 · Commentary · en · estéreo`.
/// `position` counts from 0 among the audio streams.
pub(crate) fn stream_label(position: usize, stream: &AudioStream) -> String {
    let mut parts = vec![prime_files_core::tf!("Faixa {n}", n = position + 1)];
    parts.extend(stream.name.iter().filter(|name| !name.trim().is_empty()).cloned());
    parts.extend(stream.language.iter().filter(|language| !language.trim().is_empty()).cloned());
    parts.extend(stream.channels.map(channels_label));
    parts.join(" · ")
}

fn channels_label(channels: u32) -> String {
    match channels {
        1 => prime_files_core::t!("mono").to_string(),
        2 => prime_files_core::t!("estéreo").to_string(),
        6 => "5.1".into(),
        8 => "7.1".into(),
        n => prime_files_core::tf!("{n} canais", n = n),
    }
}

fn extension(path: &str) -> String {
    std::path::Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase()
}

/// Whether `path` is a sound-only file, which the preview draws as a waveform.
pub(crate) fn is_audio(path: &str) -> bool {
    matches!(
        extension(path).as_str(),
        "mp3" | "wav" | "m4a" | "aac" | "flac" | "wma" | "ogg" | "opus"
    )
}

/// Whether the preview plays `path` as video or audio.
pub(crate) fn is_media(path: &str) -> bool {
    is_audio(path)
        || matches!(
            extension(path).as_str(),
            "mp4" | "m4v" | "mov" | "wmv" | "avi" | "mkv" | "webm" | "mpg" | "mpeg" | "3gp" | "ts" | "mts"
        )
}

/// The volume a player is told to play at.
fn effective_volume(volume: f64, muted: bool) -> f64 {
    if muted { 0.0 } else { volume.clamp(0.0, 1.0) }
}

impl MediaPreview {
    /// Sends `command` to the player.
    pub(crate) fn send(&self, command: Command) {
        // The player thread only ends when this preview is dropped.
        self.commands.send(command).ok();
    }
}

impl MediaPreview {
    /// Starts a player for `path`, paused on its first frame unless
    /// `autoplay`. `slot` finds it again in the view that keeps it, so what
    /// the player sends lands there; once the slot is empty the player ends.
    /// The pane's preview and the search box's (`palette`) both use it.
    pub(crate) fn start<V: 'static>(
        path: String,
        autoplay: bool,
        slot: fn(&mut V) -> Option<&mut MediaPreview>,
        cx: &mut Context<V>,
    ) -> Option<Self> {
        load_saved_volume();
        let (commands, command_receiver) = mpsc::channel();
        let (sender, receiver) = async_channel::unbounded();
        // Queued before the engine exists; the player applies it as it starts.
        let volume = f64::from_bits(MEDIA_VOLUME.load(Ordering::Relaxed));
        commands
            .send(Command::Volume(effective_volume(volume, MEDIA_MUTED.load(Ordering::Relaxed))))
            .ok();
        let rate = f64::from_bits(MEDIA_RATE.load(Ordering::Relaxed));
        if rate != 1.0 {
            commands.send(Command::Rate(rate)).ok();
        }
        if is_audio(&path) {
            // Its own reader on its own thread: playback starts at once and
            // the waveform fills in beside it. Both end with the preview,
            // when the receiver goes and their sends start failing.
            let (path, sender) = (path.clone(), sender.clone());
            let spawned = std::thread::Builder::new()
                .name("prime-files-waveform".into())
                .spawn(move || platform::waveform(&path, &sender));
            if let Err(error) = spawned {
                tracing::warn!("could not start reading the waveform: {error}");
            }
        }
        let spawned = std::thread::Builder::new()
            .name("prime-files-media".into())
            .spawn(move || platform::run(&path, &command_receiver, &sender));
        if let Err(error) = spawned {
            tracing::error!("could not start the media player: {error}");
            return None;
        }
        let task = cx.spawn(async move |this, cx| {
            while let Ok(update) = receiver.recv().await {
                let alive = this.update(cx, |view, cx| {
                    let Some(media) = slot(view) else {
                        return false;
                    };
                    match update {
                        Update::Frame(frame) => {
                            // The atlas keeps a copy of every image drawn;
                            // without this each frame would stay in it.
                            if let Some(old) = media.frame.replace(Arc::new(frame)) {
                                cx.drop_image(old, None);
                            }
                        }
                        Update::State { position, duration, playing, has_video } => {
                            media.position = position;
                            media.duration = duration;
                            media.playing = playing;
                            media.has_video = has_video;
                        }
                        Update::Peaks(stretch) => {
                            media.peak_max = stretch.iter().fold(media.peak_max, |max, &p| max.max(p));
                            Arc::make_mut(&mut media.peaks).extend(stretch);
                        }
                        Update::Preparing(progress) => media.preparing = progress,
                        Update::AudioStreams(streams) => media.audio_streams = streams,
                        Update::Failed(error) => {
                            media.preparing = None;
                            media.error = Some(error);
                        }
                    }
                    cx.notify();
                    true
                });
                if !matches!(alive, Ok(true)) {
                    break;
                }
            }
        });
        let media = MediaPreview {
            commands,
            frame: None,
            position: 0.0,
            duration: 0.0,
            playing: autoplay,
            has_video: false,
            error: None,
            preparing: None,
            peaks: Arc::default(),
            peak_max: 0.0,
            rate,
            audio_streams: Vec::new(),
            menu: None,
            _task: task,
        };
        // The engine takes Play before the file is open and starts once it
        // is, so autoplay needs no wait for the first state.
        if autoplay {
            media.send(Command::Play);
        }
        Some(media)
    }

    /// Plays or pauses.
    pub(crate) fn toggle(&mut self) {
        self.playing = !self.playing;
        self.send(if self.playing { Command::Play } else { Command::Pause });
    }

    /// Jumps to `fraction` (0..=1) of the length, once it is known.
    pub(crate) fn seek(&mut self, fraction: f64) {
        if self.duration > 0.0 {
            self.position = fraction.clamp(0.0, 1.0) * self.duration;
            self.send(Command::Seek(self.position));
        }
    }

    /// Plays at `rate` times the file's speed, this file and the next.
    pub(crate) fn set_rate(&mut self, rate: f64) {
        self.rate = rate.clamp(RATES[0], RATES[RATES.len() - 1]);
        MEDIA_RATE.store(self.rate.to_bits(), Ordering::Relaxed);
        self.send(Command::Rate(self.rate));
    }

    /// Turns the audio stream at `index` (the engine's) on or off; the ones
    /// on play together.
    pub(crate) fn toggle_audio_stream(&mut self, index: u32) {
        if let Some(stream) = self.audio_streams.iter_mut().find(|stream| stream.index == index) {
            stream.enabled = !stream.enabled;
        }
        let enabled = self.audio_streams.iter().filter(|stream| stream.enabled).map(|stream| stream.index).collect();
        self.send(Command::AudioStreams(enabled));
    }

    /// Stops the player and frees its last frame.
    pub(crate) fn close(self, cx: &mut gpui::App) {
        if let Some(frame) = self.frame {
            cx.drop_image(frame, None);
        }
    }
}

impl ExplorerPane {
    /// Opens `path` in the preview's player, paused on its first frame.
    pub(crate) fn open_media_preview(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(media) = MediaPreview::start(path, self.preview_autoplay, |pane: &mut Self| pane.preview_media.as_mut(), cx)
        else {
            return;
        };
        self.preview_media = Some(media);
        cx.notify();
    }

    /// Stops the player and frees its last frame, when the preview moves on.
    pub(crate) fn close_media_preview(&mut self, cx: &mut gpui::App) {
        if let Some(media) = self.preview_media.take() {
            media.close(cx);
        }
    }

    /// Plays or pauses the previewed media.
    pub(crate) fn toggle_media(&mut self, cx: &mut Context<Self>) {
        if let Some(media) = self.preview_media.as_mut() {
            media.toggle();
            cx.notify();
        }
    }

    /// Jumps the previewed media to `fraction` (0..=1) of its length.
    pub(crate) fn seek_media(&mut self, fraction: f64, cx: &mut Context<Self>) {
        if let Some(media) = self.preview_media.as_mut()
            && media.duration > 0.0
        {
            media.seek(fraction);
            cx.notify();
        }
    }

    /// Plays the previewed media at `rate` times its speed.
    pub(crate) fn set_media_rate(&mut self, rate: f64, cx: &mut Context<Self>) {
        if let Some(media) = self.preview_media.as_mut() {
            media.set_rate(rate);
            media.menu = None;
            cx.notify();
        }
    }

    /// Turns one of the previewed file's audio streams on or off. The menu
    /// stays open, so several can be switched in a row.
    pub(crate) fn toggle_media_audio_stream(&mut self, index: u32, cx: &mut Context<Self>) {
        if let Some(media) = self.preview_media.as_mut() {
            media.toggle_audio_stream(index);
            cx.notify();
        }
    }

    /// Opens `menu` of the player bar, or closes it if it is the one open;
    /// `None` closes whichever is.
    pub(crate) fn toggle_media_menu(&mut self, menu: Option<MediaMenu>, cx: &mut Context<Self>) {
        if let Some(media) = self.preview_media.as_mut() {
            media.menu = if media.menu == menu { None } else { menu };
            cx.notify();
        }
    }

    /// The preview's volume, 0..=1. One for every tab (issue #2): set in
    /// one tab, the next sound in another plays at it too.
    pub(crate) fn media_volume(&self) -> f64 {
        load_saved_volume();
        f64::from_bits(MEDIA_VOLUME.load(Ordering::Relaxed))
    }

    /// Whether the preview is muted, shared by every tab like the volume.
    pub(crate) fn media_muted(&self) -> bool {
        load_saved_volume();
        MEDIA_MUTED.load(Ordering::Relaxed)
    }

    /// Mutes or unmutes the previewed media.
    pub(crate) fn toggle_media_mute(&mut self, cx: &mut Context<Self>) {
        let muted = !self.media_muted();
        MEDIA_MUTED.store(muted, Ordering::Relaxed);
        // Unmuting a slider left at zero would still be silent.
        if !muted && self.media_volume() <= 0.0 {
            MEDIA_VOLUME.store(0.5_f64.to_bits(), Ordering::Relaxed);
        }
        self.send_media_volume(cx);
    }

    /// Sets the volume to `volume` (0..=1), unmuting.
    pub(crate) fn set_media_volume(&mut self, volume: f64, cx: &mut Context<Self>) {
        MEDIA_VOLUME.store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        MEDIA_MUTED.store(false, Ordering::Relaxed);
        self.send_media_volume(cx);
    }

    fn send_media_volume(&mut self, cx: &mut Context<Self>) {
        save_volume(self.media_volume(), self.media_muted(), cx);
        if let Some(media) = self.preview_media.as_ref() {
            media.send(Command::Volume(effective_volume(self.media_volume(), self.media_muted())));
        }
        cx.notify();
    }
}

/// Heights (0..=1) of `bars` waveform bars spread over `duration` seconds,
/// each the loudest peak in its stretch. `None` for a stretch not read yet.
pub(crate) fn waveform_bars(peaks: &[f32], peak_max: f32, duration: f64, bars: usize) -> Vec<Option<f32>> {
    // Until the engine knows the length, what has been read fills the width.
    let blocks = if duration > 0.0 {
        (duration * PEAK_BLOCKS_PER_SECOND as f64).ceil() as usize
    } else {
        peaks.len()
    };
    if bars == 0 || blocks == 0 {
        return Vec::new();
    }
    let scale = if peak_max > 0.0 { 1.0 / peak_max } else { 0.0 };
    (0..bars)
        .map(|bar| {
            let start = bar * blocks / bars;
            let end = ((bar + 1) * blocks / bars).max(start + 1);
            let stretch = peaks.get(start..end.min(peaks.len())).filter(|s| !s.is_empty())?;
            Some((stretch.iter().fold(0.0f32, |max, &p| max.max(p)) * scale).min(1.0))
        })
        .collect()
}

/// `1:05` or `1:02:05`.
pub(crate) fn clock(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    let (hours, minutes, seconds) = (total / 3600, total / 60 % 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod platform {
    //! The Media Foundation engine. `unsafe` is confined to this module: none
    //! of these interfaces has a safe binding.

    use super::{AudioStream, Command, MAX_FRAME, PEAK_BLOCKS_PER_SECOND, Update};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Imaging::{
        CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICBitmap, IWICImagingFactory,
        WICBitmapCacheOnLoad, WICBitmapLockRead, WICRect,
    };
    use windows::Win32::Media::MediaFoundation::{
        CLSID_MFMediaEngineClassFactory, IMFAttributes, IMFMediaEngine, IMFMediaEngineClassFactory,
        IMFMediaEngineEx, IMFMediaEngineNotify, IMFMediaEngineNotify_Impl, IMFSample,
        MF_MEDIA_ENGINE_AUDIOONLY, MF_MEDIA_ENGINE_CALLBACK, MF_MEDIA_ENGINE_READY_HAVE_METADATA,
        MF_SD_LANGUAGE, MF_SD_STREAM_NAME,
        MF_MEDIA_ENGINE_VIDEO_OUTPUT_FORMAT, MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND,
        MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_SOURCE_READER_ALL_STREAMS,
        MF_SOURCE_READER_FIRST_AUDIO_STREAM, MF_SOURCE_READERF_ENDOFSTREAM, MF_VERSION, MFARGB,
        MFAudioFormat_Float, MFCreateAttributes, MFCreateMediaType, MFCreateSourceReaderFromURL,
        MFMediaType_Audio, MFSTARTUP_FULL, MFShutdown, MFStartup,
    };
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
    };
    use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
    use windows::Win32::System::Variant::VT_CLSID;
    use windows::core::{BSTR, GUID, HSTRING, Interface, implement};

    // DXGI_FORMAT_B8G8R8A8_UNORM, the frame format asked of the engine: the
    // byte order gpui's images use, so frames need no swizzling.
    const B8G8R8A8: u32 = 87;

    // The engine insists on a listener; the player polls its state instead,
    // which keeps everything on this one thread.
    #[implement(IMFMediaEngineNotify)]
    struct Listener;

    impl IMFMediaEngineNotify_Impl for Listener_Impl {
        fn EventNotify(&self, _event: u32, _param1: usize, _param2: u32) -> windows::core::Result<()> {
            Ok(())
        }
    }

    pub fn run(path: &str, commands: &mpsc::Receiver<Command>, updates: &async_channel::Sender<Update>) {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok().ok();
        if let Err(error) = unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) } {
            updates.send_blocking(Update::Failed(error.message().to_string())).ok();
            return;
        }
        let mut wanted = Wanted::default();
        let video = !super::is_audio(path);
        if let Err(error) = play(path, video, commands, updates, &mut wanted) {
            // A video Windows cannot decode (ProRes, Animation...): a copy
            // FFmpeg makes plays instead, with what was asked meanwhile.
            if error.code() == CLOSED {
                // The preview closed: nothing to say.
            } else if video {
                match convert(path, commands, updates, &mut wanted) {
                    Some(Ok(copy)) => {
                        updates.send_blocking(Update::Preparing(None)).ok();
                        let copy = copy.to_string_lossy().into_owned();
                        if let Err(error) = play(&copy, true, commands, updates, &mut wanted)
                            && error.code() != CLOSED
                        {
                            updates.send_blocking(Update::Failed(error.message().to_string())).ok();
                        }
                    }
                    Some(Err(message)) => {
                        updates.send_blocking(Update::Failed(message)).ok();
                    }
                    // Closed while converting.
                    None => {}
                }
            } else {
                updates.send_blocking(Update::Failed(error.message().to_string())).ok();
            }
        }
        unsafe { MFShutdown() }.ok();
    }

    // What `play` returns when the preview closed: not a failure, and not
    // worth converting over.
    const CLOSED: windows::core::HRESULT = windows::core::HRESULT(0x8000_4004_u32 as i32);

    /// What the preview asked of the player, carried from a failed open to
    /// the copy that plays instead.
    #[derive(Default)]
    struct Wanted {
        play: bool,
        volume: Option<f64>,
        seek: Option<f64>,
        rate: Option<f64>,
        /// The audio streams to hear; `None` for the file's own choice.
        audio: Option<Vec<u32>>,
    }

    impl Wanted {
        fn take(&mut self, command: Command) {
            match command {
                Command::Play => self.play = true,
                Command::Pause => self.play = false,
                Command::Volume(volume) => self.volume = Some(volume),
                Command::Seek(seconds) => self.seek = Some(seconds),
                Command::Rate(rate) => self.rate = Some(rate),
                Command::AudioStreams(streams) => self.audio = Some(streams),
            }
        }
    }

    // Converts `path` with FFmpeg, saying how far it got. `None` when the
    // preview closed meanwhile; commands that come are kept for the copy.
    fn convert(
        path: &str,
        commands: &mpsc::Receiver<Command>,
        updates: &async_channel::Sender<Update>,
        wanted: &mut Wanted,
    ) -> Option<Result<std::path::PathBuf, String>> {
        let closed = std::cell::Cell::new(false);
        let wanted = std::cell::RefCell::new(wanted);
        let cancelled = || {
            loop {
                match commands.try_recv() {
                    Ok(command) => wanted.borrow_mut().take(command),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        closed.set(true);
                        break;
                    }
                }
            }
            closed.get() || updates.is_closed()
        };
        let result = crate::explorer::video_proxy::make(path, &cancelled, &mut |progress| {
            updates.send_blocking(Update::Preparing(Some(progress))).ok();
        });
        if closed.get() || updates.is_closed() {
            return None;
        }
        Some(result)
    }

    // The copy plays as made: a sound-only original stays sound-only there.
    fn path_is_proxy(path: &str) -> bool {
        std::path::Path::new(path).starts_with(prime_files_core::config::paths::cache_dir())
    }

    // Plays `path` until the preview closes. `video`: the file is a video,
    // so one that opens with a length and no picture failed (Windows read
    // its sound and not its images), and the caller converts it.
    fn play(
        path: &str,
        video: bool,
        commands: &mpsc::Receiver<Command>,
        updates: &async_channel::Sender<Update>,
        wanted: &mut Wanted,
    ) -> windows::core::Result<()> {
        let engine = unsafe {
            let factory: IMFMediaEngineClassFactory =
                CoCreateInstance(&CLSID_MFMediaEngineClassFactory, None, CLSCTX_INPROC_SERVER)?;
            let mut attributes: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut attributes, 2)?;
            let attributes = attributes.ok_or_else(windows::core::Error::empty)?;
            let listener: IMFMediaEngineNotify = Listener.into();
            attributes.SetUnknown(&MF_MEDIA_ENGINE_CALLBACK, &listener)?;
            attributes.SetUINT32(&MF_MEDIA_ENGINE_VIDEO_OUTPUT_FORMAT, B8G8R8A8)?;
            let engine: IMFMediaEngine = factory.CreateInstance(0, &attributes)?;
            crate::explorer::audio_session::watch();
            engine.SetSource(&BSTR::from(path))?;
            // What was asked before this engine existed (during a failed
            // first open, or the conversion after it).
            if let Some(volume) = wanted.volume {
                engine.SetVolume(volume).ok();
            }
            if let Some(seconds) = wanted.seek {
                engine.SetCurrentTime(seconds).ok();
            }
            if let Some(rate) = wanted.rate {
                set_rate(&engine, rate);
            }
            if wanted.play {
                engine.Play().ok();
            }
            engine
        };
        let wic: IWICImagingFactory =
            unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)? };
        let mut target: Option<(IWICBitmap, u32, u32)> = None;
        let mut last_frame = i64::MIN;
        let mut last_state = Instant::now() - Duration::from_secs(1);
        // The file's audio streams (the engine's indices), once it has read them.
        let mut streams: Option<Vec<u32>> = None;
        let mut extras: Vec<Extra> = Vec::new();
        // Every stream switched off: the main engine keeps one selected and
        // plays it at no volume, rather than playing with no sound at all,
        // which an audio file's engine would not keep time through.
        let mut silent = false;
        let mut last_drift = Instant::now();
        let result = loop {
            loop {
                match commands.try_recv() {
                    Ok(Command::Play) => unsafe {
                        wanted.play = true;
                        // From the end, play starts over, as players do.
                        if engine.IsEnded().as_bool() {
                            engine.SetCurrentTime(0.0).ok();
                        }
                        engine.Play().ok();
                    },
                    Ok(Command::Pause) => {
                        wanted.play = false;
                        unsafe { engine.Pause() }.unwrap_or(())
                    }
                    Ok(Command::Seek(seconds)) => {
                        wanted.seek = Some(seconds);
                        unsafe { engine.SetCurrentTime(seconds) }.unwrap_or(())
                    }
                    Ok(Command::Volume(volume)) => {
                        wanted.volume = Some(volume);
                        unsafe { engine.SetVolume(if silent { 0.0 } else { volume }) }.unwrap_or(());
                        for extra in extras.iter().filter(|extra| extra.ready) {
                            unsafe { extra.engine.SetVolume(volume) }.ok();
                        }
                    }
                    Ok(Command::Rate(rate)) => {
                        wanted.rate = Some(rate);
                        set_rate(&engine, rate);
                        for extra in &extras {
                            set_rate(&extra.engine, rate);
                        }
                    }
                    Ok(Command::AudioStreams(chosen)) => {
                        if let Some(all) = &streams {
                            silent = select_audio(&engine, path, all, &chosen, &mut extras);
                            let volume = wanted.volume.unwrap_or(1.0);
                            unsafe { engine.SetVolume(if silent { 0.0 } else { volume }) }.ok();
                        }
                        wanted.audio = Some(chosen);
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    // The preview was closed.
                    Err(mpsc::TryRecvError::Disconnected) => {
                        unsafe { engine.Shutdown() }.ok();
                        return Err(windows::core::Error::new(CLOSED, ""));
                    }
                }
            }
            if let Ok(error) = unsafe { engine.GetError() }
                && unsafe { error.GetErrorCode() } != 0
            {
                break Err(windows::core::Error::new(
                    // 0.61 hands the extended code back as a `Result`, the
                    // failure HRESULT riding in its `Err`.
                    windows::core::HRESULT::from(unsafe { error.GetExtendedErrorCode() }),
                    prime_files_core::t!("Windows não consegue reproduzir este arquivo"),
                ));
            }
            if streams.is_none()
                && unsafe { engine.GetReadyState() } >= MF_MEDIA_ENGINE_READY_HAVE_METADATA.0 as u16
            {
                let mut list = engine.cast::<IMFMediaEngineEx>().map(|ex| audio_streams(&ex)).unwrap_or_default();
                let all: Vec<u32> = list.iter().map(|stream| stream.index).collect();
                if all.len() > 1 {
                    // What the preview asked for, or what the file plays by
                    // default (its first stream when it says nothing), laid
                    // out the player's way: one stream per engine.
                    let chosen = wanted.audio.clone().unwrap_or_else(|| {
                        let on: Vec<u32> =
                            list.iter().filter(|stream| stream.enabled).map(|stream| stream.index).collect();
                        if on.is_empty() { all[..1].to_vec() } else { on }
                    });
                    silent = select_audio(&engine, path, &all, &chosen, &mut extras);
                    let volume = wanted.volume.unwrap_or(1.0);
                    unsafe { engine.SetVolume(if silent { 0.0 } else { volume }) }.ok();
                    for stream in &mut list {
                        stream.enabled = chosen.contains(&stream.index);
                    }
                }
                streams = Some(all);
                if updates.send_blocking(Update::AudioStreams(list)).is_err() {
                    break Ok(());
                }
            }
            let check_drift = last_drift.elapsed() >= Duration::from_millis(250);
            if check_drift {
                last_drift = Instant::now();
            }
            sync_extras(&engine, &mut extras, wanted.volume.unwrap_or(1.0), wanted.rate.unwrap_or(1.0), check_drift);
            let has_video = unsafe { engine.HasVideo() }.as_bool();
            // Opened, with a length, and no picture: a video whose images
            // Windows cannot decode, played as sound only.
            if video && !has_video && unsafe { engine.GetDuration() } > 0.0 && !path_is_proxy(path) {
                break Err(windows::core::Error::new(
                    windows::core::HRESULT(0xC00D_5212_u32 as i32),
                    prime_files_core::t!("Windows não consegue reproduzir este arquivo"),
                ));
            }
            if has_video
                && let Ok(time) = unsafe { engine.OnVideoStreamTick() }
                && time != last_frame
                && let Some(frame) = grab(&engine, &wic, &mut target)
            {
                last_frame = time;
                if updates.send_blocking(Update::Frame(frame)).is_err() {
                    break Ok(());
                }
            }
            if last_state.elapsed() >= Duration::from_millis(100) {
                last_state = Instant::now();
                let state = unsafe {
                    Update::State {
                        position: engine.GetCurrentTime(),
                        duration: engine.GetDuration().max(0.0),
                        playing: !engine.IsPaused().as_bool() && !engine.IsEnded().as_bool(),
                        has_video,
                    }
                };
                if updates.send_blocking(state).is_err() {
                    break Ok(());
                }
            }
            // About one vsync: frames are not missed and the thread idles.
            std::thread::sleep(Duration::from_millis(8));
        };
        unsafe { engine.Shutdown() }.ok();
        result
    }

    // The speed on both of the engine's rates: `Play` resets the current
    // one to the default, as an HTML player does.
    fn set_rate(engine: &IMFMediaEngine, rate: f64) {
        unsafe {
            engine.SetDefaultPlaybackRate(rate).ok();
            engine.SetPlaybackRate(rate).ok();
        }
    }

    /// How far, in seconds, an extra stream may drift from the main one
    /// before it is put back: past about a tenth of a second the two are
    /// heard as an echo.
    const DRIFT: f64 = 0.12;

    /// One more of the file's audio streams, on an engine of its own beside
    /// the main one; Windows' audio mixes the two. One engine per stream
    /// because the engine is only documented to play one audio stream at a
    /// time, and a second selected one was not something to bet the mix on.
    struct Extra {
        index: u32,
        engine: IMFMediaEngine,
        /// Its stream selected and its clock set to the main one's.
        ready: bool,
    }

    impl Drop for Extra {
        fn drop(&mut self) {
            unsafe { self.engine.Shutdown() }.ok();
        }
    }

    // A sound-only engine for `path`, silent until `sync_extras` has
    // selected its stream: the file's default one would play meanwhile.
    fn audio_engine(path: &str) -> windows::core::Result<IMFMediaEngine> {
        unsafe {
            let factory: IMFMediaEngineClassFactory =
                CoCreateInstance(&CLSID_MFMediaEngineClassFactory, None, CLSCTX_INPROC_SERVER)?;
            let mut attributes: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut attributes, 1)?;
            let attributes = attributes.ok_or_else(windows::core::Error::empty)?;
            let listener: IMFMediaEngineNotify = Listener.into();
            attributes.SetUnknown(&MF_MEDIA_ENGINE_CALLBACK, &listener)?;
            let engine: IMFMediaEngine = factory.CreateInstance(MF_MEDIA_ENGINE_AUDIOONLY.0 as u32, &attributes)?;
            engine.SetVolume(0.0)?;
            engine.SetSource(&BSTR::from(path))?;
            Ok(engine)
        }
    }

    // The file's audio streams with what the file says of each. `enabled`
    // is whether the engine chose it by default.
    fn audio_streams(engine: &IMFMediaEngineEx) -> Vec<AudioStream> {
        let count = unsafe { engine.GetNumberOfStreams() }.unwrap_or(0);
        (0..count)
            .filter_map(|index| {
                let attribute = |guid: GUID| unsafe { engine.GetStreamAttribute(index, &guid) }.ok();
                let channels = attribute(MF_MT_AUDIO_NUM_CHANNELS).and_then(|value| u32::try_from(&value).ok());
                let audio = match attribute(MF_MT_MAJOR_TYPE).as_ref().and_then(guid_of) {
                    Some(major) => major == MFMediaType_Audio,
                    // No major type said: only a sound stream has channels.
                    None => channels.is_some(),
                };
                if !audio {
                    return None;
                }
                let text = |guid: GUID| {
                    attribute(guid)
                        .and_then(|value| BSTR::try_from(&value).ok())
                        .map(|text| text.to_string())
                        .filter(|text| !text.trim().is_empty())
                };
                Some(AudioStream {
                    index,
                    name: text(MF_SD_STREAM_NAME),
                    language: text(MF_SD_LANGUAGE),
                    channels,
                    enabled: unsafe { engine.GetStreamSelection(index) }.is_ok_and(|on| on.as_bool()),
                })
            })
            .collect()
    }

    fn guid_of(value: &PROPVARIANT) -> Option<GUID> {
        if value.vt() != VT_CLSID {
            return None;
        }
        // A VT_CLSID holds a pointer to its GUID, owned by `value`.
        let pointer = unsafe { value.Anonymous.Anonymous.Anonymous.puuid };
        (!pointer.is_null()).then(|| unsafe { *pointer })
    }

    // Plays `chosen` of the file's `all` audio streams: the first on the main
    // engine, each other on an extra one. True when none is chosen, and the
    // main engine is to play its stream silently.
    fn select_audio(
        engine: &IMFMediaEngine,
        path: &str,
        all: &[u32],
        chosen: &[u32],
        extras: &mut Vec<Extra>,
    ) -> bool {
        let chosen: Vec<u32> = chosen.iter().copied().filter(|index| all.contains(index)).collect();
        let main = chosen.first().or(all.first()).copied();
        if let Ok(ex) = engine.cast::<IMFMediaEngineEx>() {
            for &index in all {
                unsafe { ex.SetStreamSelection(index, Some(index) == main) }.ok();
            }
            unsafe { ex.ApplyStreamSelections() }.ok();
        }
        let more = chosen.get(1..).unwrap_or_default();
        extras.retain(|extra| more.contains(&extra.index));
        for &index in more {
            if extras.iter().any(|extra| extra.index == index) {
                continue;
            }
            match audio_engine(path) {
                Ok(engine) => extras.push(Extra { index, engine, ready: false }),
                Err(error) => tracing::warn!("could not play audio stream {index} of {path}: {error}"),
            }
        }
        chosen.is_empty()
    }

    // Keeps the extra streams with the main one: playing or paused as it
    // is, at its speed, and at its place once they drift from it.
    fn sync_extras(engine: &IMFMediaEngine, extras: &mut [Extra], volume: f64, rate: f64, check_drift: bool) {
        if extras.is_empty() {
            return;
        }
        unsafe {
            let time = engine.GetCurrentTime();
            let playing = !engine.IsPaused().as_bool() && !engine.IsEnded().as_bool();
            let seeking = engine.IsSeeking().as_bool();
            for extra in extras {
                let other = &extra.engine;
                if !extra.ready {
                    if other.GetReadyState() < MF_MEDIA_ENGINE_READY_HAVE_METADATA.0 as u16 {
                        continue;
                    }
                    if let Ok(ex) = other.cast::<IMFMediaEngineEx>() {
                        for index in 0..ex.GetNumberOfStreams().unwrap_or(0) {
                            ex.SetStreamSelection(index, index == extra.index).ok();
                        }
                        ex.ApplyStreamSelections().ok();
                    }
                    set_rate(other, rate);
                    other.SetCurrentTime(time).ok();
                    other.SetVolume(volume).ok();
                    extra.ready = true;
                }
                let paused = other.IsPaused().as_bool();
                if playing && paused {
                    other.SetCurrentTime(time).ok();
                    other.Play().ok();
                } else if !playing && !paused {
                    other.Pause().ok();
                } else if check_drift
                    && !seeking
                    && !other.IsSeeking().as_bool()
                    && (other.GetCurrentTime() - time).abs() > DRIFT
                {
                    other.SetCurrentTime(time).ok();
                }
            }
        }
    }

    /// Reads `path`'s sound from start to end and sends its peaks, a stretch
    /// at a time, until the file ends or the preview is closed.
    pub fn waveform(path: &str, updates: &async_channel::Sender<Update>) {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok().ok();
        if unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }.is_err() {
            return;
        }
        // No waveform is not worth an error over the player: it just stays empty.
        if let Err(error) = read_peaks(path, updates) {
            tracing::debug!("no waveform for {path}: {error}");
        }
        unsafe { MFShutdown() }.ok();
    }

    fn read_peaks(path: &str, updates: &async_channel::Sender<Update>) -> windows::core::Result<()> {
        let stream = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;
        let reader = unsafe {
            let reader = MFCreateSourceReaderFromURL(&HSTRING::from(path), None)?;
            reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
            reader.SetStreamSelection(stream, true)?;
            // Decoded to float PCM, whatever the file holds: one sample
            // format to take peaks of.
            let wanted = MFCreateMediaType()?;
            wanted.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
            wanted.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_Float)?;
            reader.SetCurrentMediaType(stream, None, &wanted)?;
            reader
        };
        let format = unsafe { reader.GetCurrentMediaType(stream)? };
        let channels = unsafe { format.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS)? }.max(1) as usize;
        let rate = unsafe { format.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND)? }.max(1) as usize;
        // Every channel's samples of a block count toward its one peak.
        let per_block = (rate / PEAK_BLOCKS_PER_SECOND).max(1) * channels;
        let (mut stretch, mut peak, mut filled) = (Vec::new(), 0.0f32, 0usize);
        let mut last_sent = Instant::now();
        loop {
            let mut flags = 0u32;
            let mut sample: Option<IMFSample> = None;
            unsafe { reader.ReadSample(stream, 0, None, Some(&mut flags), None, Some(&mut sample))? };
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                break;
            }
            let Some(sample) = sample else { continue };
            let buffer = unsafe { sample.ConvertToContiguousBuffer()? };
            let (mut data, mut length) = (std::ptr::null_mut::<u8>(), 0u32);
            unsafe { buffer.Lock(&mut data, None, Some(&mut length))? };
            if !data.is_null() {
                // Float PCM, as asked; Media Foundation buffers are aligned
                // for it. Valid until the `Unlock` below.
                let samples = unsafe { std::slice::from_raw_parts(data as *const f32, length as usize / 4) };
                for &value in samples {
                    peak = peak.max(value.abs());
                    filled += 1;
                    if filled == per_block {
                        stretch.push(peak);
                        (peak, filled) = (0.0, 0);
                    }
                }
            }
            unsafe { buffer.Unlock()? };
            // A few stretches a second: the waveform visibly fills in
            // without a message per decoded buffer.
            if last_sent.elapsed() >= Duration::from_millis(200) && !stretch.is_empty() {
                last_sent = Instant::now();
                if updates.send_blocking(Update::Peaks(std::mem::take(&mut stretch))).is_err() {
                    return Ok(());
                }
            }
        }
        if filled > 0 {
            stretch.push(peak);
        }
        updates.send_blocking(Update::Peaks(stretch)).ok();
        Ok(())
    }

    // Copies the engine's current frame into a bitmap no larger than
    // `MAX_FRAME`, keeping the aspect ratio, and returns it as an image.
    fn grab(
        engine: &IMFMediaEngine,
        wic: &IWICImagingFactory,
        target: &mut Option<(IWICBitmap, u32, u32)>,
    ) -> Option<gpui::RenderImage> {
        let (mut width, mut height) = (0u32, 0u32);
        unsafe { engine.GetNativeVideoSize(Some(&mut width), Some(&mut height)) }.ok()?;
        if width == 0 || height == 0 {
            return None;
        }
        let scale = (MAX_FRAME.0 as f64 / width as f64).min(MAX_FRAME.1 as f64 / height as f64).min(1.0);
        let (width, height) = (((width as f64 * scale) as u32).max(1), ((height as f64 * scale) as u32).max(1));
        if !matches!(target, Some((_, w, h)) if *w == width && *h == height) {
            let bitmap = unsafe {
                wic.CreateBitmap(width, height, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnLoad)
            }
            .ok()?;
            *target = Some((bitmap, width, height));
        }
        let (bitmap, _, _) = target.as_ref()?;
        let rect = RECT { left: 0, top: 0, right: width as i32, bottom: height as i32 };
        let black = MFARGB { rgbBlue: 0, rgbGreen: 0, rgbRed: 0, rgbAlpha: 255 };
        unsafe { engine.TransferVideoFrame(bitmap, None, &rect, Some(&black)) }.ok()?;
        let lock = unsafe {
            bitmap.Lock(
                &WICRect { X: 0, Y: 0, Width: width as i32, Height: height as i32 },
                WICBitmapLockRead.0 as u32,
            )
        }
        .ok()?;
        let stride = unsafe { lock.GetStride() }.ok()? as usize;
        let (mut size, mut data) = (0u32, std::ptr::null_mut::<u8>());
        unsafe { lock.GetDataPointer(&mut size, &mut data) }.ok()?;
        if data.is_null() {
            return None;
        }
        // The lock's buffer, valid until `lock` drops at the end of this function.
        let buffer = unsafe { std::slice::from_raw_parts(data, size as usize) };
        let row = width as usize * 4;
        let mut bgra = Vec::with_capacity(row * height as usize);
        for line in 0..height as usize {
            bgra.extend_from_slice(buffer.get(line * stride..line * stride + row)?);
        }
        crate::explorer::thumbnails::render_image(width, height, bgra)
    }
}

#[cfg(not(windows))]
mod platform {
    use super::{Command, Update};
    use std::sync::mpsc;

    pub fn run(_path: &str, _commands: &mpsc::Receiver<Command>, updates: &async_channel::Sender<Update>) {
        updates.send_blocking(Update::Failed("Playback is only available on Windows".into())).ok();
    }

    pub fn waveform(_path: &str, _updates: &async_channel::Sender<Update>) {}
}


/// The volume a sound plays at outside a pane, 0 when muted: the settings
/// page's samples sound as loud as the files do.
pub(crate) fn shared_volume() -> f64 {
    load_saved_volume();
    if MEDIA_MUTED.load(Ordering::Relaxed) {
        0.0
    } else {
        f64::from_bits(MEDIA_VOLUME.load(Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use super::{PEAK_BLOCKS_PER_SECOND, clock, effective_volume, is_audio, is_media, waveform_bars};

    #[test]
    fn bars_take_the_loudest_peak_of_their_stretch_scaled_to_the_file() {
        // Two seconds known, one read: the second half has nothing to draw.
        let mut peaks = vec![0.1; PEAK_BLOCKS_PER_SECOND];
        peaks[10] = 0.5;
        let bars = waveform_bars(&peaks, 0.5, 2.0, 4);
        assert_eq!(bars, [Some(1.0), Some(0.2), None, None]);
        // With no length yet, what has been read fills every bar.
        assert!(waveform_bars(&peaks, 0.5, 0.0, 4).iter().all(Option::is_some));
        assert!(waveform_bars(&[], 0.0, 0.0, 4).is_empty());
    }

    #[test]
    fn muting_silences_without_forgetting_the_volume() {
        assert_eq!(effective_volume(0.7, false), 0.7);
        assert_eq!(effective_volume(0.7, true), 0.0);
        assert!(is_audio("a.MP3") && !is_audio("a.mp4") && is_media("a.mp4"));
    }

    #[test]
    fn clock_reads_like_a_player() {
        assert_eq!(clock(5.4), "0:05");
        assert_eq!(clock(65.0), "1:05");
        assert_eq!(clock(3725.0), "1:02:05");
    }

    #[test]
    fn speeds_and_streams_read_like_a_player() {
        use super::{AudioStream, RATES, rate_label, stream_label};
        assert_eq!(RATES.map(rate_label), ["0.5×", "0.75×", "1×", "1.25×", "1.5×", "2×"]);
        let stream = AudioStream {
            index: 3,
            name: Some("Commentary".into()),
            language: Some("en".into()),
            channels: Some(6),
            enabled: true,
        };
        assert!(stream_label(1, &stream).ends_with(" · Commentary · en · 5.1"));
        // A stream that says nothing of itself still has its number.
        let bare = AudioStream { name: Some(" ".into()), language: None, channels: None, ..stream };
        assert!(!stream_label(0, &bare).contains('·'));
    }

    #[test]
    fn media_is_recognized_by_extension() {
        assert!(is_media("C:\\v\\clip.MP4"));
        assert!(is_media("song.flac"));
        assert!(!is_media("notes.txt"));
    }
}

// The preview's volume and mute survive a restart (issue #18), in a file of
// their own beside the mixer's (`audio_session.rs`): `0.300 1`.
fn volume_file() -> std::path::PathBuf {
    prime_files_core::config::paths::data_dir().join("preview-volume")
}

// Reads the kept volume the first time it is asked for. A small file read
// once per run; the slider moves the in-memory value from then on.
#[allow(clippy::disallowed_methods)]
fn load_saved_volume() {
    static LOADED: std::sync::Once = std::sync::Once::new();
    LOADED.call_once(|| {
        let Ok(text) = std::fs::read_to_string(volume_file()) else {
            return;
        };
        let mut parts = text.split_whitespace();
        if let Some(volume) = parts.next().and_then(|volume| volume.parse::<f64>().ok())
            && (0.0..=1.0).contains(&volume)
        {
            MEDIA_VOLUME.store(volume.to_bits(), Ordering::Relaxed);
        }
        MEDIA_MUTED.store(parts.next() == Some("1"), Ordering::Relaxed);
    });
}

fn save_volume(volume: f64, muted: bool, cx: &mut Context<ExplorerPane>) {
    let text = format!("{volume:.3} {}", u8::from(muted));
    cx.background_executor().spawn(async move {
        let path = volume_file();
        let written = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| {
                #[allow(clippy::disallowed_methods)]
                std::fs::write(&path, text)
            });
        if let Err(error) = written {
            tracing::warn!("could not keep the preview volume: {error}");
        }
    })
    .detach();
}
