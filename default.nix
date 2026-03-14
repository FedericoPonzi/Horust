{
  lib,
  rustPlatform,
  protobuf,
} : rustPlatform.buildRustPackage {
    name = "horust";
    version = "master";
    nativeBuildInputs = [protobuf];
    src = ./.;
    cargoHash = "sha256-k5h1Z5AsDF+mGqqhGEzRGSxVNGOILKlMjQOoJSlbPUs=";
}
