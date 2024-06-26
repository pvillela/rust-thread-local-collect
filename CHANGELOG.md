# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

- Not applicable.

## [0.5.3] - 2024-06-XX

### Added

- This Changelog.

### Changed

- Changed `tlm::probed` to allow reusing the `control` and thread-locals after calling `take_tls`.
- Streamlined README.md, added text on Rust version requirements and license.
- Minor clarifications in doc comments for the three `probed` modules.
- Renamed internal `test_support` directory to `dev_support`, impacting only tests and examples.

## [0.5.2] - 2024-06-20

### Changed

- Updated Cargo.toml to include `tlcr` feature in published docs.

## [0.5.1] - 2024-06-20

### Changed

- Useless change to `Cargo.toml`.

## [0.5.0] - 2024-06-20

Initial release.
