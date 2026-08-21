# Changelog

All notable user-facing changes to Horust are documented in this file.
Internal changes (CI, tests, refactors, dependency bumps) are omitted.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Releases before 0.1.14 are documented on the
[GitHub releases page](https://github.com/FedericoPonzi/Horust/releases).

## [Unreleased]

## [0.1.14] - 2026-08-21

### Added

- **`shutdown-after` service option.** A service can now declare
  `shutdown-after = ["database"]` to control the order in which services are terminated
  during a graceful shutdown: it will only receive its termination signal after all the
  listed services have fully stopped. A second SIGTERM (forceful shutdown) bypasses the
  ordering. ([#308](https://github.com/FedericoPonzi/Horust/pull/308), closes
  [#180](https://github.com/FedericoPonzi/Horust/issues/180))
- **Nix build support.** Horust can now be built with `nix build github:FedericoPonzi/Horust`
  (flakes) or `nix-build` (legacy).
  ([#306](https://github.com/FedericoPonzi/Horust/pull/306))
- **`aarch64-unknown-linux-musl` release binaries.** Releases previously shipped a musl
  binary only for x86_64, so musl-based arm64 images (for example Alpine) could not run
  the published arm64 binary: the glibc build fails at exec because
  `/lib/ld-linux-aarch64.so.1` is missing, and installing `gcompat` is not enough because
  Horust also imports `__res_init@GLIBC_2.17`. arm64 musl images now get the same
  static-pie binary already available for x86_64.
  ([#321](https://github.com/FedericoPonzi/Horust/pull/321))

[Unreleased]: https://github.com/FedericoPonzi/Horust/compare/v0.1.14...HEAD
[0.1.14]: https://github.com/FedericoPonzi/Horust/compare/v0.1.13...v0.1.14
