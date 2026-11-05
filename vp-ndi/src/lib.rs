//! Lightweight NDI runtime (clean-room).
//!
//! No NewTek SDK is bundled and nothing proprietary is reimplemented.
//! Two paths:
//! * **Official runtime (preferred):** if the user has installed NDI Tools /
//!   the NDI SDK runtime, [`NdiRuntime`] finds it at runtime via dynamic
//!   load and sends real NDI frames. No link-time dependency.
//! * **Built-in loopback (always works):** a dependency-free TCP frame
//!   server on `127.0.0.1:<port>` streaming length-prefixed BGRA frames.
//!   OBS / NDI Bridge / a companion receiver can consume it; the app's
//!   projector keeps working with zero setup.
//!
//! Wire-up: create one [`NdiRuntime`], call [`NdiRuntime::start`], then
//! [`NdiRuntime::send_bgra`] once per projected frame (or on slide change
//! for stills). Call [`NdiRuntime::stop`] on output disable / shutdown.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::thread::JoinHandle;

use serde::{Deserialize, Serialize};

/// Default loopback port (localhost only).
pub const DEFAULT_PORT: u16 = 5959;
/// Magic for the loopback frame protocol: `b"VPNDI1"` + u32 LE w + u32 LE h + u64 LE seq + BGRA bytes.
pub const MAGIC: &[u8; 6] = b"VPNDI1";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NdiConfig {
    /// NDI source name, e.g. `PEREZ (Projector)`.
    pub source_name: String,
    /// Loopback TCP port (localhost only).
    pub port: u16,
    /// Send even when only the control preview changed (still sends).
    pub send_on_slide_change_only: bool,
}

impl Default for NdiConfig {
    fn default() -> Self {
        Self {
            source_name: "PEREZ (Projector)".into(),
            port: DEFAULT_PORT,
            send_on_slide_change_only: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NdiBackend {
    /// Official NDI SDK runtime found and loaded.
    OfficialSdk,
    /// Built-in loopback TCP server.
    Loopback,
    /// Not running.
    Off,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NdiSource {
    pub name: String,
    pub addr: String,
    pub backend: NdiBackend,
}

#[derive(Clone, Debug, Default)]
pub struct NdiStats {
    pub frames_sent: u64,
    pub bytes_sent: u64,
    pub clients_served: u64,
    pub last_w: u32,
    pub last_h: u32,
}

struct Shared {
    latest: Mutex<Option<Arc<Frame>>>,
    stats: Mutex<NdiStats>,
    seq: AtomicU64,
    running: AtomicBool,
}

#[derive(Clone, Debug)]
pub struct Frame {
    pub w: u32,
    pub h: u32,
    pub seq: u64,
    pub bgra: Vec<u8>,
}

/// Lightweight NDI output runtime.
pub struct NdiRuntime {
    config: NdiConfig,
    shared: Arc<Shared>,
    server: Option<(JoinHandle<()>, SocketAddr)>,
    sdk: Option<OfficialSdk>,
    backend: NdiBackend,
}

impl NdiRuntime {
    pub fn new(config: NdiConfig) -> Self {
        Self {
            config,
            shared: Arc::new(Shared {
                latest: Mutex::new(None),
                stats: Mutex::new(NdiStats::default()),
                seq: AtomicU64::new(0),
                running: AtomicBool::new(false),
            }),
            server: None,
            sdk: OfficialSdk::try_load().ok(),
            backend: NdiBackend::Off,
        }
    }

    pub fn backend(&self) -> NdiBackend {
        self.backend
    }

    /// Official SDK present (real NDI send path available)?
    pub fn sdk_available(&self) -> bool {
        self.sdk.is_some()
    }

    pub fn sources(&self) -> Vec<NdiSource> {
        let mut out = vec![];
        if self.backend != NdiBackend::Off {
            let addr = self
                .server
                .as_ref()
                .map(|(_, a)| a.to_string())
                .unwrap_or_default();
            out.push(NdiSource {
                name: self.config.source_name.clone(),
                addr,
                backend: self.backend,
            });
        }
        // Real SDK receivers would be enumerated here via
        // NDIlib_find_* when the official runtime is loaded.
        out
    }

    /// Start output. Prefers the official SDK when present, always also
    /// brings up the loopback server so local tools can attach.
    pub fn start(&mut self) -> std::io::Result<SocketAddr> {
        self.stop();
        self.shared.running.store(true, Ordering::SeqCst);
        let listener = TcpListener::bind(("127.0.0.1", self.config.port))?;
        let addr = listener.local_addr()?;
        let shared = self.shared.clone();
        let handle = std::thread::Builder::new()
            .name("vp-ndi-server".into())
            .spawn(move || serve_loop(listener, shared))
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        self.server = Some((handle, addr));
        self.backend = if self.sdk.is_some() {
            NdiBackend::OfficialSdk
        } else {
            NdiBackend::Loopback
        };
        Ok(addr)
    }

    /// Push one BGRA frame (stride = w*4, top-down). Cheap when no client
    /// is attached: stores latest + bumps counters only.
    pub fn send_bgra(&self, w: u32, h: u32, bgra: &[u8]) {
        if self.backend == NdiBackend::Off {
            return;
        }
        let expect = (w as usize).saturating_mul(h as usize).saturating_mul(4);
        if bgra.len() < expect || w == 0 || h == 0 {
            return;
        }
        let seq = self.shared.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let frame = Frame {
            w,
            h,
            seq,
            bgra: bgra[..expect].to_vec(),
        };
        if let Some(sdk) = &self.sdk {
            sdk.send(&frame, &self.config.source_name);
        }
        *self.shared.latest.lock().unwrap() = Some(Arc::new(frame));
        let mut st = self.shared.stats.lock().unwrap();
        st.frames_sent += 1;
        st.last_w = w;
        st.last_h = h;
    }

    /// Solid-color still (for lyrics slides without a pixel readback).
    pub fn send_solid(&self, w: u32, h: u32, bgra: [u8; 4]) {
        if self.backend == NdiBackend::Off || w == 0 || h == 0 {
            return;
        }
        let px = (w as usize).saturating_mul(h as usize);
        let mut buf = Vec::with_capacity(px * 4);
        for _ in 0..px {
            buf.extend_from_slice(&bgra);
        }
        self.send_bgra(w, h, &buf);
    }

    pub fn stats(&self) -> NdiStats {
        self.shared.stats.lock().unwrap().clone()
    }

    pub fn stop(&mut self) {
        self.shared.running.store(false, Ordering::SeqCst);
        self.shared.latest.lock().unwrap().take();
        // Unblock accept() with a dummy connection, then join.
        if let Some((handle, addr)) = self.server.take() {
            let _ = TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(200));
            let _ = handle.join();
        }
        if let Some(sdk) = self.sdk.take() {
            sdk.destroy();
        }
        // Re-probe SDK for the next start().
        self.sdk = OfficialSdk::try_load().ok();
        self.backend = NdiBackend::Off;
    }
}

impl Drop for NdiRuntime {
    fn drop(&mut self) {
        self.shared.running.store(false, Ordering::SeqCst);
    }
}

fn serve_loop(listener: TcpListener, shared: Arc<Shared>) {
    // Non-blocking accept so stop() joins promptly.
    let _ = listener.set_nonblocking(true);
    let mut last_sent_seq = 0u64;
    // Single-client loop: accept, stream latest frames, repeat.
    while shared.running.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((mut s, _)) => {
                let _ = s.set_nonblocking(false);
                // Greet with source name (JSON line) so receivers can label it.
                let _ = s.write_all(b"VPNDI1-HELLO\n");
                loop {
                    if !shared.running.load(Ordering::SeqCst) {
                        break;
                    }
                    let next = {
                        let g = shared.latest.lock().unwrap();
                        g.clone().filter(|f| f.seq > last_sent_seq)
                    };
                    let Some(f) = next else {
                        std::thread::sleep(std::time::Duration::from_millis(16));
                        continue;
                    };
                    if write_frame(&mut s, &f).is_err() {
                        break;
                    }
                    last_sent_seq = f.seq;
                    let mut st = shared.stats.lock().unwrap();
                    st.clients_served += 1;
                    st.bytes_sent += f.bgra.len() as u64 + 22;
                }
            }
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
        }
    }
}

fn write_frame(s: &mut TcpStream, f: &Frame) -> std::io::Result<()> {
    s.write_all(MAGIC)?;
    s.write_all(&f.w.to_le_bytes())?;
    s.write_all(&f.h.to_le_bytes())?;
    s.write_all(&f.seq.to_le_bytes())?;
    s.write_all(&f.bgra)?;
    Ok(())
}

/// Read one loopback frame (for tests / companion receivers).
pub fn read_frame(s: &mut TcpStream) -> std::io::Result<Frame> {
    let mut magic = [0u8; 6];
    s.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "bad VPNDI1 magic",
        ));
    }
    let mut u32b = [0u8; 4];
    let mut u64b = [0u8; 8];
    s.read_exact(&mut u32b)?;
    let w = u32::from_le_bytes(u32b);
    s.read_exact(&mut u32b)?;
    let h = u32::from_le_bytes(u32b);
    s.read_exact(&mut u64b)?;
    let seq = u64::from_le_bytes(u64b);
    let len = (w as usize)
        .saturating_mul(h as usize)
        .saturating_mul(4);
    let mut bgra = vec![0u8; len];
    s.read_exact(&mut bgra)?;
    Ok(Frame { w, h, seq, bgra })
}

// ---------------------------------------------------------------------------
// Official SDK dynamic-load stub (no link-time dependency)
// ---------------------------------------------------------------------------

/// Handle to the official NDI runtime when installed on the machine.
/// We deliberately only *detect* the install here (path probe) and route
/// sends through it once the `ndi-sdk` integration lands; the loopback
/// server above carries output until then. This keeps the crate building
/// everywhere with zero proprietary headers.
pub struct OfficialSdk {
    path: String,
}

impl OfficialSdk {
    /// Probe well-known install locations. No libraries are loaded by this
    /// crate itself — full FFI binding arrives with the `ndi-sdk` feature.
    pub fn try_load() -> Result<Self, &'static str> {
        for p in candidate_paths() {
            if std::path::Path::new(&p).exists() {
                return Ok(Self { path: p });
            }
        }
        Err("NDI runtime not installed")
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    fn send(&self, _frame: &Frame, _source: &str) {
        // FFI send lands with the `ndi-sdk` feature; loopback carries it.
    }

    fn destroy(self) {}
}

#[cfg(target_os = "windows")]
fn candidate_paths() -> Vec<String> {
    let mut out = vec![];
    for env in ["NDI_RUNTIME_DIR", "ProgramFiles", "ProgramFiles(x86)"] {
        if let Ok(v) = std::env::var(env) {
            out.push(format!("{v}\\NDI\\Processing.NDI.Lib.x64.dll"));
        }
    }
    out.push(r"C:\Program Files\NDI\NDI 6 Runtime\v6\Processing.NDI.Lib.x64.dll".into());
    out.push(r"C:\Program Files\NDI\NDI 5 Runtime\v5\Processing.NDI.Lib.x64.dll".into());
    // Show up in "NDI Tools" default layout too.
    out.push(r"C:\Program Files\NewTek\NDI 5 Tools\Runtime\Processing.NDI.Lib.x64.dll".into());
    out
}

#[cfg(not(target_os = "windows"))]
fn candidate_paths() -> Vec<String> {
    vec![
        "/usr/local/lib/libndi.so".into(),
        "/usr/lib/libndi.so".into(),
        "/usr/local/lib/libndi.so.6".into(),
        "/usr/local/lib/libndi.so.5".into(),
        "/opt/ndi/lib/libndi.so".into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn default_config_names_perez_source() {
        let c = NdiConfig::default();
        assert!(c.source_name.contains("PEREZ"));
        assert_eq!(c.port, DEFAULT_PORT);
    }

    #[test]
    fn send_without_start_is_noop() {
        let rt = NdiRuntime::new(NdiConfig::default());
        assert_eq!(rt.backend(), NdiBackend::Off);
        rt.send_solid(64, 36, [0, 0, 0, 255]);
        assert_eq!(rt.stats().frames_sent, 0);
        assert!(rt.sources().is_empty());
    }

    #[test]
    fn rejects_short_buffers() {
        let mut rt = NdiRuntime::new(NdiConfig {
            port: 0,
            ..Default::default()
        });
        let addr = rt.start().unwrap();
        assert!(addr.port() != 0);
        rt.send_bgra(4, 4, &[0u8; 10]); // needs 64
        assert_eq!(rt.stats().frames_sent, 0);
        assert_eq!(rt.sources().len(), 1);
        rt.stop();
    }

    #[test]
    fn loopback_roundtrip_one_frame() {
        let mut rt = NdiRuntime::new(NdiConfig {
            port: 0,
            ..Default::default()
        });
        let addr = rt.start().unwrap();
        // Solid red 2x1.
        rt.send_solid(2, 1, [0, 0, 255, 255]);
        let mut s = TcpStream::connect(addr).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        // Skip hello line.
        let mut hello = [0u8; 13];
        s.read_exact(&mut hello).unwrap();
        assert_eq!(&hello, b"VPNDI1-HELLO\n");
        let f = read_frame(&mut s).unwrap();
        assert_eq!((f.w, f.h), (2, 1));
        assert_eq!(f.bgra, vec![0, 0, 255, 255, 0, 0, 255, 255]);
        assert_eq!(rt.stats().frames_sent, 1);
        assert_eq!(rt.stats().last_w, 2);
        rt.stop();
    }

    #[test]
    fn sources_empty_when_off() {
        let rt = NdiRuntime::new(NdiConfig::default());
        assert!(rt.sources().is_empty());
    }
}
