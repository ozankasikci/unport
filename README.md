[![Crates.io](https://img.shields.io/crates/v/unport-cli.svg)](https://crates.io/crates/unport-cli)
[![Downloads](https://img.shields.io/crates/d/unport-cli.svg)](https://crates.io/crates/unport-cli)
[![License](https://img.shields.io/crates/l/unport-cli.svg)](LICENSE)
![Coverage](assets/coverage.svg)

<img src="assets/banner.jpeg" alt="Unport - Local Port Management Daemon" width="100%">

Local development port manager. Access your apps via clean domains like `http://api.localhost` instead of `localhost:3847`.

## Table of Contents

- [Installation](#installation)
- [Quick Start](#quick-start)
- [How It Works](#how-it-works)
- [Commands](#commands)
- [HTTPS Support](#https-support)
- [Config](#config)
- [License](#license)

## Installation

```bash
cargo install unport-cli
```

Or with Homebrew:

```bash
brew tap ozankasikci/tap && brew install unport
```

## Quick Start

```bash
# 1. Start the daemon (needs sudo for port 80)
sudo unport daemon start -d

# 2. In your project directory, create unport.json
echo '{"domain": "myapp"}' > unport.json

# 3. Start your app
unport start

# Your app is now at http://myapp.localhost
```

## How It Works

unport runs a reverse proxy on port 80 that routes requests based on the `Host` header.

```
Browser: http://api.localhost/users
    ↓
unport daemon (port 80)
    ↓ routes api.localhost → port 4000
Your app (port 4000)
```

When you run `unport start`:

1. Reads `domain` from `unport.json`
2. Gets an available port from the daemon (4000-4999 range)
3. Detects your framework and starts your app with that port
4. Registers the domain→port mapping with the daemon

The daemon handles all routing. Your apps don't need to know about each other—just use stable domains like `http://api.localhost`.

## Commands

| Command | Description |
|---------|-------------|
| `sudo unport daemon start -d` | Start daemon in background |
| `sudo unport daemon start -d --https` | Start daemon with HTTPS support (ports 80 + 443) |
| `unport daemon status` | Show daemon status (PID, uptime, services) |
| `unport daemon stop` | Stop the daemon |
| `unport start` | Start app in current directory |
| `unport list` | Show all running services |
| `unport stop <domain>` | Stop a service |
| `sudo unport trust-ca` | Add unport CA to system trust store (for HTTPS) |
| `sudo unport trust-ca --remove` | Remove unport CA from system trust store |
| `unport clean-certs` | Delete generated TLS certificates |
| `unport regen-cert` | Regenerate TLS certificate for all domains |

## Config

Create `unport.json` in your project:

```json
{
  "domain": "myapp"
}
```

Most frameworks are auto-detected (Next.js, Vite, Express, Django, Rails, Go, etc.). If detection fails, add a start command:

```json
{
  "domain": "myapp",
  "start": "npm run serve"
}
```

Your app must read the port from the `PORT` environment variable:

```js
// Node.js
const port = process.env.PORT || 3000;
```

```go
// Go
port := os.Getenv("PORT")
```

## HTTPS Support

unport can serve your apps over HTTPS with automatically generated certificates.

### Setup

```bash
# 1. Start daemon with HTTPS enabled
sudo unport daemon start -d --https

# 2. Trust the CA certificate (one-time setup)
sudo unport trust-ca

# 3. Start your app as usual
unport start

# Your app is now available at both:
# - http://myapp.localhost
# - https://myapp.localhost
```

### How it works

When started with `--https`, unport:

1. Generates a local CA certificate (stored in `~/.unport/ca.crt`)
2. Creates TLS certificates for `*.localhost` domains
3. Listens on both port 80 (HTTP) and port 443 (HTTPS)
4. Automatically updates certificates when new domains are registered

The CA only needs to be trusted once. After that, all `*.localhost` domains will have valid HTTPS.

### Certificate management

```bash
# Regenerate certificates (e.g., after adding many domains)
unport regen-cert

# Remove CA from trust store
sudo unport trust-ca --remove

# Delete all generated certificates
unport clean-certs
```

## License

MIT
