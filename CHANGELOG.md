# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.1] - 2024-07-XX

### Changed

- Minor changes to documentation.

## [1.0.0] - 2024-07-08

### Added

- This Changelog.

### Changed

- Changed `tlm::probed` and `tlm::joined` to allow reusing the `control` and thread-locals after calling `take_tls` or `take_own_tl`. Despite this semantic change, code using v0.5.x should work fine with the current version.
- Refactored several unit tests and added unit test cases in all modules for the reuse of `control` after the thread-local values are collected and aggregated.
- Renamed P types in `tlm::` `joined`, `simple_joined`, and `probed` to make generated docs more clear.
- Streamlined README.md, added text on Rust version requirements and license.
- Improved doc comments throughout.
- Renamed internal `test_support` directory to `dev_support`, impacting only tests and examples.
- Moved the `thiserror` dependency to optional feature `tlcr`.

## [0.5.2] - 2024-06-20

### Changed

- Updated Cargo.toml to include `tlcr` feature in published docs.

## [0.5.1] - 2024-06-20

### Changed

- Useless change to `Cargo.toml`.

## [0.5.0] - 2024-06-20

Initial release. Starts at v0.5.0 to reflect 8 months of development and close to 200 commits.
