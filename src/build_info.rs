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
    /// Revision of the console bytes this binary embeds. `web/dist` is
    /// gitignored, so the backend revision says nothing about them.
    pub console: ConsoleBuild,
}

pub struct ConsoleBuild {
    pub version: &'static str,
    pub commit: &'static str,
    pub built_at: &'static str,
}

impl BuildInfo {
    /// Whether the embedded console came from the same revision as the backend.
    /// `None` when either side could not be determined — an unmeasurable pair is
    /// not a match.
    pub fn console_matches(&self) -> Option<bool> {
        let strip = |value: &str| value.trim().trim_end_matches("-dirty").to_string();
        let backend = strip(self.commit);
        let console = strip(self.console.commit);
        if backend.is_empty() || console.is_empty() || backend == "unknown" || console == "unknown"
        {
            return None;
        }
        Some(backend == console)
    }
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
        console: ConsoleBuild {
            version: env!("PANGOLIN_WEB_VERSION"),
            commit: env!("PANGOLIN_WEB_COMMIT"),
            built_at: env!("PANGOLIN_WEB_BUILT_AT"),
        },
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
        "console": {
            "version": info.console.version,
            "commit": info.console.commit,
            "built_at": info.console.built_at,
        },
        "console_matches": info.console_matches(),
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
        for key in [
            "version", "commit", "built_at", "target", "profile", "console",
        ] {
            assert!(json.get(key).is_some_and(|value| !value.is_null()), "{key}");
        }
        for key in ["version", "commit", "built_at"] {
            assert!(
                json["console"]
                    .get(key)
                    .is_some_and(|value| !value.is_null()),
                "console.{key}"
            );
        }
    }

    #[test]
    fn reports_a_console_mismatch_but_not_an_unknowable_one() {
        let mut info = build_info();
        info.commit = "abc123";
        info.console.commit = "abc123";
        assert_eq!(info.console_matches(), Some(true));
        // A `-dirty` suffix marks uncommitted changes, not a different revision.
        info.console.commit = "abc123-dirty";
        assert_eq!(info.console_matches(), Some(true));
        info.console.commit = "def456";
        assert_eq!(info.console_matches(), Some(false));
        // An unmeasurable pair must not be reported as a match.
        info.console.commit = "unknown";
        assert_eq!(info.console_matches(), None);
    }
}
