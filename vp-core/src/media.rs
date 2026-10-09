//! Media playback behind a stub player trait.
//!
//! SPEC 2.4: background video loops under lyrics, music auto-play per song,
//! player controls (play/pause F5, I/O range), video audio-channel select
//! (v1: default channel). The original used VLC (libvlc); this
//! reimplementation is clean-room and ships a [`MediaPlayer`] trait with two
//! v1 backends:
//!
//! * [`StubPlayer`] — in-memory fake used by tests and headless runs.
//! * [`ExternalPlayer`] — default backend: opens the file with the OS
//!   handler (`xdg-open` / `open` / `start`) and tracks play/pause/range
//!   state locally.
//!
//! HOOK-UP POINT (GStreamer): to use real in-window playback, add a
//! `media_gst.rs` module implementing `MediaPlayer for GstPlayer` on top of
//! the `gstreamer` crate (pipeline `playbin`, `seek` for I/O range,
//! `audio-sink` channel selection) and select it in the app's projector
//! controls. GStreamer is deliberately NOT required to build v1.

use std::path::PathBuf;

/// Which media slot this player handles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MediaKind {
    #[default]
    Video,
    Audio,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlaybackState {
    #[default]
    Stopped,
    Playing,
    Paused,
}

/// Player controls every backend must provide.
pub trait MediaPlayer {
    fn kind(&self) -> MediaKind;
    fn state(&self) -> PlaybackState;
    /// Open/replace the current file (stops playback).
    fn open(&mut self, path: PathBuf);
    fn current(&self) -> Option<&PathBuf>;
    /// Toggle play/pause (F5 in the original).
    fn toggle(&mut self);
    fn stop(&mut self);
    /// Set the I/O play range in milliseconds (I/O keys in the original).
    fn set_range(&mut self, start_ms: Option<u64>, end_ms: Option<u64>);
    fn range(&self) -> (Option<u64>, Option<u64>);
}

/// In-memory fake: records calls, advances no clock. Used by tests.
#[derive(Debug, Default)]
pub struct StubPlayer {
    kind: MediaKind,
    state: PlaybackState,
    current: Option<PathBuf>,
    range: (Option<u64>, Option<u64>),
    pub toggle_count: u32,
}

impl StubPlayer {
    pub fn new(kind: MediaKind) -> Self {
        StubPlayer {
            kind,
            ..Default::default()
        }
    }
}

impl MediaPlayer for StubPlayer {
    fn kind(&self) -> MediaKind {
        self.kind
    }
    fn state(&self) -> PlaybackState {
        self.state
    }
    fn open(&mut self, path: PathBuf) {
        self.current = Some(path);
        self.state = PlaybackState::Stopped;
    }
    fn current(&self) -> Option<&PathBuf> {
        self.current.as_ref()
    }
    fn toggle(&mut self) {
        self.toggle_count += 1;
        self.state = match self.state {
            PlaybackState::Playing => PlaybackState::Paused,
            _ => PlaybackState::Playing,
        };
    }
    fn stop(&mut self) {
        self.state = PlaybackState::Stopped;
    }
    fn set_range(&mut self, start_ms: Option<u64>, end_ms: Option<u64>) {
        self.range = (start_ms, end_ms);
    }
    fn range(&self) -> (Option<u64>, Option<u64>) {
        self.range
    }
}

/// Default v1 backend: delegates actual decoding to the OS handler and
/// tracks transport state locally.
#[derive(Debug, Default)]
pub struct ExternalPlayer {
    kind: MediaKind,
    state: PlaybackState,
    current: Option<PathBuf>,
    range: (Option<u64>, Option<u64>),
    pub last_error: Option<String>,
}

impl ExternalPlayer {
    pub fn new(kind: MediaKind) -> Self {
        ExternalPlayer {
            kind,
            ..Default::default()
        }
    }

    /// Launch the OS default handler for the current file (best effort).
    /// Never performs network access.
    pub fn launch_external(&mut self) {
        let Some(path) = self.current.clone() else {
            self.last_error = Some("no media file selected".to_string());
            return;
        };
        #[cfg(target_os = "linux")]
        let mut cmd = std::process::Command::new("xdg-open");
        #[cfg(target_os = "macos")]
        let mut cmd = std::process::Command::new("open");
        #[cfg(target_os = "windows")]
        let mut cmd = {
            let mut c = std::process::Command::new("cmd");
            c.args(["/C", "start", ""]);
            c
        };
        let res = cmd.arg(&path).spawn();
        match res {
            Ok(_) => {
                self.state = PlaybackState::Playing;
                self.last_error = None;
            }
            Err(e) => self.last_error = Some(format!("cannot open {path:?}: {e}")),
        }
    }
}

impl MediaPlayer for ExternalPlayer {
    fn kind(&self) -> MediaKind {
        self.kind
    }
    fn state(&self) -> PlaybackState {
        self.state
    }
    fn open(&mut self, path: PathBuf) {
        self.current = Some(path);
        self.state = PlaybackState::Stopped;
    }
    fn current(&self) -> Option<&PathBuf> {
        self.current.as_ref()
    }
    fn toggle(&mut self) {
        match self.state {
            PlaybackState::Playing => self.state = PlaybackState::Paused,
            _ => self.launch_external(),
        }
    }
    fn stop(&mut self) {
        self.state = PlaybackState::Stopped;
    }
    fn set_range(&mut self, start_ms: Option<u64>, end_ms: Option<u64>) {
        self.range = (start_ms, end_ms);
    }
    fn range(&self) -> (Option<u64>, Option<u64>) {
        self.range
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_transport_and_range() {
        let mut p = StubPlayer::new(MediaKind::Video);
        assert_eq!(p.state(), PlaybackState::Stopped);
        p.open(PathBuf::from("clip.mp4"));
        p.toggle(); // F5
        assert_eq!(p.state(), PlaybackState::Playing);
        p.toggle();
        assert_eq!(p.state(), PlaybackState::Paused);
        p.set_range(Some(1000), Some(5000));
        assert_eq!(p.range(), (Some(1000), Some(5000)));
        p.stop();
        assert_eq!(p.state(), PlaybackState::Stopped);
        assert_eq!(p.toggle_count, 2);
    }

    #[test]
    fn external_player_without_file_reports_error() {
        let mut p = ExternalPlayer::new(MediaKind::Audio);
        p.toggle(); // no file -> error recorded, no panic, no spawn
        assert!(p.last_error.is_some());
        p.open(PathBuf::from("song.mp3"));
        p.set_range(None, Some(9000));
        assert_eq!(p.range(), (None, Some(9000)));
    }
}
