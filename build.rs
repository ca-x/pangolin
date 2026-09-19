use std::{env, path::Path};

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
}
