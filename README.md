# unport

Local development port manager. Access your apps via clean domains like `http://api.localhost` instead of `localhost:3847`.

## Installation

```bash
cargo install --path .
```

## Quick Start

```bash
# 1. Start the daemon (needs sudo for port 80)
sudo unport daemon

# 2. In your project directory, create unport.json
echo '{"domain": "myapp"}' > unport.json

# 3. Start your app
unport start

# Your app is now at http://myapp.localhost
```

## How It Works

1. **Daemon** runs a reverse proxy on port 80
2. **`unport start`** detects your framework, assigns a port, starts your app
3. **Proxy** routes `myapp.localhost` → your app's assigned port

No more port conflicts. No more remembering port numbers.

## Examples

### Next.js

```json
{
  "domain": "dashboard"
}
```

```bash
$ unport start
Detected framework: Next.js
Running: npm run dev (port 4000)
Available at: http://dashboard.localhost
```

### Vite / React

```json
{
  "domain": "frontend"
}
```

```bash
$ unport start
Detected framework: Vite
Running: npm run dev -- --port 4001
Available at: http://frontend.localhost
```

### Express / Node.js API

```json
{
  "domain": "api"
}
```

```bash
$ unport start
Detected framework: Express
Running: npm run dev (port 4002)
Available at: http://api.localhost
```

Your Express app should use `process.env.PORT`:

```js
const port = process.env.PORT || 3000;
app.listen(port);
```

### Go

Go projects need a custom start command:

```json
{
  "domain": "api",
  "start": "go run cmd/server/main.go"
}
```

Your Go app should use the `PORT` env var:

```go
port := os.Getenv("PORT")
if port == "" {
    port = "8080"
}
http.ListenAndServe(":"+port, router)
```

### Django

```json
{
  "domain": "backend"
}
```

```bash
$ unport start
Detected framework: Django
Running: python manage.py runserver 0.0.0.0:4003
Available at: http://backend.localhost
```

### Rails

```json
{
  "domain": "app"
}
```

```bash
$ unport start
Detected framework: Rails
Running: rails server -p 4004
Available at: http://app.localhost
```

### Custom Start Command

If auto-detection doesn't work, specify the command:

```json
{
  "domain": "myapp",
  "start": "npm run serve"
}
```

### Custom Port Environment Variable

If your app uses a different env var for port:

```json
{
  "domain": "myapp",
  "portEnv": "SERVER_PORT"
}
```

### Custom Port Argument

If your app uses a CLI flag for port:

```json
{
  "domain": "myapp",
  "start": "my-server",
  "portArg": "--listen-port"
}
```

## Running Multiple Apps

```bash
# Terminal 1: Backend
cd ~/projects/api
unport start
# → http://api.localhost

# Terminal 2: Frontend
cd ~/projects/frontend
unport start
# → http://frontend.localhost

# Terminal 3: Admin
cd ~/projects/admin
unport start
# → http://admin.localhost
```

Frontend can call backend using the stable domain:

```js
// frontend/.env
VITE_API_URL=http://api.localhost
```

## Commands

### `unport daemon`

Start the background daemon. Requires `sudo` for port 80.

```bash
sudo unport daemon
```

### `unport start`

Start your app and register with the daemon.

```bash
cd ~/myproject
unport start
```

### `unport list`

Show all running services.

```bash
$ unport list
DOMAIN                   PORT     PID      DIRECTORY
api.localhost            4000     12345    /Users/me/projects/api
frontend.localhost       4001     12346    /Users/me/projects/frontend
```

### `unport stop <domain>`

Stop a service by domain name.

```bash
unport stop api
```

### `unport daemon stop`

Stop the daemon and all services.

```bash
unport daemon stop
```

## Config Reference

### `unport.json`

| Field | Required | Description |
|-------|----------|-------------|
| `domain` | Yes | Domain name (becomes `<domain>.localhost`) |
| `start` | No | Custom start command (auto-detected if not set) |
| `portEnv` | No | Environment variable for port (default: `PORT`) |
| `portArg` | No | CLI argument for port (e.g., `--port`) |

### Auto-Detected Frameworks

| Framework | Detection | Port Strategy |
|-----------|-----------|---------------|
| Next.js | `next` in package.json | `PORT` env |
| Vite | `vite` in package.json | `--port` flag |
| Create React App | `react-scripts` in package.json | `PORT` env |
| Express | `express` in package.json | `PORT` env |
| NestJS | `@nestjs/core` in package.json | `PORT` env |
| Fastify | `fastify` in package.json | `PORT` env |
| Remix | `@remix-run/dev` in package.json | `PORT` env |
| Nuxt | `nuxt` in package.json | `PORT` env |
| Rails | `Gemfile` exists | `-p` flag |
| Django | `manage.py` exists | Appended to runserver |
| Go | `go.mod` exists | `PORT` env |

## Troubleshooting

### "Permission denied" when running `unport start`

The daemon creates a socket file. If you started the daemon with `sudo`, fix permissions:

```bash
sudo chmod 777 ~/.unport/unport.sock
```

### "Port 80 in use"

Another process (nginx, Apache) is using port 80. Stop it first:

```bash
sudo nginx -s stop
# or
sudo apachectl stop
```

### App starts but shows "Bad Gateway"

Your app isn't respecting the `PORT` environment variable. Make sure your app reads from `process.env.PORT` (Node.js) or `os.Getenv("PORT")` (Go).

### Framework not detected

Add a custom start command to `unport.json`:

```json
{
  "domain": "myapp",
  "start": "your-start-command"
}
```

## License

MIT
