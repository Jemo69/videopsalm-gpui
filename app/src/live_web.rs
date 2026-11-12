//! Opt-in live web view: a real browser window for website slides.
//!
//! Default posture is unchanged (offline link tiles, never fetched). When the
//! operator flips the Live toggle on, website slides (`WebSite`/`YouTube`
//! footers, including `<iframe>` cards) open in a hardened browser window
//! placed on the projector monitor:
//!
//! - Loopback URLs (`localhost`, `127.0.0.1`, `[::1]`, `file:`) are exempt —
//!   no packet leaves the machine, so the offline envelope holds.
//! - Remote hosts need a per-session Allow (one click, this show only) and a
//!   TCP reachability check. Decline/failure → the offline tile stays.
//! - Chromium-family launches `--incognito` inside a dedicated throwaway
//!   profile with background networking, sync, and component updates off.
//!   Firefox falls back to a private window (no geometry control).
//! - The window spawns when a website slide goes live and dies the moment it
//!   leaves. Fullscreen geometry is used ONLY on a non-primary projector
//!   output — on the control screen it opens windowed so the operator is
//!   never locked out of their own console.
//!
//! No HTTP is ever fetched by this app: the only network touch is a TCP
//! handshake for the reachability check (loopback included, so a dead stage
//! server reports honestly instead of opening a blank window).

use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::Duration;
use vp_core::slides::Slide;

/// Which browser drives the live view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BrowserKind {
    Chromium,
    Firefox,
}

/// Map an executable name (or full path) to its browser family.
/// Edge is Chromium-family: it honors `--user-data-dir`, `--app` and the
/// disable-* flags used here. macOS bundles keep their spaced names
/// (`Google Chrome`, `Microsoft Edge`) — match those too.
pub fn candidate_name(path: &str) -> Option<BrowserKind> {
    let lower = path.to_ascii_lowercase();
    let base = lower.rsplit(['/', '\\']).next().unwrap_or(&lower);
    let base = base.strip_suffix(".exe").unwrap_or(base);
    let base = base.strip_suffix(".app").unwrap_or(base);
    match base {
        "chromium" | "chromium-browser" | "google-chrome" | "google-chrome-stable" | "chrome" | "google chrome" | "msedge" | "microsoft-edge" | "microsoft edge" => {
            Some(BrowserKind::Chromium)
        }
        "firefox" => Some(BrowserKind::Firefox),
        _ => None,
    }
}

/// A browser binary found on `PATH`, in preference order.
pub fn pick_from_path(exists: &dyn Fn(&str) -> Option<PathBuf>) -> Option<(BrowserKind, PathBuf)> {
    for name in [
        "chromium",
        "chromium-browser",
        "google-chrome",
        "google-chrome-stable",
        "chrome",
        "msedge",
        "firefox",
    ] {
        if let Some(p) = exists(name) {
            if let Some(kind) = candidate_name(name) {
                return Some((kind, p));
            }
        }
    }
    None
}

/// Well-known install locations for OSes where browsers are not on `PATH`
/// (Windows, macOS). Linux is covered by `PATH`; a few fixed spots help
/// snap/opt installs.
fn well_known_browsers() -> Vec<PathBuf> {
    let mut out = Vec::new();
    #[cfg(windows)]
    {
        for root in [
            std::env::var("PROGRAMFILES").unwrap_or_default(),
            std::env::var("PROGRAMFILES(X86)").unwrap_or_default(),
            std::env::var("LOCALAPPDATA").unwrap_or_default(),
        ] {
            if root.is_empty() {
                continue;
            }
            for rel in [
                "Google\\Chrome\\Application\\chrome.exe",
                "Microsoft\\Edge\\Application\\msedge.exe",
                "Mozilla Firefox\\firefox.exe",
                "Chromium\\Application\\chrome.exe",
            ] {
                out.push(PathBuf::from(&root).join(rel));
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        for app in [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "/Applications/Firefox.app/Contents/MacOS/firefox",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
        ] {
            out.push(PathBuf::from(app));
        }
    }
    #[cfg(target_os = "linux")]
    {
        for p in [
            "/snap/bin/chromium",
            "/opt/google/chrome/chrome",
            "/opt/microsoft/msedge/msedge",
            "/usr/lib/firefox/firefox",
        ] {
            out.push(PathBuf::from(p));
        }
    }
    out
}

/// Search `PATH`, then well-known install locations, for a supported
/// browser (Chromium-family first, Firefox fallback).
pub fn pick_browser() -> Option<(BrowserKind, PathBuf)> {
    let path_env = std::env::var_os("PATH").unwrap_or_default();
    let dirs: Vec<PathBuf> = std::env::split_paths(&path_env).collect();
    let on_path = |name: &str| -> Option<PathBuf> {
        dirs.iter().find_map(|d| {
            let p = d.join(name);
            #[cfg(windows)]
            let p = if p.extension().is_none() {
                p.with_extension("exe")
            } else {
                p
            };
            if p.is_file() {
                Some(p)
            } else {
                None
            }
        })
    };
    if let found @ Some(_) = pick_from_path(&on_path) {
        return found;
    }
    well_known_browsers().into_iter().find_map(|p| {
        if p.is_file() {
            candidate_name(&p.to_string_lossy()).map(|kind| (kind, p))
        } else {
            None
        }
    })
}

/// `file:` is exempt only for local files (`file:///…` or
/// `file://localhost/…`): `file://server/share` is an SMB read that puts
/// packets on the wire and goes through Allow like any remote host.
/// Authority splits on `/`, `\\`, `?`, `#` (WHATWG treats `\\` as `/` for
/// special schemes — `http://evil.com\\@localhost/` is evil.com, not
/// localhost). Names resolve to loopback only for `localhost` (optional
/// trailing dot); every `127.0.0.0/8` and `::1` matches via `IpAddr`.
pub fn is_loopback(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("file:") {
        let authority = rest.split(['/', '\\']).nth(2).unwrap_or("");
        return authority.is_empty() || authority == "localhost";
    }
    let rest = match lower.split_once("://") {
        Some((_, r)) => r,
        None => return false,
    };
    let authority = rest.split(['/', '\\', '?', '#']).next().unwrap_or("");
    let host_part = authority.split('@').next_back().unwrap_or(authority);
    // Bracketed IPv6: whatever follows `]` must be empty or `:port` —
    // `[::1].evil.com` is not an IP literal, it is a bad hostname.
    let host: &str = if let Some(h) = host_part.strip_prefix('[') {
        let Some((inner, after)) = h.split_once(']') else {
            return false;
        };
        if !after.is_empty() && !after.starts_with(':') {
            return false;
        }
        inner
    } else {
        host_part.split(':').next().unwrap_or("")
    };
    let host = host.trim_end_matches('.');
    if host == "localhost" {
        return true;
    }
    host.parse::<std::net::IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

/// `(host, port)` for an `http(s)` URL. Defaults 80/443. `None` when the
/// URL is not http(s), has no usable host, or carries an invalid port
/// (`:99999`, `:bad`, empty) — WHATWG would reject those too, and probing
/// the default port instead would check somewhere the browser never goes.
/// Bracketed IPv6 comes back bare (`::1`, not `[::1]`) so callers can dial
/// without tripping getaddrinfo.
pub fn host_port(url: &str) -> Option<(String, u16)> {
    let t = url.trim();
    let (scheme, rest) = t.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    if rest.is_empty() || rest.starts_with('/') {
        return None;
    }
    let authority = rest
        .split(|c| c == '/' || c == '\\' || c == '?' || c == '#')
        .next()
        .unwrap_or("");
    let authority = authority.split('@').next_back().unwrap_or(authority);
    if authority.is_empty() {
        return None;
    }
    let default_port = if scheme == "https" { 443 } else { 80 };
    if let Some(stripped) = authority.strip_prefix('[') {
        // [v6] or [v6]:port — bare host out, numeric port or reject.
        // Anything else after `]` (`[::1].evil.com`) is a bad hostname.
        let (host, port) = match stripped.split_once(']') {
            Some((h, after)) => {
                let port = if after.is_empty() {
                    Ok(default_port)
                } else if let Some(p) = after.strip_prefix(':') {
                    if p.is_empty() {
                        Ok(default_port)
                    } else {
                        p.parse::<u16>().map_err(|_| ())
                    }
                } else {
                    return None;
                };
                (h.to_string(), port.ok()?)
            }
            None => return None,
        };
        if host.is_empty() {
            return None;
        }
        return Some((host, port));
    }
    match authority.split_once(':') {
        Some((h, p)) if !h.is_empty() => {
            let port = if p.is_empty() {
                default_port
            } else {
                p.parse::<u16>().ok()?
            };
            Some((h.to_string(), port))
        }
        _ if !authority.is_empty() && !authority.contains(':') => {
            Some((authority.to_string(), default_port))
        }
        _ => None,
    }
}

/// TCP handshake only (no HTTP fetch): is something serving this URL?
/// Local `file:` URLs always pass; anything else must dial. IP literals dial directly (no resolver on the
/// UI thread); names resolve normally and only the first two addresses are
/// tried, so the check stays bounded by ~2× `timeout` worst case.
pub fn reachable(url: &str, timeout: Duration) -> bool {
    if url.trim().to_ascii_lowercase().starts_with("file:") {
        return is_loopback(url);
    }
    let Some((host, port)) = host_port(url) else {
        return false;
    };
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::new(ip, port),
            timeout,
        )
        .is_ok();
    }
    let addrs: Vec<std::net::SocketAddr> =
        std::net::ToSocketAddrs::to_socket_addrs(&(host.as_str(), port))
            .map(|it| it.take(2).collect())
            .unwrap_or_default();
    addrs
        .into_iter()
        .any(|a| std::net::TcpStream::connect_timeout(&a, timeout).is_ok())
}

/// The live-view URL for a projected slide, if it is a website slide with a
/// valid URL body. Footer templates rename the footer, so templated cards
/// stay tiles (documented residual).
///
/// The slide body is the *wrapped* URL (`slides.rs` wraps at `max_chars`,
/// default 40): rejoin every line, or long URLs lose their tail and the
/// browser opens the wrong page (first fragment only).
pub fn live_url_for_slide(slide: &Slide) -> Option<String> {
    if slide.footer != "WebSite" && slide.footer != "YouTube" {
        return None;
    }
    let url: String = slide.lines.join("");
    if url.trim().is_empty() {
        return None;
    }
    vp_core::schema::normalize_website_url(url.trim())
}

/// Chromium-family launch: incognito app window in a throwaway profile.
/// `geometry` positions the window on a non-primary projector output;
/// `fullscreen` takes over the screen (kiosk-style): set when our own
/// projector window is already fullscreen (a windowed browser would open
/// underneath it, invisible) or on a dedicated output. Never on the plain
/// control screen. Crash reporting, field trials, metrics, pings and Safe
/// Browsing updates are off: the window renders the operator's page,
/// nothing else.
/// `profile` stays an `OsString` end to end: a lossy `String` round-trip
/// could point Chrome at a mangled dir while `hide()` cleans the real one.
pub fn chromium_args(
    url: &str,
    geometry: Option<(i32, i32, i32, i32)>,
    fullscreen: bool,
    profile: &std::path::Path,
) -> Vec<std::ffi::OsString> {
    // Stage overlays are transparent: the page (a browser-source output)
    // paints only its lyrics, and the room sees our own background/lyrics
    // through it. Both flags are needed — the background colour makes the
    // window alpha-capable, the visuals flag lets X11 honour it.
    chromium_args_full(url, geometry, fullscreen, profile, true, true)
}

/// [`chromium_args`] with both the alpha and the incognito decision explicit.
///
/// `incognito` is FALSE only for an operator-owned persistent profile: the
/// point of that profile is the page's own stored settings (Selah keeps
/// `transparentBackground` in localStorage), and an incognito window throws
/// exactly that away. A throwaway profile keeps incognito — belt and braces
/// on isolation, since the directory dies with the window anyway.
pub fn chromium_args_full(
    url: &str,
    geometry: Option<(i32, i32, i32, i32)>,
    fullscreen: bool,
    profile: &std::path::Path,
    transparent: bool,
    incognito: bool,
) -> Vec<std::ffi::OsString> {
    chromium_args_impl(url, geometry, fullscreen, profile, transparent, incognito)
}

/// What paints behind the page (pure, tested).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayField {
    /// Alpha window: the presentation (background, lyrics) shows through the
    /// transparent page. Needs a compositing manager.
    Alpha,
    /// Opaque black behind the page: correct text contrast anywhere, but it
    /// covers the presentation behind it.
    Black,
}

/// Pick the field for the live overlay (pure, tested).
///
/// `VP_LIVE_FIELD=alpha|black` wins when set. Otherwise: a compositing
/// desktop gets the real thing (alpha — the presentation background shows
/// through the transparent page); without a compositor an ARGB window shows
/// the X default, so we paint black instead and say so.
pub fn overlay_field(has_compositor: bool, env: Option<&str>) -> OverlayField {
    match env {
        Some("alpha") => OverlayField::Alpha,
        Some("black") => OverlayField::Black,
        _ if has_compositor => OverlayField::Alpha,
        _ => OverlayField::Black,
    }
}

/// Whether a compositing manager is present, by process name — the cheap,
/// dependency-free check available to us. Unknown desktops default to "no",
/// which selects the safe black field (never a white window).
pub fn has_compositor() -> bool {
    const NAMES: [&str; 10] = [
        "picom", "compton", "xcompmgr", "mutter", "kwin_x11", "kwin_wayland",
        "gnome-shell", "sway", "weston", "compiz",
    ];
    let Ok(entries) = std::fs::read_dir("/proc") else { return false };
    let mut found = false;
    entries.flatten().any(|e| {
        let comm = e.path().join("comm");
        let Ok(name) = std::fs::read_to_string(comm) else { return false };
        let name = name.trim();
        if NAMES.contains(&name) {
            // Mutter/KWin are compositing WMs: a hit means alpha is safe.
            found = true;
        }
        false
    }) || found
}

/// Wrapper page for the transparent overlay (pure, tested).
///
/// The stage page paints *transparently* (Selah's `transparentBackground`),
/// so what shows behind it is whatever the browser window paints. A headed
/// Chromium window without a compositor paints white there — white text on
/// white, the failure the conductor saw. This wrapper gives the page the
/// black field it expects: our own document paints black and frames the
/// page, so the result is white verse over black with no browser chrome,
/// with or without a compositor.
pub fn overlay_wrapper_html(target: &str) -> String {
    overlay_wrapper_html_with_bg(target, "background:#000")
}

/// [`overlay_wrapper_html`] with the field the page sits on: the live
/// presentation's own background, so a stage overlay over a colour or image
/// background looks like the room's picture instead of a black slab.
pub fn overlay_wrapper_html_with_bg(target: &str, field_css: &str) -> String {
    let field = field_css;
    // Attribute-escape the target: it goes into an HTML attribute.
    let safe = target
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>Overlay</title>\
<style>html,body{{margin:0;height:100%;{field};overflow:hidden}}\
iframe{{border:0;width:100vw;height:100vh;background:transparent}}</style></head>\
<body><iframe src=\"{safe}\" allow=\"autoplay\"></iframe></body></html>"
    )
}

/// Browser profile for a live view (pure, tested).
///
/// Default is a throwaway directory: no history, no cookies, nothing
/// survives the show. A stage page may keep its own settings in the
/// browser profile (localStorage) — e.g. Selah's `transparentBackground`,
/// which decides whether the page paints transparent — so an operator can
/// opt into a persistent profile with `VP_LIVE_PROFILE=<dir>` (or
/// `VP_LIVE_PROFILE=persist` for the default location): flip the page's own
/// toggle once in that profile and every later show keeps it.
pub fn profile_dir(throwaway: PathBuf, persist: Option<&str>, fallback_dir: &std::path::Path) -> PathBuf {
    // A bare name resolves under `fallback_dir` (the app-data default),
    // never the process CWD: a stray value must not scatter
    // operator-owned directories across wherever the app launched.
    match persist {
        None | Some("") => throwaway,
        Some("persist") | Some("1") => fallback_dir.to_path_buf(),
        Some(dir) => {
            let p = PathBuf::from(dir);
            if p.is_absolute() {
                p
            } else {
                fallback_dir.join(p)
            }
        }
    }
}

/// Whether the live window carries alpha (pure, tested). Transparent by
/// default — the page is an overlay ON the presentation — with an explicit
/// opaque fallback for desktops that run no compositor.
pub fn overlay_alpha(transparent: bool, opaque_env: Option<&str>) -> bool {
    transparent && opaque_env != Some("1")
}

/// Window background for the overlay when the page itself paints nothing:
/// opaque black, so white lyric text always has a black field behind it and
/// a page that forgets its own transparency can never flash white at the
/// room (pure, tested).
pub fn overlay_background() -> &'static str {
    "FF000000"
}

/// [`chromium_args`] with the transparency decision explicit (pure, tested).
pub fn chromium_args_with(
    url: &str,
    geometry: Option<(i32, i32, i32, i32)>,
    fullscreen: bool,
    profile: &std::path::Path,
    transparent: bool,
) -> Vec<std::ffi::OsString> {
    chromium_args_full(url, geometry, fullscreen, profile, transparent, true)
}

/// Body shared by every variant (see [`chromium_args_full`] for the flags).
fn chromium_args_impl(
    url: &str,
    geometry: Option<(i32, i32, i32, i32)>,
    fullscreen: bool,
    profile: &std::path::Path,
    transparent: bool,
    incognito: bool,
) -> Vec<std::ffi::OsString> {
    use std::ffi::OsString;
    let mut args: Vec<OsString> = vec![
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
        "--disable-background-networking".into(),
        "--disable-sync".into(),
        "--disable-component-update".into(),
        "--disable-breakpad".into(),
        "--disable-crash-reporter".into(),
        "--disable-field-trial-config".into(),
        "--metrics-recording-only".into(),
        "--no-pings".into(),
        "--safebrowsing-disable-auto-update".into(),
        "--disable-extensions".into(),
        "--disable-features=Translate,OptimizationHints,MediaRouter".into(),
    ];
    // The overlay is transparent ON THE PRESENTATION: the live page window
    // carries alpha, so whatever the projector window is drawing (background,
    // black base, lyrics) stays visible behind the page's own content. That
    // is the conductor's spec — white page text over the presentation, never
    // a second opaque screen and never browser chrome.
    //
    // Opaque black remains available as a deliberate fallback for desktops
    // without a compositor (VP_LIVE_OPAQUE=1), where an ARGB window would
    // otherwise show the X server's default as a white rectangle.
    if incognito {
        args.insert(0, "--incognito".into());
    }
    let opaque_env = std::env::var("VP_LIVE_OPAQUE").ok();
    if overlay_alpha(transparent, opaque_env.as_deref()) {
        args.push("--default-background-color=00000000".into());
        args.push("--enable-transparent-visuals".into());
    } else {
        args.push(format!("--default-background-color={}", overlay_background()).into());
    }
    if let Some((x, y, w, h)) = geometry {
        args.push(format!("--window-position={x},{y}").into());
        args.push(format!("--window-size={w},{h}").into());
    }
    if fullscreen {
        // Take over: our own projector tile (the placeholder for this very
        // page) would otherwise cover the window. Never set on the plain
        // control screen — it would lock the operator out of their console.
        args.push("--start-fullscreen".into());
    }
    let mut dir: OsString = "--user-data-dir=".into();
    dir.push(profile);
    args.insert(0, dir);
    args.push(format!("--app={url}").into());
    args
}

/// Firefox fallback: private window in a throwaway profile, forced into a
/// new instance so the child handle actually owns the window (otherwise the
/// URL hands off to a running Firefox and `hide()` kills nothing).
/// No geometry control: on a projector output it opens windowed and the
/// status says so (the Chromium path is the one that takes over).
pub fn firefox_args(url: &str, profile: &std::path::Path) -> Vec<std::ffi::OsString> {
    use std::ffi::OsString;
    vec![
        "--new-instance".into(),
        "--profile".into(),
        profile.as_os_str().to_os_string(),
        "--private-window".into(),
        OsString::from(url),
    ]
}

/// The running live-view window, if any. Killed on hide and on drop so a
/// browser never outlives the show. The profile is unique per spawn (0700)
/// and removed on hide: no residue, no cross-show handoff, no pre-seeding.
pub struct LiveView {
    child: Option<Child>,
    profile: Option<PathBuf>,
    /// Operator-owned profile (VP_LIVE_PROFILE): never deleted on hide.
    profile_persistent: bool,
    /// Wrapper page the browser is loading; deleted when the view hides
    /// (deleting it right after spawn races the browser's own read).
    wrapper: Option<PathBuf>,
    current: Option<(String, Option<(i32, i32, i32, i32)>, bool)>,
}

impl LiveView {
    pub fn new() -> Self {
        LiveView {
            child: None,
            profile: None,
            profile_persistent: false,
            wrapper: None,
            current: None,
        }
    }

    /// `true` when exactly this window is already up and alive. Lets the
    /// caller skip probes and subprocesses on re-pushes (a dead child or a
    /// `try_wait` error counts as down — fail closed, respawn — so a stale
    /// handle can never pin a phantom window).
    pub fn showing(
        &mut self,
        url: &str,
        geometry: Option<(i32, i32, i32, i32)>,
        room_fullscreen: bool,
    ) -> bool {
        if !self.current.as_ref().is_some_and(|(u, g, f)| {
            u == url && *g == geometry && *f == room_fullscreen
        }) {
            return false;
        }
        match self.child.as_mut() {
            // `try_wait` reaps a dead child as a side effect. An error
            // counts as down (fail closed, respawn) — never pin a phantom.
            Some(child) => matches!(child.try_wait(), Ok(None)),
            None => false,
        }
    }

    pub fn show(
        &mut self,
        bin: &PathBuf,
        kind: &BrowserKind,
        url: &str,
        geometry: Option<(i32, i32, i32, i32)>,
        room_fullscreen: bool,
        field_css: &str,
    ) -> bool {
        // Already up and alive: re-pushes must not flicker the projector.
        if self.showing(url, geometry, room_fullscreen) {
            return true;
        }
        self.hide();
        let throwaway = std::env::temp_dir().join(format!(
            "videopsalm-live-web-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let persist_env = std::env::var("VP_LIVE_PROFILE").ok();
        // Persistent profile default: the app-data dir, not the temp dir.
        // Tmp is cleaned on reboot/expiry, which would silently lose the
        // page's stored settings (and the transparency toggle with them).
        let persist_dir = vp_core::native_folder::ensure_app_dirs()
            .map(|b| b.join("live-web-profile"))
            .unwrap_or_else(|_| std::env::temp_dir().join("videopsalm-live-web-profile"));
        let profile = profile_dir(throwaway, persist_env.as_deref(), &persist_dir);
        // Persistent profiles already exist: create-or-keep, and never
        // `hide()` (delete) what the operator owns.
        let persistent = persist_env.as_deref().is_some_and(|v| !v.is_empty());
        // The 0700 throwaway profile is the isolation: refuse to spawn
        // into a dir we did not just lock down.
        let created = {
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                std::fs::DirBuilder::new()
                    .mode(0o700)
                    .create(&profile)
                    .is_ok()
            }
            #[cfg(not(unix))]
            {
                std::fs::create_dir_all(&profile).is_ok()
            }
        };
        if !created {
            // A persistent profile may already exist (that is the point:
            // the page's own settings live in it). Only a fresh throwaway
            // directory must be ours.
            if !(persistent && profile.is_dir()) {
                return false;
            }
        }
        // Overlay wrapper: the page paints transparent, so the document
        // around it paints the black field (see overlay_wrapper_html).
        // Field: alpha (presentation shows through) only when a compositor
        // can honour it; otherwise the wrapper's black field.
        let field_env = std::env::var("VP_LIVE_FIELD").ok();
        let field = overlay_field(has_compositor(), field_env.as_deref());
        let wrapper_off = std::env::var("VP_LIVE_WRAPPER").as_deref() == Ok("0")
            || field == OverlayField::Alpha;
        let mut launch_url = url.to_string();
        let mut wrapper: Option<PathBuf> = None;
        if !wrapper_off {
            let path = profile.with_extension("overlay.html");
            if std::fs::write(&path, overlay_wrapper_html_with_bg(url, field_css)).is_ok() {
                launch_url = format!("file://{}", path.display());
                wrapper = Some(path);
            }
        }
        let args = match kind {
            BrowserKind::Chromium => {
                // A persistent profile is only useful with the page's own
                // stored settings intact, so incognito is off for it.
                chromium_args_full(
                    &launch_url,
                    geometry,
                    geometry.is_some() || room_fullscreen,
                    &profile,
                    field == OverlayField::Alpha,
                    !persistent,
                )
            }
            BrowserKind::Firefox => firefox_args(&launch_url, &profile),
        };
        match Command::new(bin)
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(child) => {
                self.child = Some(child);
                self.profile = Some(profile);
                self.profile_persistent = persistent;
                self.wrapper = wrapper;
                self.current = Some((url.to_string(), geometry, room_fullscreen));
                true
            }
            Err(_) => {
                let _ = std::fs::remove_dir_all(&profile);
                false
            }
        }
    }

    pub fn hide(&mut self) {
        self.current = None;
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            // Bounded reap: never block the UI thread on teardown.
            for _ in 0..20 {
                if child.try_wait().ok().flatten().is_some() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
        if let Some(w) = self.wrapper.take() {
            let _ = std::fs::remove_file(w);
        }
        if let Some(profile) = self.profile.take() {
            if !self.profile_persistent {
                let _ = std::fs::remove_dir_all(profile);
            }
        }
        self.profile_persistent = false;
    }
}

impl Default for LiveView {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for LiveView {
    fn drop(&mut self) {
        self.hide();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_detection_covers_local_origins() {
        for u in [
            "http://localhost:4000/#/stage",
            "http://localhost/x",
            "https://127.0.0.1:8080/a",
            "http://[::1]:3000/",
            "file:///tmp/stage.html",
        ] {
            assert!(is_loopback(u), "{u}");
        }
        for u in [
            "https://example.com/live",
            "https://www.youtube.com/watch?v=x",
            "http://192.168.1.10/x",
            "http://stage.local/x",
            "http://127.0.0.1.evil.com/",
        ] {
            assert!(!is_loopback(u), "{u}");
        }
        // WHATWG treats `\` as `/`: the real host here is evil.com.
        assert!(!is_loopback(r"http://evil.com\@localhost/"));
        // The whole 127/8 is loopback, not just .0.0.1.
        assert!(is_loopback("http://127.0.0.2:4000/x"));
        // Bracket-suffix tricks are not IP literals.
        assert!(!is_loopback("http://[::1].evil.com/"));
        assert!(!is_loopback("http://[::1]x:8080/"));
        // file://server/share is SMB, not a local file.
        assert!(!is_loopback("file://server/share/stage.html"));
        assert!(is_loopback("file://localhost/stage.html"));
    }

    #[test]
    fn host_port_parses_defaults_and_brackets() {
        assert_eq!(
            host_port("http://localhost:4000/#/stage"),
            Some(("localhost".to_string(), 4000))
        );
        assert_eq!(
            host_port("https://example.com/live"),
            Some(("example.com".to_string(), 443))
        );
        assert_eq!(
            host_port("http://[::1]:3000/"),
            Some(("::1".to_string(), 3000))
        );
        // Invalid ports reject (never silently probe the default port);
        // an empty port is the default port per WHATWG.
        assert_eq!(host_port("http://host:99999/"), None);
        assert_eq!(host_port("https://host:bad/"), None);
        assert_eq!(host_port("http://host:/"), Some(("host".to_string(), 80)));
        assert_eq!(host_port("javascript:alert(1)"), None);
        assert_eq!(host_port("file:///tmp/x.html"), None);
        assert_eq!(host_port("https://"), None);
        // Bracket-suffix tricks reject instead of misreading the host.
        assert_eq!(host_port("http://[::1].evil.com/"), None);
        assert_eq!(host_port("http://[::1"), None);
    }

    #[test]
    fn candidate_names_map_edge_to_chromium() {
        assert_eq!(candidate_name("msedge"), Some(BrowserKind::Chromium));
        assert_eq!(candidate_name("Google Chrome"), Some(BrowserKind::Chromium));
        assert_eq!(candidate_name("Microsoft Edge"), Some(BrowserKind::Chromium));
        assert_eq!(
            candidate_name(r"C:\Program Files\Microsoft\Edge\Application\msedge.exe"),
            Some(BrowserKind::Chromium)
        );
        assert_eq!(candidate_name("/usr/bin/firefox"), Some(BrowserKind::Firefox));
        assert_eq!(candidate_name("safari"), None);
    }

    #[test]
    fn pick_prefers_chromium_over_firefox() {
        let found = pick_from_path(&|n| match n {
            "firefox" => Some(PathBuf::from("/usr/bin/firefox")),
            _ => None,
        });
        assert_eq!(
            found,
            Some((BrowserKind::Firefox, PathBuf::from("/usr/bin/firefox")))
        );
        let found = pick_from_path(&|n| match n {
            "chromium" => Some(PathBuf::from("/usr/bin/chromium")),
            "firefox" => Some(PathBuf::from("/usr/bin/firefox")),
            _ => None,
        });
        assert_eq!(
            found,
            Some((BrowserKind::Chromium, PathBuf::from("/usr/bin/chromium")))
        );
        assert_eq!(pick_from_path(&|_| None), None);
    }

    #[test]
    fn chromium_args_use_app_mode_with_geometry() {
        let profile = std::path::Path::new("/tmp/p");
        let args = chromium_args("http://localhost:4000/#/stage", Some((1920, 0, 1920, 1080)), true, profile);
        assert!(args.iter().any(|a| a == "--window-position=1920,0"), "{args:?}");
        assert!(args.iter().any(|a| a == "--window-size=1920,1080"), "{args:?}");
        assert!(args.iter().any(|a| a == "--incognito"), "{args:?}");
        assert!(args.iter().any(|a| a == "--disable-breakpad"), "{args:?}");
        assert!(args.last().unwrap() == "--app=http://localhost:4000/#/stage", "{args:?}");
        assert!(args.iter().any(|a| a == "--start-fullscreen"), "dedicated output takes over: {args:?}");
        let plain = chromium_args("https://example.com", None, false, profile);
        assert!(!plain.iter().any(|a| a.to_string_lossy().starts_with("--window-position")), "{plain:?}");
        assert!(!plain.iter().any(|a| a == "--start-fullscreen"), "control screen stays windowed: {plain:?}");
    }

    #[test]
    fn persistent_profile_keeps_page_settings_incognito_discards_them() {
        // The seed only survives if the window is NOT incognito: selenium
        // writes Selah's transparentBackground into the profile's
        // localStorage, and an incognito window starts empty.
        let profile = std::path::Path::new("/tmp/p");
        let throwaway = chromium_args("http://localhost:4000/#/output1", None, false, profile);
        assert!(throwaway.iter().any(|a| a == "--incognito"), "{throwaway:?}");
        let persistent = chromium_args_full(
            "http://localhost:4000/#/output1",
            None,
            false,
            profile,
            true,
            false,
        );
        assert!(
            !persistent.iter().any(|a| a == "--incognito"),
            "persistent profile must keep its stored settings: {persistent:?}"
        );
        assert!(
            persistent.iter().any(|a| a == "--default-background-color=00000000"),
            "alpha still on: {persistent:?}"
        );
    }

    #[test]
    fn live_url_survives_caption_wrapping() {
        // Slides wrap the URL body at max_chars (40): the live view must
        // rejoin every line, or long URLs open truncated (first fragment
        // only — YouTube home instead of the video).
        let long = "https://www.youtube.com/watch?v=abcdefghijk";
        assert!(long.len() > 40, "fixture must exceed the wrap width");
        let lines = vp_core::verse_splitter::wrap_caption(long, 40);
        assert!(lines.len() > 1, "fixture must actually wrap: {lines:?}");
        let slide = Slide::text("title", lines, "YouTube");
        assert_eq!(live_url_for_slide(&slide).as_deref(), Some(long));
        // Single-line slides behave exactly as before.
        let short = Slide::text("t", vec!["example.com/live".into()], "WebSite");
        assert_eq!(
            live_url_for_slide(&short).as_deref(),
            Some("https://example.com/live")
        );
        // Non-website slides and empty bodies stay tiles.
        let song = Slide::text("t", vec![long.into()], "Song");
        assert_eq!(live_url_for_slide(&song), None);
        let empty = Slide::text("t", Vec::new(), "YouTube");
        assert_eq!(live_url_for_slide(&empty), None);
    }

    #[test]
    fn overlay_field_follows_the_compositor_and_the_override() {
        // With a compositor: real alpha, so the presentation background and
        // lyrics show through the transparent page.
        assert_eq!(overlay_field(true, None), OverlayField::Alpha);
        // Without one: black field, because an ARGB window would show the X
        // default (white) and swallow white text.
        assert_eq!(overlay_field(false, None), OverlayField::Black);
        // Operator override wins either way.
        assert_eq!(overlay_field(false, Some("alpha")), OverlayField::Alpha);
        assert_eq!(overlay_field(true, Some("black")), OverlayField::Black);
        assert_eq!(overlay_field(true, Some("nonsense")), OverlayField::Alpha);
    }

    #[test]
    fn overlay_wrapper_gives_the_page_its_black_field() {
        // The stage page paints transparent; the wrapper document paints the
        // black field, so a headed window without a compositor shows white
        // verse on black instead of white-on-white.
        let html = overlay_wrapper_html("http://localhost:4000/#/output1");
        assert!(html.contains("background:#000"), "no black field: {html}");
        assert!(html.contains("iframe"), "no framing: {html}");
        assert!(
            html.contains("src=\"http://localhost:4000/#/output1\""),
            "target missing or mangled: {html}"
        );
        // Attribute-escaping: a quote in the URL must not break out.
        let escaped = overlay_wrapper_html("http://x/\" onload=\"alert(1)");
        assert!(!escaped.contains("onload=\"alert"), "attribute injection: {escaped}");
        assert!(escaped.contains("&quot;"), "quote not escaped: {escaped}");
    }

    #[test]
    fn live_profile_is_throwaway_unless_the_operator_opts_in() {
        // Privacy default: a fresh throwaway dir per show, deleted on hide.
        // Opt-in persistence exists because a stage page may keep its own
        // settings (Selah's transparentBackground) in the browser profile.
        let throwaway = std::path::PathBuf::from("/tmp/live-abc");
        let fallback = std::path::Path::new("/tmp/persist-default");
        assert_eq!(profile_dir(throwaway.clone(), None, fallback), throwaway);
        assert_eq!(profile_dir(throwaway.clone(), Some(""), fallback), throwaway);
        assert_eq!(
            profile_dir(throwaway.clone(), Some("persist"), fallback),
            fallback.to_path_buf()
        );
        // A bare name stays under the app-data fallback, never the CWD.
        assert_eq!(
            profile_dir(throwaway.clone(), Some("stage-profile"), fallback),
            fallback.join("stage-profile")
        );
        assert_eq!(
            profile_dir(throwaway, Some("/home/operator/stage-profile"), fallback),
            std::path::PathBuf::from("/home/operator/stage-profile")
        );
    }

    #[test]
    fn overlay_is_transparent_over_the_presentation_by_default() {
        // Spec: the page is an overlay ON the presentation window — alpha by
        // default so the projector's black/background stays visible behind
        // it. Opaque black is only the explicit no-compositor fallback.
        let profile = std::path::Path::new("/tmp/p");
        assert_eq!(overlay_background(), "FF000000");
        // Alpha is the default: the page overlays the presentation window.
        assert!(overlay_alpha(true, None), "transparent by default");
        assert!(!overlay_alpha(true, Some("1")), "VP_LIVE_OPAQUE=1 opts out");
        assert!(!overlay_alpha(false, None), "explicit opaque wins");
        let args = chromium_args("http://localhost:4000/#/output1", None, false, profile);
        assert!(
            args.iter().any(|a| a == "--default-background-color=00000000"),
            "overlay must be transparent by default: {args:?}"
        );
        assert!(args.iter().any(|a| a == "--enable-transparent-visuals"), "{args:?}");
        // Explicit fallback for compositor-less desktops, via the switch.
        let opaque = chromium_args_with(
            "http://localhost:4000/#/output1",
            None,
            false,
            profile,
            false,
        );
        assert!(
            opaque.iter().any(|a| a == "--default-background-color=FF000000"),
            "{opaque:?}"
        );
    }

    #[test]
    fn live_overlay_window_is_transparent() {
        // Opt-in alpha path: with VP_LIVE_TRANSPARENT=1 the window is
        // alpha-capable and X11 is told to honour it, so the overlay
        // composites over the projector instead of covering it.
        let profile = std::path::Path::new("/tmp/p");
        for (geom, full) in [
            (None, false),
            (Some((1920, 0, 1920, 1080)), true),
        ] {
            let args = chromium_args("http://localhost:4000/#/output1", geom, full, profile);
            assert!(
                args.iter().any(|a| a == "--default-background-color=00000000"),
                "no alpha background: {args:?}"
            );
            assert!(
                args.iter().any(|a| a == "--enable-transparent-visuals"),
                "no transparent visuals: {args:?}"
            );
        }

        // Explicit opt-out stays available (tests/diagnostics).
        let opaque = chromium_args_with(
            "http://localhost:4000/#/output1",
            None,
            false,
            profile,
            false,
        );
        assert!(!opaque.iter().any(|a| a == "--enable-transparent-visuals"), "{opaque:?}");
    }

    #[test]
    fn reachable_detects_a_local_listener() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{port}/stage");
        assert!(reachable(&url, Duration::from_secs(2)), "{url}");
        drop(listener);
        assert!(!reachable(&url, Duration::from_millis(300)), "{url}");
    }

    /// Hermetic lifecycle test: drive `LiveView` with a sentinel process
    /// that writes its pid to a file and sleeps (`sh -c 'echo $$ > f;
    /// exec sleep 300'`). Asserts the real pid is gone after `hide()` —
    /// killing and reaping, not just clearing state — and the profile dir
    /// is removed. Linux-only: process + `/proc` semantics.
    #[cfg(target_os = "linux")]
    #[test]
    fn hide_kills_reaps_and_cleans_the_profile() {
        let dir = std::env::temp_dir().join(format!(
            "vp-live-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let pidfile = dir.join("pid");
        // Spawn `sh` directly (not through `show`, which builds browser
        // args): the point is what `hide()` does to a live child.
        let mut view = LiveView::new();
        view.child = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("echo $$ > {}; exec sleep 300", pidfile.display()))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok();
        assert!(view.child.is_some(), "sentinel spawned");
        let mut pid = String::new();
        for _ in 0..50 {
            if let Ok(s) = std::fs::read_to_string(&pidfile) {
                pid = s.trim().to_string();
                if !pid.is_empty() {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!pid.is_empty(), "sentinel wrote its pid");
        assert!(
            std::path::Path::new(&format!("/proc/{pid}")).exists(),
            "sentinel alive before hide"
        );
        view.profile = Some(dir.clone());
        view.hide();
        assert!(
            !std::path::Path::new(&format!("/proc/{pid}")).exists(),
            "sentinel reaped by hide"
        );
        assert!(!dir.exists(), "profile removed on hide");
    }

    #[test]
    fn live_url_accepts_website_and_youtube_slides() {
        let card = Slide::text(
            "Stage",
            vec!["http://localhost:4000/#/stage".to_string()],
            "WebSite",
        );
        assert_eq!(
            live_url_for_slide(&card),
            Some("http://localhost:4000/#/stage".to_string())
        );
        let yt = Slide::text(
            "Worship video",
            vec!["https://www.youtube.com/watch?v=x".to_string()],
            "YouTube",
        );
        assert!(live_url_for_slide(&yt).is_some());
        let lyric = Slide::text("T — V1", vec!["Amazing grace".to_string()], "John Newton");
        assert_eq!(live_url_for_slide(&lyric), None);
        let bad = Slide::text("X", vec!["not a url at all -->".to_string()], "WebSite");
        assert_eq!(live_url_for_slide(&bad), None);
    }
}
