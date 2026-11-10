//! Print the `config.toml` JSON Schema to stdout.
//!
//! This emits the exact bytes committed to `docs/config.schema.json` (and the
//! same output as `prime-files config schema`), but builds only `prime-files-core`, so it
//! runs on Linux where the gpui-backed `prime-files` binary does not link. CI uses it
//! to keep the committed schema in sync:
//!
//! ```bash
//! cargo run -p prime-files-core --example schema > docs/config.schema.json
//! ```

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", prime_files_core::config::json_schema_string()?);
    Ok(())
}
