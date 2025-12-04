# unport

Local development port manager. Access your apps via clean domains like `http://api.localhost` instead of `localhost:3847`.

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
| `unport daemon status` | Show daemon status (PID, uptime, services) |
| `unport daemon stop` | Stop the daemon |
| `unport start` | Start app in current directory |
| `unport list` | Show all running services |
| `unport stop <domain>` | Stop a service |

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

## License

MIT
