# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- Map and set iterators (`MapEntries`, `MapKeys`, `MapValues`, and `SetEntries`) now implement `DoubleEndedIterator`.

### Changed

- `Iterator::count`, `Iterator::last`, and `Iterator::nth` for map and set iterators are now `O(1)` time instead of `O(n)` time.
- Queries and construction now use a [PTHash](https://github.com/jermp/pthash)/[PtrHash](https://github.com/RagnarGrootKoerkamp/PTRHash)-style per-bucket pilot table instead of [CHD](https://cmph.sourceforge.net/chd.html) displacement pairs.
- Queries and construction now use a "packed" strategy instead of a "direct" strategy. The packed strategy searches for a bit window that separates keys' hashes into distinct slots. The direct strategy searches for an RNG seed where keys hash to distinct slots, which takes more computational effort.

### Fixed

- Reseeding and bucket assignment during construction are now more robust against structured key sets (e.g. consecutive integers).

## [0.1.1] - 2026-06-22

### Fixed

- Improved README appearance on crates.io.

## [0.1.0] - 2026-06-22

Initial release!
