//! Build script for perez-rs.
//!
//! Windows-only icon resource compilation (cfg-gated so Linux and macOS builds
//! are completely unaffected and do not invoke winres).
//!
//! The ico is rendered directly from the vector master `assets/perez.svg`
//! (`windows/perez.ico`); `assets/icons-dist/*.png` are PNG masters of the
//! same tile, not the byte source of the ico.

fn main() {
    println!("cargo:rerun-if-changed=../windows/perez.ico");
    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("../windows/perez.ico");
        if let Err(e) = res.compile() {
            panic!("Failed to compile Windows resource (../windows/perez.ico): {e}");
        }
    }
}
