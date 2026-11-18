//! Sound card output through cpal (WASAPI, CoreAudio, ALSA).
//!
//! Samples go through a ring buffer to the device callback. The cpal stream is not `Send`,
//! so a small thread owns it and takes commands over a channel.
//!
//! This is the shared-mode path: on Windows the system mixer still sits in between. Exclusive
//! mode, which would be bit-perfect all the way, is not available through cpal.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::SeqCst};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SampleRate, StreamConfig};

use crate::out::PcmOut;

struct Shared {
    queue: Mutex<VecDeque<i32>>,
    space: Condvar,
    capacity: AtomicUsize,
    channels: AtomicUsize,
    played: AtomicU64,
    generation: AtomicU64,
    closed: AtomicBool,
}

enum Cmd {
    Play,
    Pause,
    Close,
}

pub struct CpalOut {
    shared: Arc<Shared>,
    ctl: Mutex<Option<mpsc::Sender<Cmd>>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl CpalOut {
    pub fn new() -> CpalOut {
        CpalOut {
            shared: Arc::new(Shared {
                queue: Mutex::new(VecDeque::new()),
                space: Condvar::new(),
                capacity: AtomicUsize::new(0),
                channels: AtomicUsize::new(2),
                played: AtomicU64::new(0),
                generation: AtomicU64::new(0),
                closed: AtomicBool::new(false),
            }),
            ctl: Mutex::new(None),
        }
    }

    fn send(&self, c: Cmd) {
        if let Some(tx) = lock(&self.ctl).as_ref() {
            let _ = tx.send(c);
        }
    }
}

impl Default for CpalOut {
    fn default() -> Self {
        Self::new()
    }
}

/// Fill one device buffer from the ring. Underruns play silence and are not counted.
fn pull<T: Copy>(sh: &Shared, data: &mut [T], zero: T, conv: impl Fn(i32) -> T) {
    let Ok(mut q) = sh.queue.try_lock() else {
        data.fill(zero);
        return;
    };
    let mut got = 0usize;
    for slot in data.iter_mut() {
        match q.pop_front() {
            Some(v) => {
                *slot = conv(v);
                got += 1;
            }
            None => *slot = zero,
        }
    }
    drop(q);
    sh.played.fetch_add((got / sh.channels.load(SeqCst).max(1)) as u64, SeqCst);
    sh.space.notify_all();
}

/// Build the stream. Formats are tried in the order that keeps the file's samples exact:
/// 32-bit integer, then 32-bit float (holds 24 bits exactly), then 16-bit.
fn build(sh: &Arc<Shared>, rate: u32, channels: u16, bits: u16) -> Result<(cpal::Stream, u16), String> {
    let device = cpal::default_host().default_output_device().ok_or("no sound output device")?;
    let ranges: Vec<_> = device
        .supported_output_configs()
        .map_err(|e| e.to_string())?
        .filter(|r| {
            r.channels() == channels && r.min_sample_rate().0 <= rate && rate <= r.max_sample_rate().0
        })
        .collect();
    let want: &[(SampleFormat, u16)] = if bits > 16 {
        &[(SampleFormat::I32, bits), (SampleFormat::F32, bits), (SampleFormat::I16, 16)]
    } else {
        &[(SampleFormat::I16, 16), (SampleFormat::I32, 16), (SampleFormat::F32, 16)]
    };
    for &(fmt, src_bits) in want {
        let Some(range) = ranges.iter().find(|r| r.sample_format() == fmt) else { continue };
        let config: StreamConfig = range.clone().with_sample_rate(SampleRate(rate)).config();
        let s = sh.clone();
        let err = |e| eprintln!("audio stream error: {e}");
        let stream = match fmt {
            SampleFormat::I16 => device.build_output_stream(
                &config,
                move |d: &mut [i16], _| pull(&s, d, 0, |v| v as i16),
                err,
                None,
            ),
            SampleFormat::I32 => {
                let shift = 32 - src_bits as u32;
                device.build_output_stream(
                    &config,
                    move |d: &mut [i32], _| pull(&s, d, 0, |v| v << shift),
                    err,
                    None,
                )
            }
            _ => {
                let scale = (1u64 << (src_bits - 1)) as f32;
                device.build_output_stream(
                    &config,
                    move |d: &mut [f32], _| pull(&s, d, 0.0, |v| v as f32 / scale),
                    err,
                    None,
                )
            }
        };
        if let Ok(stream) = stream {
            return Ok((stream, src_bits));
        }
    }
    Err(format!("the sound card takes none of {rate} Hz, {channels} ch, {bits} bit"))
}

impl PcmOut for CpalOut {
    fn open(&self, rate: u32, channels: u16, bits: u16) -> Result<u16, String> {
        let sh = self.shared.clone();
        sh.channels.store(channels as usize, SeqCst);
        // Half a second in the ring.
        sh.capacity.store(rate as usize * channels as usize / 2, SeqCst);
        sh.closed.store(false, SeqCst);
        sh.played.store(0, SeqCst);
        let (ready_tx, ready_rx) = mpsc::channel::<Result<u16, String>>();
        let (tx, rx) = mpsc::channel::<Cmd>();
        thread::Builder::new()
            .name("audio-out".into())
            .spawn(move || match build(&sh, rate, channels, bits) {
                Ok((stream, opened)) => {
                    // Some backends start on their own.
                    let _ = stream.pause();
                    let _ = ready_tx.send(Ok(opened));
                    while let Ok(c) = rx.recv() {
                        match c {
                            Cmd::Play => {
                                let _ = stream.play();
                            }
                            Cmd::Pause => {
                                let _ = stream.pause();
                            }
                            Cmd::Close => break,
                        }
                    }
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            })
            .map_err(|e| e.to_string())?;
        let opened = ready_rx.recv().map_err(|e| e.to_string())??;
        *lock(&self.ctl) = Some(tx);
        Ok(opened)
    }

    fn write(&self, samples: &[i32]) -> usize {
        let sh = &self.shared;
        let generation = sh.generation.load(SeqCst);
        let cap = sh.capacity.load(SeqCst).max(1);
        let mut done = 0;
        let mut q = lock(&sh.queue);
        while done < samples.len() {
            if sh.closed.load(SeqCst) || sh.generation.load(SeqCst) != generation {
                return done;
            }
            let room = cap.saturating_sub(q.len());
            if room == 0 {
                q = sh.space.wait_timeout(q, Duration::from_millis(50)).unwrap_or_else(|e| e.into_inner()).0;
                continue;
            }
            let n = room.min(samples.len() - done);
            q.extend(&samples[done..done + n]);
            done += n;
        }
        done
    }

    fn start(&self) {
        self.send(Cmd::Play);
    }

    fn stop(&self) {
        self.send(Cmd::Pause);
    }

    fn flush(&self) {
        let sh = &self.shared;
        let mut q = lock(&sh.queue);
        q.clear();
        sh.generation.fetch_add(1, SeqCst);
        drop(q);
        sh.space.notify_all();
    }

    fn drain(&self) {
        let sh = &self.shared;
        let generation = sh.generation.load(SeqCst);
        let t = Instant::now();
        loop {
            let q = lock(&sh.queue);
            if q.is_empty() || sh.closed.load(SeqCst) || sh.generation.load(SeqCst) != generation {
                break;
            }
            let _ = sh.space.wait_timeout(q, Duration::from_millis(50));
            // A stopped stream never empties the ring; do not wait for it forever.
            if t.elapsed() > Duration::from_secs(30) {
                return;
            }
        }
        // What the device itself still holds.
        thread::sleep(Duration::from_millis(80));
    }

    fn close(&self) {
        self.shared.closed.store(true, SeqCst);
        self.shared.space.notify_all();
        if let Some(tx) = lock(&self.ctl).take() {
            let _ = tx.send(Cmd::Close);
        }
    }

    fn frame_position(&self) -> u64 {
        self.shared.played.load(SeqCst)
    }
}
