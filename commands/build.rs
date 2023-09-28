pub fn main() {
    prost_build::Config::new()
        .out_dir("src/proto")
        .compile_protos(&["src/commands.proto"], &["src"])
        .expect("Could not compile protobuf types in commands.proto");
}
