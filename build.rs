use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, fs};

/// Run a git command, returning a trimmed value or `None` when git is absent
/// (a source tarball or a container context must still build).
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn env_value(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
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
    // stale `web/dist` bytes is indistinguishable from a good one at runtime.
    for key in [
        "PANGOLIN_BUILD_COMMIT",
        "GITHUB_SHA",
        "SOURCE_DATE_EPOCH",
        "TARGET",
        "PROFILE",
    ] {
        println!("cargo:rerun-if-env-changed={key}");
    }
    // Watch only paths that exist: Cargo treats a missing watch target as
    // changed on every invocation, which would rebuild on every command. In a
    // worktree `.git` is a file, so the directory is resolved through git.
    if let Some(git_dir) = git(&["rev-parse", "--absolute-git-dir"]) {
        for name in ["HEAD", "index", "packed-refs"] {
            let path = PathBuf::from(&git_dir).join(name);
            if path.is_file() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }

    // CI and container builds have no `.git`, so an explicit revision wins.
    let commit = env_value("PANGOLIN_BUILD_COMMIT")
        .or_else(|| env_value("GITHUB_SHA").map(|sha| sha.chars().take(12).collect()))
        .or_else(|| {
            git(&["rev-parse", "--short=12", "HEAD"]).map(|commit| {
                // A dirty tree means the binary does not correspond to the commit.
                let dirty = git(&["status", "--porcelain", "--untracked-files=no"])
                    .is_some_and(|status| !status.is_empty());
                if dirty {
                    format!("{commit}-dirty")
                } else {
                    commit
                }
            })
        })
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=PANGOLIN_BUILD_COMMIT={commit}");

    // `SOURCE_DATE_EPOCH` keeps rebuilds reproducible when a packager sets it.
    let epoch = env_value("SOURCE_DATE_EPOCH")
        .and_then(|value| value.parse::<u64>().ok())
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
        env_value("TARGET").unwrap_or_else(|| "unknown".into())
    );
    println!(
        "cargo:rustc-env=PANGOLIN_BUILD_PROFILE={}",
        env_value("PROFILE").unwrap_or_else(|| "unknown".into())
    );

    // The console is embedded as built bytes, and `web/dist` is gitignored, so
    // the working-tree revision says nothing about them. The web build records
    // its own revision in `web/dist/build.json`; embed that too, or a release
    // can ship a console from an older revision while claiming the current one.
    let (web_version, web_commit, web_built_at) = match fs::read_to_string("web/dist/build.json") {
        Ok(raw) => match serde_json::from_str::<serde_json::Value>(&raw) {
            Ok(value) => (
                value["version"].as_str().unwrap_or("unknown").to_string(),
                value["commit"].as_str().unwrap_or("unknown").to_string(),
                value["builtAt"].as_str().unwrap_or("unknown").to_string(),
            ),
            Err(error) => {
                println!("cargo:warning=web/dist/build.json is not valid JSON: {error}");
                ("unknown".into(), "unknown".into(), "unknown".into())
            }
        },
        Err(_) => ("unknown".into(), "unknown".into(), "unknown".into()),
    };
    println!("cargo:rustc-env=PANGOLIN_WEB_VERSION={web_version}");
    println!("cargo:rustc-env=PANGOLIN_WEB_COMMIT={web_commit}");
    println!("cargo:rustc-env=PANGOLIN_WEB_BUILT_AT={web_built_at}");
}
