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

fn run_daemon(cfg: config::Config) -> Result<()> {
    let db = db::open_db()?;

    let redirect_uri = "http://localhost:8080/callback";
    ipc::start_redirect_server()?;
    println!("redirect server listening at {redirect_uri}\n");

    // Full OAuth flow once to get a long-lived access token.
    let access_token = {
        let mut client = DiscordIpcClient::new(&cfg.client_id);
        client.connect()?;
        let code = ipc::authorize(&mut client, &cfg.client_id)?;
        ipc::exchange_code(&code, &cfg.client_id, &cfg.client_secret, redirect_uri)?
    };

    let mut client = daemon_connect(&cfg.client_id, &access_token)?;

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
                client = daemon_reconnect(&cfg.client_id, &access_token);
            }
        }
    }
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

/// Retry `daemon_connect` with exponential backoff until it succeeds.
fn daemon_reconnect(client_id: &str, access_token: &str) -> DiscordIpcClient {
    let backoff = [5u64, 10, 30, 60];
    let mut attempt = 0usize;
    loop {
        let secs = backoff[attempt.min(backoff.len() - 1)];
        eprintln!("[daemon] reconnecting in {secs}s…");
        thread::sleep(Duration::from_secs(secs));
        match daemon_connect(client_id, access_token) {
            Ok(client) => {
                println!("[daemon] reconnected");
                return client;
            }
            Err(e) => {
                eprintln!("[daemon] reconnect failed: {e}");
                attempt += 1;
            }
        }
    }
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
