## Compile

Normal builds use the checked-in protobuf bindings and do not require `protoc`.

## Regenerate protobuf bindings

After changing `src/commands.proto`, install `protoc` (on Debian:
`apt-get install protobuf-compiler`) and run from the workspace root:

```bash
cargo run -p horust-commands-lib --features codegen --bin horust-generate-proto
```

An optional output-directory argument generates bindings elsewhere without
overwriting the checked-in files.
