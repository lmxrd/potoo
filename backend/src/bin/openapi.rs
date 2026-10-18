//! Writes the OpenAPI spec to a file (default `target/specs/openapi.json`,
//! or the path given as the first argument), for the frontend's client
//! generator. Run with `mise run gen:spec`; it builds the library alone —
//! no frontend bundle, no database.

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/specs/openapi.json".into());
    if let Some(parent) = std::path::Path::new(&path).parent() {
        std::fs::create_dir_all(parent).unwrap_or_else(|e| panic!("cannot create {parent:?}: {e}"));
    }
    std::fs::write(&path, potoo::openapi::spec_json())
        .unwrap_or_else(|e| panic!("cannot write {path}: {e}"));
    println!("wrote {path}");
}
