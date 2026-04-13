// Copyright 2026 Mark Mandel
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

mod config;
mod db;
mod ipc;
mod show;

use std::thread;
use std::time::Duration;

use clap::{Parser, Subcommand};
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};
use serde_json::json;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "discord-notification-center")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Listen for Discord notifications via IPC (default)
    Daemon,
    /// Toggle the notification panel (show if hidden, hide if visible)
    Toggle,
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

fn run_gc(db: &rusqlite::Connection) {
    match db::delete_old_notifications(db) {
        Ok(n) => println!("[gc] deleted {n} old notification(s)"),
        Err(e) => eprintln!("[gc] error: {e}"),
    }
}

fn notify(summary: &str, body: &str) {
    let _ = std::process::Command::new("notify-send")
        .args(["--app-name=Discord Notification Center", summary, body])
        .spawn();
}

fn run_daemon(cfg: config::Config) -> Result<()> {
    let db = db::open_db()?;

    // GC on startup, then once every 24 hours on a separate connection.
    run_gc(&db);
    thread::spawn(|| loop {
        thread::sleep(Duration::from_secs(24 * 3600));
        match db::open_db() {
            Ok(conn) => run_gc(&conn),
            Err(e)   => eprintln!("[gc] could not open db: {e}"),
        }
    });

    let redirect_uri = "http://localhost:8080/callback";
    ipc::start_redirect_server()?;
    println!("redirect server listening at {redirect_uri}\n");

    // Full OAuth flow once — retries every 5s until Discord is available.
    let access_token = retry_until(
        "Discord not available",
        || notify("Discord Notification Center", "Waiting for Discord to start…"),
        || initial_auth(&cfg, redirect_uri),
    );

    let mut client = daemon_connect(&cfg.client_id, &access_token)?;
    notify("Discord Notification Center", "Connected to Discord.");

    loop {
        match client.recv() {
            Ok((op, data)) => {
                println!(
                    "[{}]\n{}\n",
                    ipc::opcode_name(op),
                    serde_json::to_string_pretty(&data)?
                );

                if data["evt"] == "NOTIFICATION_CREATE" {
                    let channel_id = data["data"]["channel_id"].as_str().unwrap_or("");
                    let guild_id = ipc::get_channel_guild_id(&mut client, channel_id);

                    if let Err(e) =
                        db::store_notification(&db, &data["data"], guild_id.as_deref())
                    {
                        eprintln!("[db] error storing notification: {e}");
                    }
                }
            }
            Err(e) => {
                eprintln!("[daemon] connection lost: {e}");
                notify("Discord Notification Center", "Lost connection to Discord, reconnecting…");
                client = retry_until(
                    "reconnect failed",
                    || {},
                    || daemon_connect(&cfg.client_id, &access_token),
                );
                notify("Discord Notification Center", "Reconnected to Discord.");
            }
        }
    }
}

/// Retry `op` every 5 seconds until it succeeds. Logs and calls `on_first_fail`
/// on the first failure, then retries silently.
fn retry_until<T>(label: &str, on_first_fail: impl Fn(), op: impl Fn() -> Result<T>) -> T {
    let mut first = true;
    loop {
        match op() {
            Ok(val) => return val,
            Err(e) => {
                if first {
                    eprintln!("[daemon] {label}: {e}");
                    on_first_fail();
                    first = false;
                }
                thread::sleep(Duration::from_secs(5));
            }
        }
    }
}

/// Try the full OAuth flow once; returns the access token or an error.
fn initial_auth(cfg: &config::Config, redirect_uri: &str) -> Result<String> {
    let mut client = DiscordIpcClient::new(&cfg.client_id);
    client.connect()?;
    let code = ipc::authorize(&mut client, &cfg.client_id)?;
    ipc::exchange_code(&code, &cfg.client_id, &cfg.client_secret, redirect_uri)
}

/// Authenticate and subscribe on a fresh IPC connection.
fn daemon_connect(client_id: &str, access_token: &str) -> Result<DiscordIpcClient> {
    let mut client = DiscordIpcClient::new(client_id);
    client.connect()?;
    ipc::authenticate(&mut client, access_token)?;
    println!("[daemon] authenticated\n");
    ipc::subscribe(&mut client, "NOTIFICATION_CREATE", json!({}))?;
    Ok(client)
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command.unwrap_or(Command::Daemon) {
        Command::Daemon => {
            let cfg = config::load_config()?;
            run_daemon(cfg)?;
        }
        Command::Toggle => {
            show::run()?;
        }
    }

    Ok(())
}
