# unport - Local Development Port Manager

## Overview

`unport` solves port collisions in local development. Instead of remembering which app runs on which port, you assign each project a domain name and access it via `http://myapp.localhost`.

## Core Components

### 1. Daemon (`unport daemon`)

Background process that:
- Runs HTTP reverse proxy on port 80
- Maintains registry of domain → port mappings
- Listens for commands from CLI via Unix socket at `~/.unport/unport.sock`

### 2. CLI (`unport start`, `unport list`, etc.)

Commands that communicate with the daemon to register services, start apps, and query status.

### 3. Config File (`unport.json`)

Minimal config per project:

```json
{
  "domain": "api"
}
```

Optional overrides for edge cases:

```json
{
  "domain": "api",
  "start": "node server.js",
  "portEnv": "SERVER_PORT"
}
```

## Data Flow

```
unport start ──(unix socket)──▶ daemon
                                  │
                                  ▼
                            ┌──────────┐
browser ──▶ :80 proxy ──▶   │ registry │ ──▶ localhost:4000
                            └──────────┘
```

## CLI Commands

### `unport daemon`

Starts the background daemon. Creates Unix socket and PID file.

### `unport start`

1. Reads `unport.json` from current directory
2. Detects framework from `package.json`
3. Requests available port from daemon
4. Registers domain → port mapping
5. Starts app with port injection
6. On exit (Ctrl+C or crash), unregisters from daemon

### `unport stop <domain>`

Stops a running app by domain name.

### `unport list`

Shows all registered services:

```
DOMAIN              PORT    PID     STATUS
api.localhost       4000    12345   running
app.localhost       4001    12346   running
```

### `unport daemon stop`

Stops the daemon and all registered services.

## Framework Detection

Reads `package.json` to detect framework and determine port injection strategy.

| Check | Framework | Port Strategy |
|-------|-----------|---------------|
| `"next"` in dependencies | Next.js | `PORT={port}` env |
| `"vite"` in dependencies | Vite | `npm run dev -- --port {port}` |
| `"react-scripts"` in dependencies | Create React App | `PORT={port}` env |
| `"@remix-run/dev"` in dependencies | Remix | `PORT={port}` env |
| `"nuxt"` in dependencies | Nuxt | `PORT={port}` env |
| `"express"` in dependencies | Express | `PORT={port}` env |
| `"fastify"` in dependencies | Fastify | `PORT={port}` env |
| `"@nestjs/core"` in dependencies | NestJS | `PORT={port}` env |
| `Gemfile` exists | Rails | `rails server -p {port}` |
| `manage.py` exists | Django | `python manage.py runserver 0.0.0.0:{port}` |

Fallback: If no framework detected, use `PORT={port}` env var.

## Project Structure

```
unport/
├── Cargo.toml
├── src/
│   ├── main.rs           # CLI entry point, argument parsing
│   ├── daemon.rs         # Daemon process, proxy server, registry
│   ├── client.rs         # CLI commands that talk to daemon
│   ├── config.rs         # unport.json parsing
│   ├── detect.rs         # Framework detection logic
│   ├── process.rs        # Spawn/manage child processes
│   └── types.rs          # Shared types (Domain, Service, etc.)
```

## Runtime Files

```
~/.unport/
├── unport.sock           # Unix socket for CLI ↔ daemon
├── registry.json         # Persisted domain → port mappings
└── unport.pid            # Daemon PID file
```

## Dependencies

| Crate | Purpose |
|-------|---------|
| `clap` | CLI argument parsing |
| `tokio` | Async runtime |
| `hyper` | HTTP proxy server |
| `serde` / `serde_json` | JSON parsing |
| `daemonize` | Fork to background |

## Error Handling

**Daemon not running:**
```
Error: Daemon not running. Start it with: unport daemon
```

**Port 80 already in use:**
```
Error: Port 80 in use. Stop the process using it or choose a different port.
```

**Domain already registered:**
```
Error: Domain "api" already registered (PID 12345).
Use "unport stop api" first.
```

**Framework not detected:**
```
Warning: Could not detect framework. Using PORT env var.
Starting: PORT=4000 npm run dev
```

**App crashes:** Auto-unregister from daemon, show exit code.

**Daemon crashes:** Apps keep running. On restart, daemon checks PIDs and cleans stale entries.

**Ctrl+C:** Catch SIGINT, send SIGTERM to child, unregister, clean exit.

## Implementation Order

### Phase 1: Foundation
1. Set up Cargo project with dependencies
2. Implement `types.rs` - shared structs (Config, Service, Message)
3. Implement `config.rs` - parse `unport.json`

### Phase 2: Daemon
4. Basic daemon that listens on Unix socket
5. Registry - store/retrieve domain → port mappings
6. HTTP proxy using hyper - route requests by Host header

### Phase 3: CLI Commands
7. `unport daemon` - start daemon in background
8. `unport list` - query and display registry
9. `unport start` - register, spawn app, handle exit
10. `unport stop` - stop a service by domain

### Phase 4: Framework Detection
11. Implement `detect.rs` - read package.json, detect framework
12. Port injection strategies (env var vs CLI flag)

### Phase 5: Polish
13. Error messages and edge cases
14. Cleanup stale entries on daemon restart
15. Signal handling (Ctrl+C graceful shutdown)
