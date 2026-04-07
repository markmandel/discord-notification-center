# discord-notification-center

A notification center for Discord. The daemon listens for incoming notifications
via the Discord RPC IPC interface and stores them in a local SQLite database.
A GUI (in progress) will display them as a Wayland layer-shell overlay.

## Commands

### `daemon` (default)

Connects to the running Discord client via IPC, authenticates, subscribes to
`NOTIFICATION_CREATE` events, and persists each notification to the local
database.

```
discord-notification-center
discord-notification-center daemon
```

### `show`

Opens the notification window (placeholder — GUI not yet implemented).

```
discord-notification-center show
```

## Configuration

Create a config file at `~/.config/discord-notification-center/config.toml`:

```toml
client_id = "your-discord-client-id"
client_secret = "your-discord-client-secret"
```

`client_id` and `client_secret` come from your application in the
[Discord Developer Portal](https://discord.com/developers/applications). The app
must have the `rpc` and `rpc.notifications.read` OAuth2 scopes enabled.

On first run the daemon will create the SQLite database automatically at
`~/.config/discord-notification-center/notifications.db`.

## Building

### Debian/Ubuntu dependencies

The GTK4 layer-shell library and its build dependencies must be installed before
`cargo build` will succeed:

```
sudo apt install libgtk4-layer-shell-dev libgtk-4-dev libwayland-dev wayland-protocols pkg-config
```

The runtime libraries (`libgtk4-layer-shell0`, `libgtk-4-1`, `libwayland-client0`)
are pulled in automatically as dependencies of the dev packages above.

### Compile

```
cargo build --release
```
