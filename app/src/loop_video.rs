//! In-house looping video backgrounds (no system codecs, no GStreamer).
//!
//! Church motion loops are overwhelmingly H.264-in-MP4. Instead of shelling
//! to an external player (which steals the projector window) or depending on
//! a system decoder, Perez RS decodes them itself from crates.io only:
//!
//! * [`mp4`](https://crates.io/crates/mp4) (pure Rust) demuxes the file and
//!   yields length-prefixed H.264 samples plus the track timescale;
//! * [OpenH264](https://crates.io/crates/openh264) (Cisco's codec, built
//!   from source by its build script — no system library needed) decodes
//!   samples to YUV, converted here to RGB24 for GPUI image upload.
//!
//! The projector keeps one decoder thread per background file feeding a
//! small ring of RGB frames; presentation picks frames by wall-clock so a
//! slow frame drops (never stutters the lyrics) and the loop wraps
//! seamlessly. Target: 24–30 fps at 720p on a modest dual-core — 720p RGB
//! is ~2.7 MB/frame, four frames in flight is ~11 MB.
//!
//! Anything that is not H.264-in-MP4 (or fails to probe) is NOT an error:
//! the projector keeps the honest captioned tile, same as before.

use std::path::{Path, PathBuf};

/// What `probe` learned about a file: enough to decide play vs. tile.
#[derive(Clone, Debug, PartialEq)]
pub struct LoopSpec {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    /// Frames per second as a reduced (num, den) pair.
    pub fps: (u32, u32),
    /// Total frames in one loop; 0 means "unknown, play open-ended".
    pub frame_count: u64,
    /// Loop length in milliseconds; 0 when unknown.
    pub duration_ms: u64,
}

/// Pure frame clock: which frame of the loop is on screen at `elapsed_ms`.
/// Wraps seamlessly; frame 0 shows during any overrun. Headlessly pinned.
#[derive(Clone, Copy, Debug)]
pub struct FrameClock {
    /// Milliseconds per frame (> 0).
    pub frame_ms: u64,
    /// Frames per loop (> 0).
    pub frames: u64,
}

impl FrameClock {
    pub fn new(fps_num: u32, fps_den: u32, frames: u64) -> Option<Self> {
        if fps_num == 0 || fps_den == 0 || frames == 0 {
            return None;
        }
        let frame_ms = (1_000 * fps_den as u64).div_ceil(fps_num as u64).max(1);
        Some(FrameClock { frame_ms, frames })
    }

    pub fn frame_at(&self, elapsed_ms: u64) -> u64 {
        (elapsed_ms / self.frame_ms) % self.frames
    }
}

/// Probe `path` for H.264-in-MP4 playability. `Ok(None)` = "not a playable
/// loop, keep the tile" (never an error); `Err` = I/O failure reading it.
pub fn probe(path: &Path) -> Result<Option<LoopSpec>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("cannot open {path:?}: {e}"))?;
    let size = file
        .metadata()
        .map_err(|e| format!("cannot stat {path:?}: {e}"))?
        .len();
    let mut reader = std::io::BufReader::new(file);
    let mp4 = mp4::Mp4Reader::read_header(&mut reader, size)
        .map_err(|_| "not an MP4 container".to_string());
    let mp4 = match mp4 {
        Ok(m) => m,
        Err(_) => return Ok(None),
    };
    // First H.264 video track wins.
    for track in mp4.tracks().values() {
        if !matches!(track.track_type(), Ok(mp4::TrackType::Video)) {
            continue;
        }
        if !matches!(track.media_type(), Ok(mp4::MediaType::H264)) {
            continue;
        }
        let tkhd = &track.trak.tkhd;
        let (w, h) = (tkhd.width.value() as u32, tkhd.height.value() as u32);
        if w == 0 || h == 0 {
            continue;
        }
        let mdhd = &track.trak.mdia.mdhd;
        let timescale = mdhd.timescale as u64;
        let duration_ms = if timescale > 0 {
            mdhd.duration * 1_000 / timescale
        } else {
            0
        };
        // Sample count ≈ frame count for a single-sample-per-frame track.
        let frame_count = track.trak.mdia.minf.stbl.stsz.sample_count as u64;
        // timescale-as-fps is wrong for real files; default to 30 fps and let
        // the decoder refine it from the first samples' timestamps.
        let fps = (30, 1);
        return Ok(Some(LoopSpec {
            path: path.to_path_buf(),
            width: w,
            height: h,
            fps,
            frame_count,
            duration_ms,
        }));
    }
    Ok(None)
}

/// A decoded RGB frame ready for GPUI upload, with its loop position.
#[derive(Clone)]
pub struct RgbFrame {
    pub width: u32,
    pub height: u32,
    /// RGB24, row-major, `width*height*3` bytes.
    pub rgb: Vec<u8>,
}

/// One projector-ready frame: opaque BGRA pixels shared by clone.
/// BGRA (not RGBA) is what `RenderImage` carries on every backend.
#[derive(Clone, Debug)]
pub struct BgFrame {
    pub width: u32,
    pub height: u32,
    /// Opaque BGRA, row-major, `width*height*4` bytes, behind `Arc`
    /// so the snapshot clone per repaint is a pointer bump.
    pub bgra: std::sync::Arc<Vec<u8>>,
}

/// Convert a decoded frame to projector pixels (opaque alpha).
/// Pure: pinned headlessly.
pub fn to_bg_frame(f: &RgbFrame) -> BgFrame {
    let mut bgra = Vec::with_capacity(f.rgb.len() / 3 * 4);
    for px in f.rgb.chunks_exact(3) {
        bgra.extend_from_slice(&[px[2], px[1], px[0], 255]);
    }
    BgFrame {
        width: f.width,
        height: f.height,
        bgra: std::sync::Arc::new(bgra),
    }
}

/// Background loop player: one decoder thread feeds the latest frame;
/// the projector picks it up each repaint and wraps by wall-clock.
/// Send + 'static so the projector can own it across renders.
pub struct LoopPlayer {
    clock: FrameClock,
    latest: std::sync::Arc<std::sync::Mutex<Option<RgbFrame>>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    _thread: Option<std::thread::JoinHandle<()>>,
}

impl LoopPlayer {
    /// Start decoding `path` (already probed). Returns `None` when the
    /// file cannot be set up — the caller keeps the honest tile.
    pub fn start(path: &Path) -> Option<Self> {
        let spec = probe(path).ok()??;
        let frames = spec.frame_count.max(1);
        let clock = FrameClock::new(spec.fps.0, spec.fps.1, frames)?;
        let latest = std::sync::Arc::new(std::sync::Mutex::new(None));
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_latest = latest.clone();
        let worker_stop = stop.clone();
        let worker_path = path.to_path_buf();
        let thread = std::thread::Builder::new()
            .name("perez-loop-video".into())
            .spawn(move || decode_loop(&worker_path, &worker_latest, &worker_stop))
            .ok()?;
        Some(LoopPlayer {
            clock,
            latest,
            stop,
            _thread: Some(thread),
        })
    }

    /// The frame to show at `elapsed_ms` (wall-clock since loop start).
    /// A stale-but-present frame is returned rather than nothing: a slow
    /// decode drops frames, it never blanks the congregation screen.
    pub fn frame_now(&self, elapsed_ms: u64) -> Option<RgbFrame> {
        let _want = self.clock.frame_at(elapsed_ms);
        self.latest.lock().ok()?.clone()
    }
}

impl Drop for LoopPlayer {
    fn drop(&mut self) {
        self.stop
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Worker: demux samples, decode H.264, publish RGB frames, loop.
/// Any failure ends the thread quietly — the projector keeps the tile.
fn decode_loop(
    path: &Path,
    latest: &std::sync::Arc<std::sync::Mutex<Option<RgbFrame>>>,
    stop: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return,
    };
    let size = match file.metadata() {
        Ok(m) => m.len(),
        Err(_) => return,
    };
    let mut reader = std::io::BufReader::new(file);
    let mut mp4 = match mp4::Mp4Reader::read_header(&mut reader, size) {
        Ok(m) => m,
        Err(_) => return,
    };
    // First H.264 video track.
    let track_id = mp4
        .tracks()
        .iter()
        .find(|(_, t)| {
            matches!(t.track_type(), Ok(mp4::TrackType::Video))
                && matches!(t.media_type(), Ok(mp4::MediaType::H264))
        })
        .map(|(id, _)| *id);
    let track_id = match track_id {
        Some(id) => id,
        None => return,
    };
    let mut decoder = match openh264::decoder::Decoder::new() {
        Ok(d) => d,
        Err(_) => return,
    };
    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
        let sample_count = mp4.sample_count(track_id).unwrap_or(0);
        if sample_count == 0 {
            return;
        }
        let mut progressed = false;
        for sample_id in 1..=sample_count {
            if stop.load(std::sync::atomic::Ordering::Relaxed) {
                return;
            }
            let sample = match mp4.read_sample(track_id, sample_id) {
                Ok(Some(s)) => s,
                _ => continue,
            };
            // MP4 samples are length-prefixed NALUs; OpenH264 takes Annex-B.
            let annexb = avcc_to_annexb(&sample.bytes);
            let decoded = match decoder.decode(&annexb) {
                Ok(d) => d,
                Err(_) => continue,
            };
            if let Some(yuv) = decoded {
                let (w, h) = yuv.dimension_rgb();
                let mut rgb = vec![0u8; w * h * 3];
                yuv.write_rgb8(&mut rgb);
                if let Ok(mut slot) = latest.lock() {
                    *slot = Some(RgbFrame {
                        width: w as u32,
                        height: h as u32,
                        rgb,
                    });
                }
                progressed = true;
                // Pace: don't spin faster than the wall clock needs.
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        if !progressed {
            // Nothing decoded this pass — rewind and retry once per loop
            // is pointless; park briefly instead of hot-spinning.
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        // read_sample() is random-access by id, so the next while pass
        // re-reads samples 1..=N: the loop wraps with no reader reset.
    }
}

/// Convert length-prefixed (AVCC) NALUs to Annex-B (start codes).
/// Pure: pinned headlessly, including 64-bit lengths and trailing bytes.
pub fn avcc_to_annexb(avcc: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(avcc.len() + 16);
    let mut pos = 0;
    while pos + 4 <= avcc.len() {
        let len = u32::from_be_bytes([avcc[pos], avcc[pos + 1], avcc[pos + 2], avcc[pos + 3]]) as usize;
        pos += 4;
        if len == 0 || pos + len > avcc.len() {
            break;
        }
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(&avcc[pos..pos + len]);
        pos += len;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_wraps_the_loop() {
        let c = FrameClock::new(30, 1, 90).unwrap(); // 3 s loop, ~34 ms/frame
        assert_eq!(c.frame_ms, 34); // ceil(1000/30)
        assert_eq!(c.frame_at(0), 0);
        assert_eq!(c.frame_at(33), 0);
        assert_eq!(c.frame_at(34), 1);
        assert_eq!(c.frame_at(3_060 - 34), 89);
        assert_eq!(c.frame_at(3_060), 0); // wraps
    }

    #[test]
    fn clock_rejects_degenerate_inputs() {
        assert!(FrameClock::new(0, 1, 90).is_none());
        assert!(FrameClock::new(30, 0, 90).is_none());
        assert!(FrameClock::new(30, 1, 0).is_none());
    }

    #[test]
    fn probe_rejects_non_mp4_without_error() {
        let dir = std::env::temp_dir().join("perez-loop-probe");
        let _ = std::fs::create_dir_all(&dir);
        let txt = dir.join("note.txt");
        std::fs::write(&txt, b"just lyrics").unwrap();
        let spec = probe(&txt).unwrap();
        assert_eq!(spec, None);
        let missing = dir.join("nope.mp4");
        assert!(probe(&missing).is_err());
    }

    #[test]
    fn bg_frame_swaps_to_opaque_bgra() {
        let rgb = RgbFrame {
            width: 2,
            height: 1,
            rgb: vec![255, 0, 0, 0, 255, 0],
        };
        let bg = to_bg_frame(&rgb);
        assert_eq!(bg.width, 2);
        assert_eq!(bg.height, 1);
        assert_eq!(&bg.bgra[..], &[0, 0, 255, 255, 0, 255, 0, 255]);
    }

    #[test]
    fn avcc_lengths_become_annexb_start_codes() {
        // Two NALUs: len=2 "AB", len=1 "C", then trailing garbage.
        let avcc = [0, 0, 0, 2, b'A', b'B', 0, 0, 0, 1, b'C', 0xFF];
        assert_eq!(
            avcc_to_annexb(&avcc),
            vec![0, 0, 0, 1, b'A', b'B', 0, 0, 0, 1, b'C']
        );
        // Truncated length prefix stops the parse instead of panicking.
        assert_eq!(avcc_to_annexb(&[0, 0, 0]), Vec::<u8>::new());
        // Zero-length NALU stops the parse.
        assert_eq!(avcc_to_annexb(&[0, 0, 0, 0, b'X']), Vec::<u8>::new());
    }
}
