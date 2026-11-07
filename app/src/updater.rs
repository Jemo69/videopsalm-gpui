//! Opt-in update check for Perez RS (no auto-download, no background polling).
//!
//! Checker is opt-in only (triggered via F9 / Help menu; strictly no background
//! use, zero telemetry payloads).
//!
//! Offline-first contract: nothing here runs unless the operator explicitly
//! asks (Help → Check for updates or F9). Then — and only then — one HTTPS GET
//! goes to the GitHub releases API for `videopsalm-gpui`; the answer is a
//! version comparison and a download-page pointer. No installer is fetched,
//! no telemetry is sent, and a machine without network just reports
//! "unavailable" instead of failing the service.

/// Outcome of a manual update check, safe to show verbatim in the UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateState {
    /// This build is current (or newer than the latest tag).
    Current { latest: String },
    /// A newer release exists; the operator decides what to do.
    Available { latest: String, url: String },
    /// No network, API error, or unparseable reply — never a crash.
    Unavailable { reason: String },
}

/// Compare two `vMAJOR.MINOR.PATCH`-ish tags. Returns Ordering of `a` vs `b`;
/// non-numeric tails are ignored (`v0.1.19-gpui` == `v0.1.19`).
pub fn compare_tags(a: &str, b: &str) -> std::cmp::Ordering {
    fn parts(s: &str) -> Vec<u64> {
        s.trim_start_matches(['v', 'V'])
            .split(|c: char| !(c.is_ascii_digit()))
            .filter(|p| !p.is_empty())
            .filter_map(|p| p.parse().ok())
            .collect()
    }
    let (mut x, mut y) = (parts(a), parts(b));
    while x.len() < 3 {
        x.push(0);
    }
    while y.len() < 3 {
        y.push(0);
    }
    x.cmp(&y)
}

/// Decide the update state from the running version and a releases-API body.
/// Pure: the network fetch lives in the app layer; this is pinned headlessly.
///
/// Expects the `/releases/latest` JSON shape (`tag_name`, `html_url`).
pub fn decide_update(running: &str, api_body: &str) -> UpdateState {
    let v: serde_json::Value = match serde_json::from_str(api_body) {
        Ok(v) => v,
        Err(e) => {
            return UpdateState::Unavailable {
                reason: format!("release feed did not parse: {e}"),
            }
        }
    };
    let latest = v.get("tag_name").and_then(|t| t.as_str()).unwrap_or("");
    if latest.is_empty() {
        return UpdateState::Unavailable {
            reason: "release feed has no tag_name".into(),
        };
    }
    let url = v
        .get("html_url")
        .and_then(|u| u.as_str())
        .unwrap_or("https://github.com/Jemo69/videopsalm-gpui/releases")
        .to_string();
    match compare_tags(running, latest) {
        std::cmp::Ordering::Less => UpdateState::Available {
            latest: latest.to_string(),
            url,
        },
        _ => UpdateState::Current {
            latest: latest.to_string(),
        },
    }
}

/// Latest-release endpoint for the public Rust repo.
pub const RELEASES_LATEST_URL: &str =
    "https://api.github.com/repos/Jemo69/videopsalm-gpui/releases/latest";

/// One-shot HTTPS fetch of the latest-release JSON. Called ONLY from the
/// operator's explicit "Check for updates" action, on a worker thread —
/// never on the UI thread, never at startup, never on a timer.
/// `running` is this build's `CARGO_PKG_VERSION`.
pub fn check_now(running: &str) -> UpdateState {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("perez-rs")
        .build();
    let body = match agent
        .get(RELEASES_LATEST_URL)
        .set("Accept", "application/vnd.github+json")
        .call()
    {
        Ok(resp) => match resp.into_string() {
            Ok(b) => b,
            Err(e) => {
                return UpdateState::Unavailable {
                    reason: format!("release feed unreadable: {e}"),
                }
            }
        },
        Err(e) => {
            return UpdateState::Unavailable {
                reason: format!("update check failed (offline?): {e}"),
            }
        }
    };
    decide_update(running, &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_compare_numerically_not_lexically() {
        assert_eq!(compare_tags("v0.1.9", "v0.1.19"), std::cmp::Ordering::Less);
        assert_eq!(compare_tags("v0.1.19", "v0.1.19"), std::cmp::Ordering::Equal);
        assert_eq!(compare_tags("v0.2.0", "v0.1.27"), std::cmp::Ordering::Greater);
        assert_eq!(
            compare_tags("v0.1.19-gpui", "v0.1.19"),
            std::cmp::Ordering::Equal
        );
    }

    #[test]
    fn newer_release_is_available_otherwise_current() {
        let body = r#"{"tag_name":"v0.2.0","html_url":"https://example/r/2"}"#;
        assert_eq!(
            decide_update("v0.1.19", body),
            UpdateState::Available {
                latest: "v0.2.0".into(),
                url: "https://example/r/2".into()
            }
        );
        assert_eq!(
            decide_update("v0.2.0", body),
            UpdateState::Current {
                latest: "v0.2.0".into()
            }
        );
    }

    #[test]
    fn garbage_feed_is_unavailable_never_a_crash() {
        assert!(matches!(
            decide_update("v0.1.19", "not json"),
            UpdateState::Unavailable { .. }
        ));
        assert!(matches!(
            decide_update("v0.1.19", "{}"),
            UpdateState::Unavailable { .. }
        ));
    }
}
