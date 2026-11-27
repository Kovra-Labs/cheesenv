# Changelog

English edition of [`CHANGELOG.md`](CHANGELOG.md), which explains the format.
Keep both files in step: same versions, same entries.

## [Unreleased]

## [0.2.0] - 2026-11-27

### Added

- Accent colour choice (Cheese, Blue, Teal, Violet) in Settings → General.
- "Run when the computer starts" option: CheesEnv opens in the tray at login.
- Composer module in the Module Hub, with its own PHP CLI; the `composer`
  command is available in the terminal.

## [0.1.0] - 2026-11-27

### Added

- First CheesEnv release: manage local web servers, runtimes and databases on
  Windows, macOS and Linux.
- Module Hub: nginx, Apache, PHP, Node.js, Python, MySQL, MariaDB,
  PostgreSQL, MongoDB and Redis, plus phpMyAdmin and pgAdmin installed
  automatically with their database.
- Projects: every folder in `~/CheesEnv/www` opens right away at
  `folder-name.test`, with a PHP version per project.
- Local HTTPS for `.test` domains without browser warnings.
- Linux: projects open without `:8080` (ports 80 and 443).
- Built-in terminal with tabs; the default Node.js and Python versions also
  work from any terminal.
- System tray icon; closing the window keeps services running.
- Automatic updates with a changelog: see what an update brings before
  installing it, a "What's new" summary after updating, and the full history
  in Settings.
