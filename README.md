# discord-notification-center

A notification center for Discord. The daemon listens for incoming notifications
via the Discord RPC IPC interface and stores them in a local SQLite database.
The `toggle` command opens a Wayland layer-shell panel (Rose Pine themed) that
displays unread notifications and lets you act on them.

## Commands

### `daemon` (default)

Connects to the running Discord client via IPC, authenticates, and subscribes to
`NOTIFICATION_CREATE` events. Each notification is persisted to the local SQLite
database, including the `guild_id` resolved via a follow-up `GET_CHANNEL` RPC call
(needed for Discord deep links).

If Discord is not running when the daemon starts, it waits and retries every 5
seconds until it can connect. If the connection drops while running (e.g. Discord
is restarted), the daemon reconnects automatically on the same 5-second poll.

A system notification (`notify-send`) is shown on first connect, on disconnection,
and on reconnect.

Only one daemon instance may run at a time; a second invocation will exit
immediately with an error.

Notifications older than 24 hours (excluding pinned) are deleted on startup and
once every 4 hours while the daemon is running.

```
discord-notification-center
discord-notification-center daemon
```

### `toggle`

Toggles the Wayland layer-shell notification panel: slides in from the right if
not visible, or closes it if already open. Bind this to a key in your compositor
for quick access.

```
discord-notification-center toggle
```

**Panel features:**

- Only notifications from the last 24 hours are shown (pinned notifications are
  always visible regardless of age)
- Notifications are listed newest-first; new arrivals appear while the panel is
  open (polled every second from the database)
- Sender avatars are loaded asynchronously from Discord's CDN
- Multiple unread messages from the same channel or DM are **grouped** into a
  single summary row; hovering expands the group to show individual messages
- Each notification (and each row inside an expanded group) has two action buttons:
  - **📌 / 📍** — pin or unpin (pinned notifications stay visible until explicitly unpinned)
  - **✓ / ↩** — mark as read / mark as unread
- Clicking a notification (or a group header) navigates directly to that channel
  in the Discord client and marks it as read
- **✓** on a group header marks all messages in that channel as read
- **✓ all** button in the header marks every notification as read
- **All** toggle (or **Ctrl+F**) switches to full-history mode and focuses the
  filter field; switching back clears the filter
- **Filter field** (All mode only) — type to instantly filter notifications by
  channel name, body text, sender, or message content; the **✕** button clears it
- **Esc** closes the panel

## Keyboard shortcuts

| Key | Action |
|-----|--------|
| `Ctrl+F` | Switch to All mode and focus the filter field |
| `Esc` | Close the panel |

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

## LICENCE

Apache 2.0
