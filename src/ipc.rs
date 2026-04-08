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

/// Opens a channel directly in the Discord client via a fresh IPC connection.
/// No AUTHORIZE/AUTHENTICATE required — RPC_LOCAL_SCOPE is granted automatically
/// to all IPC socket connections. For DMs pass `guild_id = None`.
pub fn open_channel_in_discord(
    client_id: &str,
    guild_id: Option<&str>,
    channel_id: &str,
) -> Result<()> {
    let mut client = DiscordIpcClient::new(client_id);
    client.connect()?;

    let guild = guild_id.unwrap_or("@me");
    client.send(
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
    )?;
    client.recv()?;

    Ok(())
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
