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

use rusqlite::Connection;
use serde_json::Value;

use crate::config::config_dir;
use crate::Result;

pub struct Notification {
    pub id: i64,
    pub received_at: String,
    pub read: bool,
    pub pinned: bool,
    pub channel_id: String,
    pub title: String,
    pub body: String,
    pub icon_url: Option<String>,
    pub author_id: String,
    pub author_username: String,
    pub author_discriminator: Option<String>,
    pub author_avatar: Option<String>,
    pub author_color: Option<String>,
    pub author_bot: bool,
    pub message_id: String,
    pub message_timestamp: Option<String>,
    pub message_content: Option<String>,
    pub message_type: Option<i64>,
    pub guild_id: Option<String>,
}

pub fn open_db() -> Result<Connection> {
    let dir = config_dir()?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("cannot create config dir {}: {e}", dir.display()))?;

    let db_path = dir.join("notifications.db");
    let conn = Connection::open(&db_path)
        .map_err(|e| format!("cannot open database {}: {e}", db_path.display()))?;

    conn.busy_timeout(std::time::Duration::from_secs(5))?;

    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS notifications (
            id                   INTEGER PRIMARY KEY AUTOINCREMENT,
            received_at          TEXT    NOT NULL,
            read                 INTEGER NOT NULL DEFAULT 0,
            pinned               INTEGER NOT NULL DEFAULT 0,

            channel_id           TEXT    NOT NULL,
            title                TEXT    NOT NULL,
            body                 TEXT    NOT NULL,
            icon_url             TEXT,

            author_id            TEXT    NOT NULL,
            author_username      TEXT    NOT NULL,
            author_discriminator TEXT,
            author_avatar        TEXT,
            author_color         TEXT,
            author_bot           INTEGER NOT NULL DEFAULT 0,

            message_id           TEXT    NOT NULL,
            message_timestamp    TEXT,
            message_content      TEXT,
            message_type         INTEGER,

            guild_id             TEXT
        );
    ")?;

    println!("[db] opened {}", db_path.display());
    Ok(conn)
}

pub fn store_notification(conn: &Connection, data: &Value, guild_id: Option<&str>) -> Result<()> {
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
            message_id, message_timestamp, message_content, message_type,
            guild_id
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5,
            ?6, ?7, ?8, ?9,
            ?10, ?11,
            ?12, ?13, ?14, ?15,
            ?16
        )",
        rusqlite::params![
            received_at, channel_id, title, body, icon_url,
            author_id, author_username, author_discriminator, author_avatar,
            author_color, author_bot,
            message_id, message_timestamp, message_content, message_type,
            guild_id,
        ],
    )?;

    println!("[db] stored notification from {author_username} in channel {channel_id}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Show-command queries
// ---------------------------------------------------------------------------

pub fn fetch_display(conn: &Connection, show_all: bool) -> Result<Vec<Notification>> {
    let sql = if show_all {
        "SELECT id, received_at, read, pinned, channel_id, title, body, icon_url,
                author_id, author_username, author_discriminator, author_avatar,
                author_color, author_bot, message_id, message_timestamp,
                message_content, message_type, guild_id
         FROM notifications
         ORDER BY received_at DESC"
    } else {
        "SELECT id, received_at, read, pinned, channel_id, title, body, icon_url,
                author_id, author_username, author_discriminator, author_avatar,
                author_color, author_bot, message_id, message_timestamp,
                message_content, message_type, guild_id
         FROM notifications
         WHERE read = 0 OR pinned = 1
         ORDER BY received_at DESC"
    };

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], map_row)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn fetch_new_since(conn: &Connection, last_id: i64) -> Result<Vec<Notification>> {
    let mut stmt = conn.prepare(
        "SELECT id, received_at, read, pinned, channel_id, title, body, icon_url,
                author_id, author_username, author_discriminator, author_avatar,
                author_color, author_bot, message_id, message_timestamp,
                message_content, message_type, guild_id
         FROM notifications
         WHERE id > ?1 AND (read = 0 OR pinned = 1)
         ORDER BY id ASC",
    )?;
    let rows = stmt.query_map([last_id], map_row)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn set_read(conn: &Connection, id: i64, read: bool) -> Result<()> {
    conn.execute(
        "UPDATE notifications SET read = ?1 WHERE id = ?2",
        rusqlite::params![read as i32, id],
    )?;
    Ok(())
}

pub fn set_pinned(conn: &Connection, id: i64, pinned: bool) -> Result<()> {
    conn.execute(
        "UPDATE notifications SET pinned = ?1 WHERE id = ?2",
        rusqlite::params![pinned as i32, id],
    )?;
    Ok(())
}

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Notification> {
    Ok(Notification {
        id:                   row.get(0)?,
        received_at:          row.get(1)?,
        read:                 row.get::<_, i32>(2)? != 0,
        pinned:               row.get::<_, i32>(3)? != 0,
        channel_id:           row.get(4)?,
        title:                row.get(5)?,
        body:                 row.get(6)?,
        icon_url:             row.get(7)?,
        author_id:            row.get(8)?,
        author_username:      row.get(9)?,
        author_discriminator: row.get(10)?,
        author_avatar:        row.get(11)?,
        author_color:         row.get(12)?,
        author_bot:           row.get::<_, i32>(13)? != 0,
        message_id:           row.get(14)?,
        message_timestamp:    row.get(15)?,
        message_content:      row.get(16)?,
        message_type:         row.get(17)?,
        guild_id:             row.get(18)?,
    })
}
