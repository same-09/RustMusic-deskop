//! 24 to 16 bit with TPDF dither, and soft volume.

/// Tiny xorshift generator; dither does not need anything better.
pub struct Rng(u32);

impl Rng {
    pub fn new() -> Self {
        Rng(0x9E37_79B9)
    }
    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
}

/// 24-bit samples to 16-bit values, with triangular dither (one 16-bit step peak to peak
/// each side), so quiet fades do not turn into rounding noise that follows the music.
pub fn to_16(src: &[i32], dst: &mut Vec<i32>, rng: &mut Rng) {
    dst.clear();
    dst.reserve(src.len());
    for &v in src {
        let d = (rng.next() % 256) as i32 - (rng.next() % 256) as i32;
        let q = ((v + d) as f64 / 256.0).round();
        dst.push(q.clamp(-32768.0, 32767.0) as i32);
    }
}

/// Multiply by `gain` in place, clamped to the range of `bits`.
pub fn scale(samples: &mut [i32], bits: u16, gain: f64) {
    let max = (1i64 << (bits - 1)) - 1;
    let min = -(1i64 << (bits - 1));
    for s in samples {
        *s = ((*s as f64 * gain).round() as i64).clamp(min, max) as i32;
    }
}
