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

    let mut client = DiscordIpcClient::new(&cfg.client_id);
    client.connect()?;
    println!("connected (READY consumed during handshake)\n");

    let code = ipc::authorize(&mut client, &cfg.client_id)?;
    let access_token =
        ipc::exchange_code(&code, &cfg.client_id, &cfg.client_secret, redirect_uri)?;
    ipc::authenticate(&mut client, &access_token)?;
    println!("authenticated\n");

    ipc::subscribe(&mut client, "NOTIFICATION_CREATE", json!({}))?;

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
                    let guild_id =
                        ipc::get_channel_guild_id(&mut client, channel_id);

                    if let Err(e) =
                        db::store_notification(&db, &data["data"], guild_id.as_deref())
                    {
                        eprintln!("[db] error storing notification: {e}");
                    }
                }
            }
            Err(e) => {
                eprintln!("recv error: {e}");
                break;
            }
        }
    }

    Ok(())
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
