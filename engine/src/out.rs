//! The sound card side of the engine, as a trait so the engine can be tested without one.

/// A PCM sink. Samples are interleaved, signed, at the bit depth that `open` returned
/// (16 or 24), held in the low bits of an `i32`.
///
/// Every method takes `&self`: the engine's player thread sits inside `write` while the
/// UI thread calls `stop`, `start` and `flush`, so an implementation synchronises itself.
pub trait PcmOut: Send + Sync {
    /// Open the device at `rate` Hz, `channels` channels, `bits` bits. Returns the bit depth
    /// that was actually opened: `bits`, or 16 when the card only takes 16.
    fn open(&self, rate: u32, channels: u16, bits: u16) -> Result<u16, String>;
    /// Queue samples; blocks while the device buffer is full. Returns how many were taken,
    /// which is fewer than `samples.len()` when `flush` or `close` cut the wait short.
    fn write(&self, samples: &[i32]) -> usize;
    fn start(&self);
    fn stop(&self);
    /// Throw away everything queued but not yet played.
    fn flush(&self);
    /// Wait until everything queued has been played.
    fn drain(&self);
    fn close(&self);
    /// Frames played since `open`.
    fn frame_position(&self) -> u64;
    /// Hardware or driver volume. `false` means not supported; the engine scales instead.
    fn set_gain_db(&self, _db: f32) -> bool {
        false
    }
}
