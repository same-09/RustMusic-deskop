//! FLAC engine: download to a temp file while decoding from it, play at the file's own
//! rate and bit depth, seek by decoding forward (or by the seek table once all of it has
//! arrived).

use std::fs::{self, File};
use std::io::{self, ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering::SeqCst};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::Duration;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{Decoder, DecoderOptions};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::default::codecs::FlacDecoder;
use symphonia::default::formats::FlacReader;

use crate::convert::{scale, to_16, Rng};
use crate::out::PcmOut;

/// Where the bytes come from. Called once, on the download thread.
pub type Source = Box<dyn Fn() -> io::Result<Box<dyn Read + Send>> + Send + Sync>;

/// What an engine tells its owner. All three may be called from the player thread.
#[derive(Clone)]
pub struct Callbacks {
    /// The first sound reached the device.
    pub on_started: Arc<dyn Fn() + Send + Sync>,
    /// The file played to its end.
    pub on_end: Arc<dyn Fn() + Send + Sync>,
    pub on_error: Arc<dyn Fn(String) + Send + Sync>,
}

impl Callbacks {
    pub fn none() -> Self {
        Callbacks {
            on_started: Arc::new(|| {}),
            on_end: Arc::new(|| {}),
            on_error: Arc::new(|_| {}),
        }
    }
}

/// Same surface for every kind of engine.
pub trait Engine {
    fn play(&self);
    fn pause(&self);
    fn seek(&self, ms: u64);
    fn position_ms(&self) -> u64;
    fn duration_ms(&self) -> u64;
    fn set_volume(&self, volume: f64);
    fn dispose(&self);
}

#[derive(Clone, Copy)]
struct Info {
    rate: u32,
    channels: u16,
    bits: u16,
    total: u64,
}

struct Inner {
    path: PathBuf,
    source: Source,
    start_at_ms: Box<dyn Fn() -> u64 + Send + Sync>,
    cb: Callbacks,
    out: Arc<dyn PcmOut>,

    lock: Mutex<()>,
    cv: Condvar,

    arrived: AtomicU64,
    complete: AtomicBool,
    failed: Mutex<Option<String>>,
    disposed: AtomicBool,
    paused: AtomicBool,

    /// Sample to jump to, or -1.
    seek_request: AtomicI64,
    /// Sample being sought, shown as the position until sound from there plays, or -1.
    seeking_to: AtomicI64,
    /// Overrides `start_at_ms` when set before the file header is known, or -1.
    early_seek_ms: AtomicI64,

    info: OnceLock<Info>,
    output_bits: AtomicU32,
    started: AtomicBool,
    /// File sample at which the sound now playing began, and the device frame count then.
    run_start: AtomicU64,
    line_base: AtomicU64,
    volume: AtomicU64,
    soft_gain: AtomicU64,
}

fn f64_bits(v: f64) -> u64 {
    v.to_bits()
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Inner {
    fn notify(&self) {
        let _g = lock(&self.lock);
        self.cv.notify_all();
    }

    fn wait(&self, ms: u64) {
        let g = lock(&self.lock);
        let _ = self.cv.wait_timeout(g, Duration::from_millis(ms));
    }

    fn take_seek(&self) -> Option<u64> {
        let r = self.seek_request.swap(-1, SeqCst);
        (r >= 0).then_some(r as u64)
    }

    fn gain(&self) -> f64 {
        f64::from_bits(self.soft_gain.load(SeqCst))
    }

    fn apply_volume(&self) {
        if self.info.get().is_none() {
            return;
        }
        let v = f64::from_bits(self.volume.load(SeqCst));
        let db = if v <= 0.0 { -80.0 } else { 20.0 * v.log10() as f32 };
        let hardware = self.out.set_gain_db(db);
        self.soft_gain.store(f64_bits(if hardware { 1.0 } else { v }), SeqCst);
    }

    fn start_sample(&self, info: Info) -> u64 {
        let early = self.early_seek_ms.load(SeqCst);
        let ms = if early >= 0 { early as u64 } else { (self.start_at_ms)() };
        (ms * info.rate as u64 / 1000).min(info.total)
    }
}

pub struct FlacEngine {
    inner: Arc<Inner>,
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

impl FlacEngine {
    /// Start fetching and decoding at once. `start_at_ms` is asked when the file's header
    /// is known, so a position that changes while the file is still arriving is honoured.
    /// With `autoplay` false the engine loads and waits for `play`.
    pub fn new(
        source: Source,
        start_at_ms: Box<dyn Fn() -> u64 + Send + Sync>,
        autoplay: bool,
        cb: Callbacks,
        out: Arc<dyn PcmOut>,
    ) -> io::Result<FlacEngine> {
        let path = std::env::temp_dir().join(format!(
            "rustmusic-{}-{}.flac",
            std::process::id(),
            COUNTER.fetch_add(1, SeqCst)
        ));
        // Created before either thread starts, so the reader always finds it.
        File::create(&path)?;
        let inner = Arc::new(Inner {
            path,
            source,
            start_at_ms,
            cb,
            out,
            lock: Mutex::new(()),
            cv: Condvar::new(),
            arrived: AtomicU64::new(0),
            complete: AtomicBool::new(false),
            failed: Mutex::new(None),
            disposed: AtomicBool::new(false),
            paused: AtomicBool::new(!autoplay),
            seek_request: AtomicI64::new(-1),
            seeking_to: AtomicI64::new(-1),
            early_seek_ms: AtomicI64::new(-1),
            info: OnceLock::new(),
            output_bits: AtomicU32::new(0),
            started: AtomicBool::new(false),
            run_start: AtomicU64::new(0),
            line_base: AtomicU64::new(0),
            volume: AtomicU64::new(f64_bits(1.0)),
            soft_gain: AtomicU64::new(f64_bits(1.0)),
        });
        let i = inner.clone();
        thread::Builder::new().name("flac-fetch".into()).spawn(move || fetch(&i))?;
        let i = inner.clone();
        thread::Builder::new().name("flac-play".into()).spawn(move || run(i))?;
        Ok(FlacEngine { inner })
    }

    pub fn sample_rate(&self) -> Option<u32> {
        self.inner.info.get().map(|i| i.rate)
    }

    pub fn bits(&self) -> Option<u16> {
        self.inner.info.get().map(|i| i.bits)
    }

    /// Bit depth the device was opened at, once known.
    pub fn output_bits(&self) -> Option<u16> {
        match self.inner.output_bits.load(SeqCst) {
            0 => None,
            b => Some(b as u16),
        }
    }

    /// Not paused.
    pub fn is_playing(&self) -> bool {
        !self.inner.paused.load(SeqCst)
    }

    /// Whether the whole file has been downloaded.
    pub fn is_complete(&self) -> bool {
        self.inner.complete.load(SeqCst)
    }
}

impl Engine for FlacEngine {
    fn play(&self) {
        let i = &self.inner;
        i.paused.store(false, SeqCst);
        if i.output_bits.load(SeqCst) != 0 {
            i.out.start();
        }
        i.notify();
    }

    fn pause(&self) {
        let i = &self.inner;
        i.paused.store(true, SeqCst);
        if i.output_bits.load(SeqCst) != 0 {
            i.out.stop();
        }
        i.notify();
    }

    fn seek(&self, ms: u64) {
        let i = &self.inner;
        match i.info.get() {
            None => {
                i.early_seek_ms.store(ms as i64, SeqCst);
            }
            Some(info) => {
                let s = (ms * info.rate as u64 / 1000).min(info.total);
                i.seeking_to.store(s as i64, SeqCst);
                i.seek_request.store(s as i64, SeqCst);
                // Cut a write that is blocked on a full device buffer.
                i.out.flush();
            }
        }
        i.notify();
    }

    fn position_ms(&self) -> u64 {
        let i = &self.inner;
        let Some(info) = i.info.get() else {
            let early = i.early_seek_ms.load(SeqCst);
            return if early >= 0 { early as u64 } else { 0 };
        };
        let seeking = i.seeking_to.load(SeqCst);
        let sample = if seeking >= 0 {
            seeking as u64
        } else if !i.started.load(SeqCst) {
            i.run_start.load(SeqCst)
        } else {
            let played = i.out.frame_position().saturating_sub(i.line_base.load(SeqCst));
            i.run_start.load(SeqCst) + played
        };
        sample.min(info.total) * 1000 / info.rate as u64
    }

    fn duration_ms(&self) -> u64 {
        self.inner.info.get().map_or(0, |i| i.total * 1000 / i.rate as u64)
    }

    fn set_volume(&self, volume: f64) {
        self.inner.volume.store(f64_bits(volume.clamp(0.0, 1.0)), SeqCst);
        self.inner.apply_volume();
    }

    fn dispose(&self) {
        let i = &self.inner;
        if i.disposed.swap(true, SeqCst) {
            return;
        }
        i.out.flush();
        i.notify();
    }
}

impl Drop for FlacEngine {
    fn drop(&mut self) {
        self.dispose();
    }
}

/// Download thread: bytes from the source into the temp file.
fn fetch(inner: &Arc<Inner>) {
    let result = (|| -> io::Result<()> {
        let mut src = (inner.source)()?;
        let mut file = fs::OpenOptions::new().write(true).open(&inner.path)?;
        let mut buf = vec![0u8; 1 << 16];
        while !inner.disposed.load(SeqCst) {
            let n = src.read(&mut buf)?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])?;
            file.flush()?;
            inner.arrived.fetch_add(n as u64, SeqCst);
            inner.notify();
        }
        Ok(())
    })();
    match result {
        Ok(()) => inner.complete.store(true, SeqCst),
        Err(e) => {
            if !inner.disposed.load(SeqCst) {
                *lock(&inner.failed) = Some(e.to_string());
            }
        }
    }
    inner.notify();
}

/// The temp file while it is still growing: a read waits for bytes that have not arrived
/// yet, and says end of file only once the download is over.
struct Arriving {
    inner: Arc<Inner>,
    file: File,
    at: u64,
}

impl Read for Arriving {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let i = &self.inner;
        loop {
            if i.disposed.load(SeqCst) {
                return Err(io::Error::new(ErrorKind::Other, "closed"));
            }
            let arrived = i.arrived.load(SeqCst);
            if self.at < arrived {
                let n = (buf.len() as u64).min(arrived - self.at) as usize;
                self.file.seek(SeekFrom::Start(self.at))?;
                let n = self.file.read(&mut buf[..n])?;
                self.at += n as u64;
                return Ok(n);
            }
            if i.complete.load(SeqCst) || lock(&i.failed).is_some() {
                return Ok(0);
            }
            i.wait(200);
        }
    }
}

impl Seek for Arriving {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        match pos {
            SeekFrom::Current(0) => Ok(self.at),
            _ => Err(io::Error::new(ErrorKind::Unsupported, "still downloading")),
        }
    }
}

impl MediaSource for Arriving {
    fn is_seekable(&self) -> bool {
        false
    }
    fn byte_len(&self) -> Option<u64> {
        None
    }
}

fn run(inner: Arc<Inner>) {
    let result = run_inner(&inner);
    inner.out.close();
    let _ = fs::remove_file(&inner.path);
    if let Err(e) = result {
        if !inner.disposed.load(SeqCst) {
            (inner.cb.on_error)(e);
        }
    }
}

fn failure(inner: &Inner) -> Option<String> {
    lock(&inner.failed).as_ref().map(|f| format!("the file stopped arriving: {f}"))
}

fn run_inner(inner: &Arc<Inner>) -> Result<(), String> {
    let mut target: Option<u64> = None;
    loop {
        if inner.disposed.load(SeqCst) {
            return Ok(());
        }
        // With the whole file on disk a seek can use the seek table; before that it
        // starts again from the top of what has arrived and decodes forward.
        let ready = inner.complete.load(SeqCst) && target.is_some();
        let source: Box<dyn MediaSource> = if ready {
            Box::new(File::open(&inner.path).map_err(|e| e.to_string())?)
        } else {
            Box::new(Arriving {
                inner: inner.clone(),
                file: File::open(&inner.path).map_err(|e| e.to_string())?,
                at: 0,
            })
        };
        let mss = MediaSourceStream::new(source, Default::default());
        let mut reader = match FlacReader::try_new(mss, &FormatOptions::default()) {
            Ok(r) => r,
            Err(e) => {
                if inner.disposed.load(SeqCst) {
                    return Ok(());
                }
                return Err(failure(inner).unwrap_or_else(|| format!("not a FLAC file: {e}")));
            }
        };
        let track = reader.default_track().ok_or("no audio track")?.clone();
        let p = &track.codec_params;
        let info = Info {
            rate: p.sample_rate.ok_or("no sample rate")?,
            channels: p.channels.ok_or("no channel layout")?.count() as u16,
            bits: p.bits_per_sample.ok_or("no bit depth")? as u16,
            total: p.n_frames.unwrap_or(0),
        };
        if info.bits != 16 && info.bits != 24 {
            return Err(format!("only 16 and 24 bit FLAC is supported, this file is {} bit", info.bits));
        }
        if inner.info.get().is_none() {
            let opened = inner.out.open(info.rate, info.channels, info.bits)?;
            inner.output_bits.store(opened as u32, SeqCst);
            let _ = inner.info.set(info);
            inner.apply_volume();
            if !inner.paused.load(SeqCst) {
                inner.out.start();
            }
        }
        let mut decoder = FlacDecoder::try_new(&track.codec_params, &DecoderOptions::default())
            .map_err(|e| e.to_string())?;
        let mut from = 0;
        if ready {
            let to = target.unwrap().min(info.total);
            let seeked = reader
                .seek(SeekMode::Accurate, SeekTo::TimeStamp { ts: to, track_id: track.id })
                .map_err(|e| e.to_string())?;
            from = seeked.actual_ts;
            decoder.reset();
        }
        match play(inner, &mut reader, &mut decoder, info, track.id, from, target)? {
            Some(t) => target = Some(t),
            None => return Ok(()),
        }
    }
}

/// Decode and write until the end, a seek request (returned) or dispose (`None`).
#[allow(clippy::too_many_arguments)]
fn play(
    inner: &Inner,
    reader: &mut FlacReader,
    decoder: &mut FlacDecoder,
    info: Info,
    track_id: u32,
    from: u64,
    fixed: Option<u64>,
) -> Result<Option<u64>, String> {
    let ch = info.channels as usize;
    let shift = 32 - info.bits as u32;
    let out_bits = inner.output_bits.load(SeqCst) as u16;
    let mut sample = from;
    let mut target = fixed.unwrap_or_else(|| inner.start_sample(info));
    let mut emitting = false;
    let mut sbuf: Option<SampleBuffer<i32>> = None;
    let mut native: Vec<i32> = Vec::new();
    let mut dithered: Vec<i32> = Vec::new();
    let mut rng = Rng::new();

    loop {
        if inner.disposed.load(SeqCst) {
            return Ok(None);
        }
        if let Some(t) = inner.take_seek() {
            inner.out.flush();
            return Ok(Some(t));
        }
        let packet = match reader.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(e)) => {
                if inner.disposed.load(SeqCst) {
                    return Ok(None);
                }
                if let Some(f) = failure(inner) {
                    return Err(f);
                }
                if e.kind() != ErrorKind::UnexpectedEof {
                    return Err(e.to_string());
                }
                // The end of the file.
                inner.out.drain();
                if !inner.disposed.load(SeqCst) {
                    (inner.cb.on_end)();
                }
                return Ok(None);
            }
            Err(e) => return Err(failure(inner).unwrap_or_else(|| e.to_string())),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = decoder.decode(&packet).map_err(|e| e.to_string())?;
        let frames = decoded.frames() as u64;
        if sbuf.as_ref().map_or(true, |b| (b.capacity() as u64) < decoded.capacity() as u64 * ch as u64) {
            sbuf = Some(SampleBuffer::<i32>::new(decoded.capacity() as u64, *decoded.spec()));
        }
        let sb = sbuf.as_mut().unwrap();
        sb.copy_interleaved_ref(decoded);

        // A seek that came in while this packet was being decoded.
        if inner.seek_request.load(SeqCst) >= 0 {
            continue;
        }
        let end = sample + frames;
        if end <= target {
            sample = end;
            continue;
        }
        let skip = target.saturating_sub(sample) as usize;
        if !emitting {
            emitting = true;
            inner.out.flush();
            inner.run_start.store(sample + skip as u64, SeqCst);
            inner.line_base.store(inner.out.frame_position(), SeqCst);
            inner.seeking_to.store(-1, SeqCst);
            // `target` stays as it is for any later packet.
            target = sample + skip as u64;
        }
        sample = end;

        // The decoder hands back samples left-justified in 32 bits; bring them back down
        // to the file's own bit depth.
        native.clear();
        native.extend(sb.samples()[skip * ch..frames as usize * ch].iter().map(|&s| s >> shift));

        // Volume is read after a pause ends, so a change made while paused applies to the
        // very first sound that follows.
        if !wait_unpaused(inner) {
            let r = inner.take_seek();
            if r.is_some() {
                inner.out.flush();
            }
            return Ok(r);
        }
        let gain = inner.gain();
        let slice: &[i32] = if out_bits < info.bits {
            to_16(&native, &mut dithered, &mut rng);
            if gain != 1.0 {
                scale(&mut dithered, 16, gain);
            }
            &dithered
        } else {
            if gain != 1.0 {
                scale(&mut native, info.bits, gain);
            }
            &native
        };
        if !write_all(inner, slice) {
            let r = inner.take_seek();
            if r.is_some() {
                inner.out.flush();
            }
            return Ok(r);
        }
        if !inner.started.swap(true, SeqCst) {
            (inner.cb.on_started)();
        }
    }
}

/// Wait out a pause. `false` when a seek or dispose came first.
fn wait_unpaused(inner: &Inner) -> bool {
    loop {
        if inner.disposed.load(SeqCst) || inner.seek_request.load(SeqCst) >= 0 {
            return false;
        }
        if !inner.paused.load(SeqCst) {
            return true;
        }
        inner.wait(200);
    }
}

/// Write every sample, waiting out a pause. `false` when a seek or dispose cut it short.
fn write_all(inner: &Inner, samples: &[i32]) -> bool {
    let mut at = 0;
    while at < samples.len() {
        if inner.disposed.load(SeqCst) || inner.seek_request.load(SeqCst) >= 0 {
            return false;
        }
        if inner.paused.load(SeqCst) {
            inner.wait(200);
            continue;
        }
        at += inner.out.write(&samples[at..]);
    }
    true
}
