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

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::thread;

use clap::{Parser, Subcommand};
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};
use serde::Deserialize;
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct Config {
    client_id: String,
    client_secret: String,
}

fn load_config() -> Result<Config> {
    let path: PathBuf = [
        &std::env::var("HOME").unwrap_or_else(|_| "~".into()),
        ".config",
        "discord-notification-center",
        "config.toml",
    ]
    .iter()
    .collect();

    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read config at {}: {e}", path.display()))?;

    let config: Config = toml::from_str(&raw)
        .map_err(|e| format!("invalid config at {}: {e}", path.display()))?;

    Ok(config)
}

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
    /// Show the notification window
    Show,
}

// ---------------------------------------------------------------------------
// Daemon helpers (lifted from discord-ipc-example)
// ---------------------------------------------------------------------------

fn opcode_name(op: u32) -> &'static str {
    match op {
        0 => "HANDSHAKE",
        1 => "FRAME",
        2 => "CLOSE",
        3 => "PING",
        4 => "PONG",
        _ => "UNKNOWN",
    }
}

fn start_redirect_server() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:8080")?;

    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };

            let mut request_line = String::new();
            BufReader::new(&stream).read_line(&mut request_line).ok();
            println!("[redirect server] {request_line}");

            let body = b"<h1>Authorized! You may close this tab.</h1>";
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(body);
        }
    });

    Ok(())
}

fn authorize(client: &mut DiscordIpcClient, client_id: &str) -> Result<String> {
    client.send(
        json!({
            "cmd": "AUTHORIZE",
            "args": {
                "client_id": client_id,
                "scopes": ["rpc", "rpc.notifications.read"]
            },
            "nonce": "authorize"
        }),
        1,
    )?;

    let (_, data) = client.recv()?;
    println!("[authorize response]\n{}\n", serde_json::to_string_pretty(&data)?);

    let code = data["data"]["code"]
        .as_str()
        .ok_or("AUTHORIZE response missing data.code")?
        .to_string();

    Ok(code)
}

fn exchange_code(code: &str, client_id: &str, client_secret: &str, redirect_uri: &str) -> Result<String> {
    let response = reqwest::blocking::Client::new()
        .post("https://discord.com/api/oauth2/token")
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("scope", "rpc"),
        ])
        .send()?
        .json::<Value>()?;

    println!("[token exchange response]\n{}\n", serde_json::to_string_pretty(&response)?);

    let access_token = response["access_token"]
        .as_str()
        .ok_or("token exchange response missing access_token")?
        .to_string();

    Ok(access_token)
}

fn authenticate(client: &mut DiscordIpcClient, access_token: &str) -> Result<()> {
    client.send(
        json!({
            "cmd": "AUTHENTICATE",
            "args": {
                "access_token": access_token
            },
            "nonce": "authenticate"
        }),
        1,
    )?;

    let (_, data) = client.recv()?;
    println!("[authenticate response]\n{}\n", serde_json::to_string_pretty(&data)?);

    if data["evt"] == "ERROR" {
        return Err(format!("AUTHENTICATE failed: {}", data["data"]["message"]).into());
    }

    Ok(())
}

fn subscribe(client: &mut DiscordIpcClient, evt: &str, args: Value) -> Result<()> {
    client.send(
        json!({
            "cmd": "SUBSCRIBE",
            "evt": evt,
            "args": args,
            "nonce": format!("sub-{evt}")
        }),
        1,
    )?;
    let (_, data) = client.recv()?;
    println!("[subscribe] {evt}: {}\n", serde_json::to_string_pretty(&data)?);
    Ok(())
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

fn run_daemon(config: Config) -> Result<()> {
    let redirect_uri = "http://localhost:8080/callback";
    start_redirect_server()?;
    println!("redirect server listening at {redirect_uri}\n");

    let mut client = DiscordIpcClient::new(&config.client_id);
    client.connect()?;
    println!("connected (READY consumed during handshake)\n");

    let code = authorize(&mut client, &config.client_id)?;
    let access_token = exchange_code(&code, &config.client_id, &config.client_secret, redirect_uri)?;
    authenticate(&mut client, &access_token)?;
    println!("authenticated\n");

    subscribe(&mut client, "NOTIFICATION_CREATE", json!({}))?;

    loop {
        match client.recv() {
            Ok((op, data)) => {
                println!(
                    "[{}]\n{}\n",
                    opcode_name(op),
                    serde_json::to_string_pretty(&data)?
                );
            }
            Err(e) => {
                eprintln!("recv error: {e}");
                break;
            }
        }
    }

    Ok(())
}

fn run_show() {
    println!("TODO: show notification window");
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command.unwrap_or(Command::Daemon) {
        Command::Daemon => {
            let config = load_config()?;
            run_daemon(config)?;
        }
        Command::Show => {
            run_show();
        }
    }

    Ok(())
}
