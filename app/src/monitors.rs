//! Physical monitor geometries on Linux via RandR.
//!
//! GPUI 0.2.2's X11 backend enumerates X *screens* (`cx.displays()`), and a
//! normal dual-monitor desktop is a single X screen spanning both outputs —
//! so the monitor buttons could never resolve Display 2 and the projector
//! always fell back to the primary. `xrandr --listmonitors` reports the
//! real output rectangles; the projector opens borderless inside the
//! chosen one. No network, one local subprocess, graceful `None` when
//! `xrandr` is missing (single-display fallback).

/// One connected output, in screen pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Monitor {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub primary: bool,
}

/// Connected outputs, primary first. Empty when `xrandr` is unavailable.
pub fn list() -> Vec<Monitor> {
    let out = std::process::Command::new("xrandr")
        .arg("--listmonitors")
        .output();
    let Ok(out) = out else { return Vec::new() };
    if !out.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut monitors = parse_listmonitors(&text);
    sort_primary_first(&mut monitors);
    monitors
}

/// Parse `xrandr --listmonitors` output. Pure so the format is unit-tested.
///
/// ```text
/// Monitors: 2
///  0: +*DP-1 1920/510x1080/290+0+0  DP-1
///  1: +HDMI-1 1920/510x1080/290+1920+0  HDMI-1
/// ```
/// Flags: `+` connected, `*` primary. Geometry is `WxH+X+Y` (the `/mm`
/// physical sizes are ignored).
fn parse_listmonitors(text: &str) -> Vec<Monitor> {
    let mut out = Vec::new();
    for line in text.lines().skip(1) {
        let mut parts = line.split_whitespace();
        let Some(_index) = parts.next() else { continue };
        let Some(flags) = parts.next() else { continue };
        let Some(geom) = parts.next() else { continue };
        let name = parts.next().unwrap_or("").to_string();
        let Some(rect) = parse_geometry(geom) else { continue };
        out.push(Monitor {
            name,
            x: rect.0,
            y: rect.1,
            w: rect.2,
            h: rect.3,
            primary: flags.contains('*'),
        });
    }
    out
}

/// Primary-first ordering shared by `list` (pure so tests pin it
/// instead of re-asserting their own sort).
pub fn sort_primary_first(monitors: &mut [Monitor]) {
    monitors.sort_by_key(|m| !m.primary);
}

/// Resolve the projector target: a persisted output name first (stable
/// across hotplug reorderings), then the position index, then `None`.
/// Pure so the fallback chain is unit-testable.
pub fn resolve_target(monitors: &[Monitor], name: Option<&str>, idx: usize) -> Option<Monitor> {
    if let Some(want) = name.filter(|n| !n.is_empty()) {
        if let Some(m) = monitors.iter().find(|m| m.name == want) {
            return Some(m.clone());
        }
    }
    monitors.get(idx).cloned()
}
/// Parse `1920/510x1080/290+0+0` → `(x, y, w, h)`. Pure for tests.
fn parse_geometry(geom: &str) -> Option<(i32, i32, i32, i32)> {
    // Strip the `/mm` physical sizes: `1920/510x1080/290+0+0`.
    let (w_part, rest) = geom.split_once('x')?;
    let w: i32 = w_part.split('/').next()?.parse().ok()?;
    let (h_part, offsets) = rest.split_once('/')?;
    let h: i32 = h_part.parse().ok()?;
    // Offsets follow the height's physical size (`500+0+0`, `290+1920+0`):
    // x and y are the last two `+`-separated numbers.
    let mut nums = offsets.rsplit('+');
    let y: i32 = nums.next()?.parse().ok()?;
    let x: i32 = nums.next()?.parse().ok()?;
    (w > 0 && h > 0).then_some((x, y, w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO_MONITORS: &str = "Monitors: 2\n 0: +*DP-1 1920/510x1080/290+0+0  DP-1\n 1: +HDMI-1 1920/510x1080/290+1920+0  HDMI-1\n";

    #[test]
    fn parses_two_monitor_layout() {
        let ms = parse_listmonitors(TWO_MONITORS);
        assert_eq!(ms.len(), 2);
        assert_eq!(
            ms[0],
            Monitor { name: "DP-1".to_string(), x: 0, y: 0, w: 1920, h: 1080, primary: true }
        );
        assert_eq!(
            ms[1],
            Monitor { name: "HDMI-1".to_string(), x: 1920, y: 0, w: 1920, h: 1080, primary: false }
        );
    }

    #[test]
    fn resolves_by_name_then_index() {
        let ms = parse_listmonitors(TWO_MONITORS);
        // Name wins even when the index points elsewhere (hotplug reorder).
        assert_eq!(
            resolve_target(&ms, Some("HDMI-1"), 0).map(|m| m.name),
            Some("HDMI-1".to_string())
        );
        // Unknown name falls back to the index.
        assert_eq!(
            resolve_target(&ms, Some("GONE"), 1).map(|m| m.name),
            Some("HDMI-1".to_string())
        );
        // No name: plain index, out-of-range is None.
        assert_eq!(
            resolve_target(&ms, None, 0).map(|m| m.name),
            Some("DP-1".to_string())
        );
        assert_eq!(resolve_target(&ms, None, 9), None);
    }

    #[test]
    fn primary_first_ordering_is_stable() {
        // The rule `list` applies, exercised directly: primary sorts
        // before secondary regardless of input order.
        let mut ms = parse_listmonitors(TWO_MONITORS);
        ms.reverse();
        sort_primary_first(&mut ms);
        assert!(ms[0].primary);
        assert_eq!(ms[0].name, "DP-1");
    }

    #[test]
    fn rejects_garbage_geometry() {
        assert_eq!(parse_geometry("1920x1080+0+0"), None);
        assert_eq!(parse_geometry("0x1080+0+0"), None);
        assert_eq!(parse_geometry("nope"), None);
        assert_eq!(
            parse_geometry("3840/800x2160/500+0+0"),
            Some((0, 0, 3840, 2160))
        );
    }

    #[test]
    fn empty_or_missing_output_yields_no_monitors() {
        assert!(parse_listmonitors("").is_empty());
        assert!(parse_listmonitors("Monitors: 0\n").is_empty());
        assert!(parse_listmonitors("can't open display\n").is_empty());
    }
}
