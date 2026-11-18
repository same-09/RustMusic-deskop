//! Perintah FLAC untuk UI: putar file lossless lewat mesin Rust (`rustmusic-engine`),
//! pada sample rate dan bit depth aslinya.

use std::io::{self, Read};
use std::sync::{Arc, Mutex, MutexGuard};

use rustmusic_engine::{Callbacks, CpalOut, Engine, FlacEngine};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

#[derive(Default)]
pub struct Player(Mutex<Option<FlacEngine>>);

impl Player {
    fn slot(&self) -> MutexGuard<'_, Option<FlacEngine>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[derive(Serialize)]
pub struct Status {
    position_ms: u64,
    duration_ms: u64,
    sample_rate: Option<u32>,
    bits: Option<u16>,
    output_bits: Option<u16>,
    playing: bool,
}

/// Mulai putar FLAC dari `url`. Event ke UI: `flac-started`, `flac-ended`, `flac-error`.
#[tauri::command]
pub fn flac_play(
    app: AppHandle,
    state: State<'_, Player>,
    url: String,
    start_ms: u64,
) -> Result<(), String> {
    let source = Box::new(move || -> io::Result<Box<dyn Read + Send>> {
        let response = reqwest::blocking::get(&url)
            .and_then(|r| r.error_for_status())
            .map_err(io::Error::other)?;
        Ok(Box::new(response))
    });
    let (a, b, c) = (app.clone(), app.clone(), app);
    let callbacks = Callbacks {
        on_started: Arc::new(move || {
            let _ = a.emit("flac-started", ());
        }),
        on_end: Arc::new(move || {
            let _ = b.emit("flac-ended", ());
        }),
        on_error: Arc::new(move |message| {
            let _ = c.emit("flac-error", message);
        }),
    };
    let engine = FlacEngine::new(
        source,
        Box::new(move || start_ms),
        true,
        callbacks,
        Arc::new(CpalOut::new()),
    )
    .map_err(|e| e.to_string())?;
    // Mesin lama berhenti saat dibuang (Drop).
    *state.slot() = Some(engine);
    Ok(())
}

#[tauri::command]
pub fn flac_pause(state: State<'_, Player>) {
    if let Some(e) = state.slot().as_ref() {
        e.pause();
    }
}

#[tauri::command]
pub fn flac_resume(state: State<'_, Player>) {
    if let Some(e) = state.slot().as_ref() {
        e.play();
    }
}

#[tauri::command]
pub fn flac_seek(state: State<'_, Player>, ms: u64) {
    if let Some(e) = state.slot().as_ref() {
        e.seek(ms);
    }
}

/// Volume 0.0 sampai 1.0.
#[tauri::command]
pub fn flac_volume(state: State<'_, Player>, volume: f64) {
    if let Some(e) = state.slot().as_ref() {
        e.set_volume(volume);
    }
}

#[tauri::command]
pub fn flac_stop(state: State<'_, Player>) {
    *state.slot() = None;
}

#[tauri::command]
pub fn flac_status(state: State<'_, Player>) -> Option<Status> {
    let guard = state.slot();
    let e = guard.as_ref()?;
    Some(Status {
        position_ms: e.position_ms(),
        duration_ms: e.duration_ms(),
        sample_rate: e.sample_rate(),
        bits: e.bits(),
        output_bits: e.output_bits(),
        playing: e.is_playing(),
    })
}
