//! Generate the OpenAPI specification with utoipa.
//!
//! Run with `mise run gen:spec` (or `cargo run --features spec --bin specs`).
//! Writes `openapi.json` and `openapi.yaml` to `target/specs/`, where the
//! SDK generator and the release read them. Built with `--features spec`,
//! so it needs no frontend build.

use anyhow::Context;

fn main() -> anyhow::Result<()> {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/specs");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;

    let openapi = potoo::openapi::spec();
    std::fs::write(dir.join("openapi.json"), openapi.to_pretty_json()?)?;
    std::fs::write(dir.join("openapi.yaml"), openapi.to_yaml()?)?;
    println!("wrote {}/openapi.json and openapi.yaml", dir.display());
    Ok(())
}
