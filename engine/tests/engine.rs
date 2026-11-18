use std::fs::File;
use std::io::{self, Read};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::SeqCst};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rustmusic_engine::{Callbacks, Engine, FlacEngine, PcmOut};

fn data(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name)
}

/// A sound card that keeps what it is given.
struct Capture {
    only16: bool,
    opened: Mutex<Option<(u32, u16, u16)>>,
    pcm: Mutex<Vec<i32>>,
    frames: AtomicU64,
    channels: AtomicU64,
    flushes: AtomicU64,
}

impl Capture {
    fn new(only16: bool) -> Arc<Capture> {
        Arc::new(Capture {
            only16,
            opened: Mutex::new(None),
            pcm: Mutex::new(Vec::new()),
            frames: AtomicU64::new(0),
            channels: AtomicU64::new(1),
            flushes: AtomicU64::new(0),
        })
    }
    fn taken(&self) -> Vec<i32> {
        self.pcm.lock().unwrap().clone()
    }
}

impl PcmOut for Capture {
    fn open(&self, rate: u32, channels: u16, bits: u16) -> Result<u16, String> {
        let b = if self.only16 && bits > 16 { 16 } else { bits };
        *self.opened.lock().unwrap() = Some((rate, channels, b));
        self.channels.store(channels as u64, SeqCst);
        Ok(b)
    }
    fn write(&self, samples: &[i32]) -> usize {
        self.pcm.lock().unwrap().extend_from_slice(samples);
        self.frames.fetch_add(samples.len() as u64 / self.channels.load(SeqCst), SeqCst);
        samples.len()
    }
    fn start(&self) {}
    fn stop(&self) {}
    fn flush(&self) {
        self.flushes.fetch_add(1, SeqCst);
        self.pcm.lock().unwrap().clear();
        self.frames.store(0, SeqCst);
    }
    fn drain(&self) {}
    fn close(&self) {}
    fn frame_position(&self) -> u64 {
        self.frames.load(SeqCst)
    }
}

struct Run {
    engine: FlacEngine,
    out: Arc<Capture>,
    ended: Arc<AtomicBool>,
    error: Arc<Mutex<Option<String>>>,
}

fn start(source: Box<dyn Fn() -> io::Result<Box<dyn Read + Send>> + Send + Sync>, start_ms: u64, only16: bool) -> Run {
    let out = Capture::new(only16);
    let ended = Arc::new(AtomicBool::new(false));
    let error = Arc::new(Mutex::new(None));
    let (e, er) = (ended.clone(), error.clone());
    let cb = Callbacks {
        on_started: Arc::new(|| {}),
        on_end: Arc::new(move || e.store(true, SeqCst)),
        on_error: Arc::new(move |m| *er.lock().unwrap() = Some(m)),
    };
    let engine = FlacEngine::new(source, Box::new(move || start_ms), true, cb, out.clone()).unwrap();
    Run { engine, out, ended, error }
}

fn file_source(name: &str) -> Box<dyn Fn() -> io::Result<Box<dyn Read + Send>> + Send + Sync> {
    let p = data(name);
    Box::new(move || Ok(Box::new(File::open(&p)?) as Box<dyn Read + Send>))
}

/// A source that hands the file over a little at a time, like a slow connection.
struct Trickle(File);
impl Read for Trickle {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        std::thread::sleep(Duration::from_millis(2));
        let n = buf.len().min(4096);
        self.0.read(&mut buf[..n])
    }
}

fn wait_for(what: &str, f: impl Fn() -> bool) {
    let t = Instant::now();
    while !f() {
        assert!(t.elapsed() < Duration::from_secs(60), "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Every sample of the file, decoded by claxon, interleaved, at the file's bit depth.
fn reference(name: &str) -> (Vec<i32>, u32, u32, u32) {
    let mut r = claxon::FlacReader::open(data(name)).unwrap();
    let i = r.streaminfo();
    let (rate, ch, bits) = (i.sample_rate, i.channels, i.bits_per_sample);
    (r.samples().map(|s| s.unwrap()).collect(), rate, ch, bits)
}

#[test]
fn plays_16_bit_noise_bit_for_bit() {
    let (want, rate, ch, bits) = reference("noise-16-44k.flac");
    let run = start(file_source("noise-16-44k.flac"), 0, false);
    wait_for("end", || run.ended.load(SeqCst));
    assert_eq!(*run.error.lock().unwrap(), None);
    assert_eq!(*run.out.opened.lock().unwrap(), Some((rate, ch as u16, bits as u16)));
    assert_eq!(run.out.taken(), want);
}

#[test]
fn plays_24_bit_sine_bit_for_bit_at_96k() {
    let (want, rate, ch, bits) = reference("sine-24-96k.flac");
    assert_eq!((rate, bits), (96000, 24));
    let run = start(file_source("sine-24-96k.flac"), 0, false);
    wait_for("end", || run.ended.load(SeqCst));
    assert_eq!(*run.out.opened.lock().unwrap(), Some((rate, ch as u16, 24)));
    assert_eq!(run.out.taken(), want);
    assert!(run.engine.duration_ms() > 0);
}

#[test]
fn plays_while_the_file_is_still_arriving() {
    let (want, ..) = reference("sine-24-96k.flac");
    let p = data("sine-24-96k.flac");
    let source: Box<dyn Fn() -> io::Result<Box<dyn Read + Send>> + Send + Sync> =
        Box::new(move || Ok(Box::new(Trickle(File::open(&p)?)) as Box<dyn Read + Send>));
    let run = start(source, 0, false);
    wait_for("end", || run.ended.load(SeqCst));
    assert_eq!(run.out.taken(), want);
    assert!(run.engine.is_complete());
}

#[test]
fn starts_at_the_asked_position() {
    let (want, rate, ch, _) = reference("sine-24-96k.flac");
    let run = start(file_source("sine-24-96k.flac"), 100, false);
    wait_for("end", || run.ended.load(SeqCst));
    let from = (100 * rate as usize / 1000) * ch as usize;
    assert_eq!(run.out.taken(), want[from..].to_vec());
}

#[test]
fn seek_after_the_download_lands_on_the_exact_sample() {
    let (want, rate, ch, _) = reference("sine-24-96k.flac");
    // Paused, so nothing plays before the seek.
    let out = Capture::new(false);
    let ended = Arc::new(AtomicBool::new(false));
    let e = ended.clone();
    let cb = Callbacks { on_end: Arc::new(move || e.store(true, SeqCst)), ..Callbacks::none() };
    let engine = FlacEngine::new(file_source("sine-24-96k.flac"), Box::new(|| 0), false, cb, out.clone()).unwrap();
    wait_for("download", || engine.is_complete() && engine.sample_rate().is_some());
    engine.seek(200);
    wait_for("seek position", || engine.position_ms() == 200);
    engine.play();
    wait_for("end", || ended.load(SeqCst));
    let from = (200 * rate as usize / 1000) * ch as usize;
    assert_eq!(out.taken(), want[from..].to_vec());
}

#[test]
fn a_24_bit_file_on_a_16_bit_card_is_dithered_not_cut() {
    let (want, ..) = reference("sine-24-96k.flac");
    let run = start(file_source("sine-24-96k.flac"), 0, true);
    wait_for("end", || run.ended.load(SeqCst));
    assert_eq!(run.out.opened.lock().unwrap().unwrap().2, 16);
    assert_eq!(run.engine.output_bits(), Some(16));
    let got = run.out.taken();
    assert_eq!(got.len(), want.len());
    for (g, w) in got.iter().zip(&want) {
        assert!((*w as f64 / 256.0 - *g as f64).abs() <= 2.0, "{g} vs {w}");
        assert!((-32768..=32767).contains(g));
    }
}

#[test]
fn a_file_that_is_not_flac_reports_an_error() {
    let source: Box<dyn Fn() -> io::Result<Box<dyn Read + Send>> + Send + Sync> =
        Box::new(|| Ok(Box::new(io::Cursor::new(vec![7u8; 5000])) as Box<dyn Read + Send>));
    let run = start(source, 0, false);
    wait_for("error", || run.error.lock().unwrap().is_some());
    assert!(!run.ended.load(SeqCst));
}

#[test]
fn a_download_that_fails_reports_an_error() {
    let source: Box<dyn Fn() -> io::Result<Box<dyn Read + Send>> + Send + Sync> =
        Box::new(|| Err(io::Error::new(io::ErrorKind::ConnectionReset, "reset")));
    let run = start(source, 0, false);
    wait_for("error", || run.error.lock().unwrap().is_some());
    assert!(run.error.lock().unwrap().as_ref().unwrap().contains("reset"));
}

#[test]
fn volume_scales_when_the_card_has_no_gain_control() {
    let (want, ..) = reference("noise-16-44k.flac");
    let out = Capture::new(false);
    let ended = Arc::new(AtomicBool::new(false));
    let e = ended.clone();
    let cb = Callbacks { on_end: Arc::new(move || e.store(true, SeqCst)), ..Callbacks::none() };
    let engine = FlacEngine::new(file_source("noise-16-44k.flac"), Box::new(|| 0), false, cb, out.clone()).unwrap();
    wait_for("header", || engine.sample_rate().is_some());
    engine.set_volume(0.5);
    engine.play();
    wait_for("end", || ended.load(SeqCst));
    let got = out.taken();
    assert_eq!(got.len(), want.len());
    for (g, w) in got.iter().zip(&want) {
        assert_eq!(*g, (*w as f64 * 0.5).round() as i32);
    }
}

fn temp_files() -> usize {
    let prefix = format!("rustmusic-{}-", std::process::id());
    std::fs::read_dir(std::env::temp_dir())
        .unwrap()
        .filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().starts_with(&prefix))
        .count()
}

#[test]
fn dispose_stops_the_engine_and_removes_the_temp_file() {
    let p = data("sine-24-96k.flac");
    let source: Box<dyn Fn() -> io::Result<Box<dyn Read + Send>> + Send + Sync> =
        Box::new(move || Ok(Box::new(Trickle(File::open(&p)?)) as Box<dyn Read + Send>));
    let run = start(source, 0, false);
    wait_for("some sound", || run.out.frame_position() > 0);
    assert!(temp_files() >= 1);
    run.engine.dispose();
    wait_for("temp file gone", || temp_files() == 0);
    assert!(!run.ended.load(SeqCst));
    assert_eq!(*run.error.lock().unwrap(), None);
}
