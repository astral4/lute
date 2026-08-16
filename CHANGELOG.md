# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Changed

- Queries and construction now use a [PTHash](https://github.com/jermp/pthash)/[PtrHash](https://github.com/RagnarGrootKoerkamp/PTRHash)-style per-bucket pilot table instead of [CHD](https://cmph.sourceforge.net/chd.html) displacement pairs.

### Fixed

- Reseeding and bucket assignment during construction are now more robust against structured key sets (e.g. consecutive integers).

## [0.1.1] - 2026-06-22

### Fixed

- Improved README appearance on crates.io.

## [0.1.0] - 2026-06-22

Initial release!
