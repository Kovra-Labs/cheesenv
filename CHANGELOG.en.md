# Changelog

English edition of [`CHANGELOG.md`](CHANGELOG.md), which explains the format.
Keep both files in step: same versions, same entries.

## [Unreleased]

### Fixed

- The changelog now follows the app language and is easier to read.

## [0.5.0] - 2026-11-26

### Added

- Changelog: see what an update brings before installing it, a "What's new"
  summary after updating, and the full history in Settings.

## [0.4.0] - 2026-11-26

### Added

- Projects: every folder in `~/Stackyard/www` opens right away at
  `folder-name.test`.
- Built-in terminal with tabs, ready to use Stackyard's runtimes and tools.
- Local HTTPS for `.test` domains without browser warnings.
- Linux: projects open without `:8080` (ports 80 and 443).
- Choose the PHP version per project (8.4 or 8.3).
- PHP 8.4 and 8.3 for Windows in the Module Hub.

### Changed

- nginx and Apache that are already installed now support `.test` domains, no
  reinstall needed.

## [0.3.0] - 2026-11-25

### Added

- Node.js, Python, MariaDB, MongoDB, Redis and Apache modules in the Module
  Hub.
- phpMyAdmin and pgAdmin are installed automatically with their database.
- The default Node.js and Python versions work straight from the terminal.
- Stackyard runs only once; opening it again shows the existing window.

### Changed

- New look, colors and app icon.

### Fixed

- macOS: services failed to start on some computers.
- pgAdmin did not show its web page.
- Missing stop button and module icons.
