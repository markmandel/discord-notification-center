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
use std::sync::mpsc;
use std::thread;

use discord_rich_presence::{DiscordIpc, DiscordIpcClient};
use serde_json::{json, Value};

use crate::Result;

pub fn opcode_name(op: u32) -> &'static str {
    match op {
        0 => "HANDSHAKE",
        1 => "FRAME",
        2 => "CLOSE",
        3 => "PING",
        4 => "PONG",
        _ => "UNKNOWN",
    }
}

pub fn start_redirect_server() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:8080")?;

    thread::spawn(move || {
        // Accept exactly one connection — the OAuth callback — then stop,
        // which drops the listener and releases port 8080.
        if let Ok((mut stream, _)) = listener.accept() {
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

pub fn authorize(client: &mut DiscordIpcClient, client_id: &str) -> Result<String> {
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

pub fn exchange_code(
    code: &str,
    client_id: &str,
    client_secret: &str,
    redirect_uri: &str,
) -> Result<String> {
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

    println!(
        "[token exchange response]\n{}\n",
        serde_json::to_string_pretty(&response)?
    );

    let access_token = response["access_token"]
        .as_str()
        .ok_or("token exchange response missing access_token")?
        .to_string();

    Ok(access_token)
}

pub fn authenticate(client: &mut DiscordIpcClient, access_token: &str) -> Result<()> {
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
    println!(
        "[authenticate response]\n{}\n",
        serde_json::to_string_pretty(&data)?
    );

    if data["evt"] == "ERROR" {
        return Err(format!("AUTHENTICATE failed: {}", data["data"]["message"]).into());
    }

    Ok(())
}

pub fn subscribe(client: &mut DiscordIpcClient, evt: &str, args: Value) -> Result<()> {
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
    println!(
        "[subscribe] {evt}: {}\n",
        serde_json::to_string_pretty(&data)?
    );
    Ok(())
}

/// Spawns a background thread that keeps a single `DiscordIpcClient` connection
/// alive and processes navigation requests sent over the returned channel.
/// Each message is `(guild_id, channel_id)`; use `guild_id = None` for DMs.
/// The connection is established lazily on the first message and re-established
/// automatically if it ever drops.
pub fn start_navigation_worker(client_id: String) -> mpsc::Sender<(Option<String>, String)> {
    let (tx, rx) = mpsc::channel::<(Option<String>, String)>();

    thread::spawn(move || {
        let mut client: Option<DiscordIpcClient> = None;

        for (guild_id, channel_id) in rx {
            // Reconnect if we don't have a live connection.
            if client.is_none() {
                let mut c = DiscordIpcClient::new(&client_id);
                match c.connect() {
                    Ok(_) => client = Some(c),
                    Err(e) => {
                        eprintln!("[nav] connect failed: {e}");
                        continue;
                    }
                }
            }

            let guild = guild_id.as_deref().unwrap_or("@me");
            if let Err(e) = client.as_mut().unwrap().send(
                json!({
                    "cmd": "DEEP_LINK",
                    "args": {
                        "type": "CHANNEL",
                        "params": {
                            "guildId": guild,
                            "channelId": channel_id
                        }
                    },
                    "nonce": format!("deep-link-{channel_id}")
                }),
                1,
            ) {
                eprintln!("[nav] send failed: {e}");
                client = None; // will reconnect on next message
            }
        }
    });

    tx
}

/// Issues a GET_CHANNEL command and returns the guild_id from the response,
/// or None for DMs (channel type 1 or 3) or if the field is absent.
pub fn get_channel_guild_id(
    client: &mut DiscordIpcClient,
    channel_id: &str,
) -> Option<String> {
    client
        .send(
            json!({
                "cmd": "GET_CHANNEL",
                "args": { "channel_id": channel_id },
                "nonce": format!("get-channel-{channel_id}")
            }),
            1,
        )
        .ok()?;

    let (_, data) = client.recv().ok()?;

    // Type 1 = DM, Type 3 = Group DM — no guild
    let channel_type = data["data"]["type"].as_i64().unwrap_or(-1);
    if channel_type == 1 || channel_type == 3 {
        return None;
    }

    let guild_id = data["data"]["guild_id"].as_str()?;
    if guild_id.is_empty() {
        return None;
    }
    Some(guild_id.to_string())
}
