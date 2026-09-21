use std::{env, path::Path, process::Command, time::SystemTime, time::UNIX_EPOCH};

/// Run a git command, returning a trimmed value or `None` when git is absent
/// (a source tarball build must still succeed).
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn main() {
    for path in [
        "web/index.html",
        "web/package.json",
        "web/src",
        "web/public",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }

    if env::var("PROFILE").as_deref() == Ok("release") {
        println!("cargo:rerun-if-changed=web/dist");
        if !Path::new("web/dist/index.html").is_file() {
            panic!("web/dist is missing; run `pnpm --dir web build` before a release build");
        }
    }

    // Build provenance. Without it a binary built from a stale checkout or from
    // a half-written `web/dist` is indistinguishable from a good one at runtime.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
    let commit = match git(&["rev-parse", "--short=12", "HEAD"]) {
        Some(commit) => {
            // A dirty tree means the binary does not correspond to the commit.
            let dirty = git(&["status", "--porcelain", "--untracked-files=no"])
                .is_some_and(|status| !status.is_empty());
            if dirty {
                format!("{commit}-dirty")
            } else {
                commit
            }
        }
        None => "unknown".to_string(),
    };
    println!("cargo:rustc-env=PANGOLIN_BUILD_COMMIT={commit}");

    // `SOURCE_DATE_EPOCH` keeps rebuilds reproducible when a packager sets it.
    let epoch = env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()
                .map(|since| since.as_secs())
        })
        .unwrap_or_default();
    println!("cargo:rustc-env=PANGOLIN_BUILD_EPOCH={epoch}");
    println!(
        "cargo:rustc-env=PANGOLIN_BUILD_TARGET={}",
        env::var("TARGET").unwrap_or_else(|_| "unknown".into())
    );
    println!(
        "cargo:rustc-env=PANGOLIN_BUILD_PROFILE={}",
        env::var("PROFILE").unwrap_or_else(|_| "unknown".into())
    );
}
