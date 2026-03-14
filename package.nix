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
    # horust tests need full environment
    # cannot run in nix build sandboxes
    doCheck = false;
}
