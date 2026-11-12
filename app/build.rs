//! Build script for perez-rs.
//!
//! Windows-only icon resource compilation. The `cfg(target_os)` gate tests the
//! HOST, and `winres` is a Windows-only build-dependency, so cross-compiling
//! for Windows from Linux/macOS skips the icon (native Windows CI covers
//! releases) — that limitation is deliberate, not an accident of the gate.
//!
//! The ico is rendered directly from the vector master `assets/perez.svg`
//! (`windows/perez.ico`); `assets/icons-dist/*.png` are PNG masters of the
//! same tile, not the byte source of the ico.

fn main() {
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rerun-if-changed=../windows/perez.ico");
        let mut res = winres::WindowsResource::new();
        res.set_icon("../windows/perez.ico");
        if let Err(e) = res.compile() {
            panic!("Failed to compile Windows resource (../windows/perez.ico): {e}");
        }
    }
}
