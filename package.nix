{
  lib,
  rustPlatform,
  protobuf,
} : let
  cargoContent = builtins.readFile ./Cargo.toml;
  cargoData = builtins.fromTOML cargoContent;
  in rustPlatform.buildRustPackage {
    pname = "horust";
    version = cargoData.workspace.package.version;
    nativeBuildInputs = [protobuf];
    src = ./.;
    cargoHash = "sha256-k5h1Z5AsDF+mGqqhGEzRGSxVNGOILKlMjQOoJSlbPUs=";
    # horust tests need full environment
    # cannot run in nix build sandboxes
    doCheck = false;
    meta = with lib; {
      description = "Horust - a supervisor / init system written in Rust, designed for containers";
      homepage = "https://github.com/FedericoPonzi/Horust";
      license = licenses.mit;
    };
}
