//! Versioned declarative catalog. SQLite snapshots are authoritative; bundled
//! data is a network-independent fallback. No catalog data is executable.
pub mod refresh;
pub mod repository;
pub mod types;

use std::sync::OnceLock;
pub use types::{Catalog, Model, Provider};

pub fn builtin() -> &'static Catalog {
    static BUILTIN: OnceLock<Catalog> = OnceLock::new();
    BUILTIN.get_or_init(|| {
        Catalog::parse(include_bytes!("data/builtin.json")).expect("bundled catalog must be valid")
    })
}

#[cfg(test)]
mod tests;
