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
use rusqlite::Connection;
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

fn config_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "~".into());
    Ok(PathBuf::from(home)
        .join(".config")
        .join("discord-notification-center"))
}

fn load_config() -> Result<Config> {
    let path = config_dir()?.join("config.toml");

    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read config at {}: {e}", path.display()))?;

    let config: Config = toml::from_str(&raw)
        .map_err(|e| format!("invalid config at {}: {e}", path.display()))?;

    Ok(config)
}

// ---------------------------------------------------------------------------
// Database
// ---------------------------------------------------------------------------

fn open_db() -> Result<Connection> {
    let dir = config_dir()?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("cannot create config dir {}: {e}", dir.display()))?;

    let db_path = dir.join("notifications.db");
    let conn = Connection::open(&db_path)
        .map_err(|e| format!("cannot open database {}: {e}", db_path.display()))?;

    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS notifications (
            id                   INTEGER PRIMARY KEY AUTOINCREMENT,
            received_at          TEXT    NOT NULL,
            read                 INTEGER NOT NULL DEFAULT 0,
            pinned               INTEGER NOT NULL DEFAULT 0,

            -- top-level notification fields
            channel_id           TEXT    NOT NULL,
            title                TEXT    NOT NULL,
            body                 TEXT    NOT NULL,
            icon_url             TEXT,

            -- author
            author_id            TEXT    NOT NULL,
            author_username      TEXT    NOT NULL,
            author_discriminator TEXT,
            author_avatar        TEXT,
            author_color         TEXT,
            author_bot           INTEGER NOT NULL DEFAULT 0,

            -- message
            message_id           TEXT    NOT NULL,
            message_timestamp    TEXT,
            message_content      TEXT,
            message_type         INTEGER
        );
    ")?;

    println!("[db] opened {}", db_path.display());
    Ok(conn)
}

fn store_notification(conn: &Connection, data: &Value) -> Result<()> {
    let channel_id = data["channel_id"].as_str().unwrap_or("");
    let title      = data["title"].as_str().unwrap_or("");
    let body       = data["body"].as_str().unwrap_or("");
    let icon_url   = data["icon_url"].as_str();

    let msg    = &data["message"];
    let author = &msg["author"];

    let author_id            = author["id"].as_str().unwrap_or("");
    let author_username      = author["username"].as_str().unwrap_or("");
    let author_discriminator = author["discriminator"].as_str();
    let author_avatar        = author["avatar"].as_str();
    let author_color         = msg["author_color"].as_str();
    let author_bot           = author["bot"].as_bool().unwrap_or(false) as i32;

    let message_id        = msg["id"].as_str().unwrap_or("");
    let message_timestamp = msg["timestamp"].as_str();
    let message_content   = msg["content"].as_str();
    let message_type      = msg["type"].as_i64();

    let received_at = chrono::Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO notifications (
            received_at, channel_id, title, body, icon_url,
            author_id, author_username, author_discriminator, author_avatar,
            author_color, author_bot,
            message_id, message_timestamp, message_content, message_type
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5,
            ?6, ?7, ?8, ?9,
            ?10, ?11,
            ?12, ?13, ?14, ?15
        )",
        rusqlite::params![
            received_at, channel_id, title, body, icon_url,
            author_id, author_username, author_discriminator, author_avatar,
            author_color, author_bot,
            message_id, message_timestamp, message_content, message_type,
        ],
    )?;

    println!("[db] stored notification from {author_username} in channel {channel_id}");
    Ok(())
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
    let db = open_db()?;

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

                if data["evt"] == "NOTIFICATION_CREATE" {
                    if let Err(e) = store_notification(&db, &data["data"]) {
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
