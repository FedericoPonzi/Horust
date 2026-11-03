use anyhow::{Context, Result, ensure};
use std::path::PathBuf;

fn main() -> Result<()> {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut args = std::env::args_os().skip(1);
    let output = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| base.join("src/proto"));
    ensure!(
        args.next().is_none(),
        "Usage: horust-generate-proto [output-directory]"
    );
    prost_build::Config::new()
        .out_dir(output)
        .compile_protos(&[base.join("src/commands.proto")], &[base.join("src")])
        .context("Could not regenerate commands.proto; ensure protoc is installed")
}
