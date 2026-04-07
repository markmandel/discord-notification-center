# discord-notification-center

Listens for Discord notifications via the Discord RPC IPC interface and displays them.

## Commands

### `daemon` (default)

Connects to the running Discord client via IPC, authenticates, and subscribes to
`NOTIFICATION_CREATE` events, printing them to stdout.

```
discord-notification-center
discord-notification-center daemon
```

### `show`

Opens the notification window (placeholder).

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

## Building

```
cargo build --release
```
