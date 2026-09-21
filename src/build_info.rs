//! Build provenance.
//!
//! A release binary embeds `web/dist` at compile time, so a binary built before
//! the assets were rebuilt serves a console that does not match its sources. That
//! is invisible from the outside unless the binary says what it was built from,
//! which is what this module is for.

use chrono::{TimeZone, Utc};

pub struct BuildInfo {
    /// Cargo package version, the single source of truth for the release number.
    pub version: &'static str,
    /// Short commit, suffixed `-dirty` when the tree had uncommitted changes.
    pub commit: &'static str,
    /// RFC 3339 UTC build time.
    pub built_at: String,
    pub target: &'static str,
    pub profile: &'static str,
}

pub fn build_info() -> BuildInfo {
    let epoch = env!("PANGOLIN_BUILD_EPOCH")
        .parse::<i64>()
        .unwrap_or_default();
    let built_at = Utc
        .timestamp_opt(epoch, 0)
        .single()
        .map(|moment| moment.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_else(|| "unknown".into());
    BuildInfo {
        version: env!("CARGO_PKG_VERSION"),
        commit: env!("PANGOLIN_BUILD_COMMIT"),
        built_at,
        target: env!("PANGOLIN_BUILD_TARGET"),
        profile: env!("PANGOLIN_BUILD_PROFILE"),
    }
}

pub fn build_info_json() -> serde_json::Value {
    let info = build_info();
    serde_json::json!({
        "version": info.version,
        "commit": info.commit,
        "built_at": info.built_at,
        "target": info.target,
        "profile": info.profile,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_the_package_version_and_a_parsable_build_time() {
        let info = build_info();
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
        assert!(!info.commit.is_empty());
        assert!(!info.target.is_empty());
        // A build with no usable timestamp would report "unknown" and leave the
        // stale-binary question unanswerable, so require a real instant.
        assert!(
            chrono::DateTime::parse_from_rfc3339(&info.built_at).is_ok(),
            "built_at was {}",
            info.built_at
        );
    }

    #[test]
    fn exposes_every_field_the_console_about_page_shows() {
        let json = build_info_json();
        for key in ["version", "commit", "built_at", "target", "profile"] {
            assert!(json.get(key).is_some_and(|value| !value.is_null()), "{key}");
        }
    }
}
