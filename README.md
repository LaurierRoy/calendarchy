# Calendarchy

A terminal calendar app that displays Google Calendar and iCloud Calendar events side by side.

## Installation

### macOS (Homebrew)

```bash
brew tap sovanesyan/calendarchy
brew install calendarchy
```

### Omarchy / Linux (from source)

Requires [Rust](https://rustup.rs/).

```bash
git clone https://github.com/sovanesyan/calendarchy.git
cd calendarchy
cargo build --release
sudo cp target/release/calendarchy /usr/local/bin/
```

To add Calendarchy to your application launcher, copy the desktop entry:

```bash
sudo cp calendarchy.desktop /usr/share/applications/
```

## Configuration

Create `~/.config/calendarchy/config.json`:

```json
{
  "google": {
    "client_id": "YOUR_CLIENT_ID",
    "client_secret": "YOUR_CLIENT_SECRET"
  },
  "icloud": {
    "username": "YOUR_APPLE_ID",
    "app_password": "YOUR_APP_SPECIFIC_PASSWORD"
  }
}
```

Either source can be omitted if you only use one.

## Usage

```bash
calendarchy
```

Navigate with arrow keys. Events from both calendars are displayed in side-by-side panels.
