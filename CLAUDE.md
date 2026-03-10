# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build Commands

```bash
cargo build              # Debug build
cargo build --release    # Release build (used by keyboard shortcut)
cargo test               # Run all tests (64 tests across 4 modules)
cargo test cache         # Run tests in cache module only
cargo test icloud        # Run tests in icloud module only
```

## Architecture

Calendarchy is a terminal calendar app that displays Google Calendar and iCloud Calendar events side by side.

### Core Flow

1. **Startup**: `main.rs` loads config, restores cached events from disk for instant display, then authenticates
2. **Auth**: Google uses OAuth device flow; iCloud uses app-specific password with CalDAV discovery
3. **Fetching**: Events are fetched per-month via async tasks, converted to `DisplayEvent`, cached to disk
4. **Rendering**: `ui.rs` renders a month calendar grid and two event panels using crossterm

### Module Structure

- **`main.rs`** - App state machine, async message handling, keyboard input loop
- **`ui.rs`** - Terminal rendering with crossterm, event panel display, calendar grid
- **`cache.rs`** - `DisplayEvent` (unified event type), `SourceCache` (per-source), `EventCache` (disk persistence)
- **`config.rs`** - Config loading from `~/.config/calendarchy/config.json`, token storage
- **`google/`** - OAuth device flow (`auth.rs`), Calendar API client (`calendar.rs`), types (`types.rs`)
- **`icloud/`** - Basic auth (`auth.rs`), CalDAV client with REPORT queries (`calendar.rs`), iCal parser (`types.rs`)

### Key Types

- `DisplayEvent` - Normalized event with title, time_str, date, accepted, meeting_url
- `GoogleAuthState` / `ICloudAuthState` - Auth state machines (enums in main.rs)
- `AsyncMessage` - Channel messages from background tasks to main loop

### Data Flow

```
Config → Auth → Fetch Events → Convert to DisplayEvent → Store in SourceCache → Save to disk
                                                                              ↓
UI ← EventCache.get(date) ←──────────────────────────────────────────────────┘
```

### Caching

- Events cached to `~/.cache/calendarchy/events.json`
- Auth tokens stored in `~/.config/calendarchy/tokens.json`
- Cache loads on startup for instant display; `fetched_months` not restored to force refresh

## Release Process

To release a new version (e.g., 0.1.3 -> 0.1.4):

1. **Bump version** in `Cargo.toml`, `pkg/arch/PKGBUILD`, `pkg/arch-bin/PKGBUILD` (set sha256sums to SKIP temporarily)
2. **Commit and push** to master
3. **Tag and push**: `git tag v0.1.4 && git push origin v0.1.4`
4. **Wait for GitHub Actions** (`.github/workflows/release.yml`) — it builds macOS ARM/Intel + Linux binaries, creates the GitHub Release, and updates the Homebrew tap automatically
5. **Compute sha256sums** from released artifacts:
   - Source: `curl -sL "https://github.com/sovanesyan/calendarchy/archive/v0.1.4.tar.gz" | sha256sum`
   - Linux binary: `curl -sL "https://github.com/sovanesyan/calendarchy/releases/download/v0.1.4/calendarchy-x86_64-unknown-linux-gnu.tar.gz" | sha256sum`
6. **Update PKGBUILDs** with real sha256sums, commit and push to master
7. **Update AUR packages** (two separate repos, push manually):
   - `git clone ssh://aur@aur.archlinux.org/calendarchy-bin.git` — copy from `pkg/arch-bin/PKGBUILD`
   - `git clone ssh://aur@aur.archlinux.org/calendarchy.git` — copy from `pkg/arch/PKGBUILD`
   - Generate `.SRCINFO`: `makepkg --printsrcinfo > .SRCINFO`
   - Commit and push each

### Package Locations

- **Homebrew**: `sovanesyan/homebrew-calendarchy` (auto-updated by release workflow via `HOMEBREW_TAP_TOKEN` secret)
- **AUR binary**: `ssh://aur@aur.archlinux.org/calendarchy-bin.git`
- **AUR source**: `ssh://aur@aur.archlinux.org/calendarchy.git`

### Website

- GitHub Pages from `docs/` folder on master
- Self-hosts Cascadia Code SemiBold font for block art logo
- Terminal mockup SVG generated from ANSI capture via `/tmp/ansi2svg.py`
