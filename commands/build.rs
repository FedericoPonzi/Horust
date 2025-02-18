pub fn main() {
    if std::env::var("CARGO_PACKAGE_NAME").is_ok() {
        // Skip regeneration when publishing
        return;
    }

    prost_build::Config::new()
        .out_dir("src/proto")
        .compile_protos(&["src/commands.proto"], &["src"])
        .expect("Could not compile protobuf types in commands.proto");
}
