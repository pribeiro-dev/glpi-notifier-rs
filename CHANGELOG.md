# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- CI workflow (`.github/workflows/ci.yml`) with rustfmt, clippy (deny warnings), build and tests.
- Release workflow (`.github/workflows/release.yml`) for tagged builds publishing a Windows ZIP.
- Heartbeat file written to `%LOCALAPPDATA%\GlpiNotifier\heartbeat.json`.
- Requester displayed on toast and optional **Open** button.
- Scripts: `scripts/install.ps1`, `scripts/uninstall.ps1`, `scripts/health.ps1`.

### Changed
- English comments throughout the codebase.
- Scheduled Task user-mode install recommended instead of Windows Service.

### Fixed
- Follow simple 30x redirect during `initSession` and update base URL.

## [0.1.0] - 2025-11-07
### Added
- Initial version with GLPI polling and Windows toast notifications via SnoreToast.
- Persistent seen-ticket state and basic logging.
