{
  lib,
  rustPlatform,
} : rustPlatform.buildRustPackage {
    name = "horust";
    version = lib.commitIdFromGitRepo ./.git;
    src = ./.;
    cargoHash = lib.fakeHash;
}
