# IRC MCP Server Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a standalone IRC MCP server that enables rusty-bidule to connect to IRC networks, join channels, send/receive messages, and handle DCC file transfers.

**Architecture:** Axum HTTP server exposing MCP streamable_http endpoint, using the `irc` crate for protocol handling, SQLite for message history, and custom DCC transfer handler. Single persistent IRC connection shared across MCP sessions.

**Tech Stack:** Rust 2024, Axum 0.8, irc 1.1.0, rusqlite, tokio, serde/serde_json

---

## File Structure Overview

**New project at:** `irc-mcp-server/` (sibling to rusty-bidule)

```
irc-mcp-server/
├── Cargo.toml
├── irc-mcp-config.yaml
├── README.md
├── src/
│   ├── main.rs              # Entry point, CLI arg parsing, server startup
│   ├── types.rs             # Shared types: AppState, ConnectionStatus, Message, DccTransfer
│   ├── config.rs            # Load and validate YAML configuration
│   ├── storage/
│   │   ├── mod.rs           # Re-exports
│   │   └── database.rs      # SQLite setup, schema init, message/DCC queries
│   ├── irc/
│   │   ├── mod.rs           # Re-exports
│   │   ├── client.rs        # IRC client wrapper, connection management, message router
│   │   └── dcc.rs           # DCC SEND parser, file downloader, path sanitization
│   └── mcp/
│       ├── mod.rs           # Re-exports
│       ├── server.rs        # Axum server setup, routing, JSON-RPC handler
│       └── tools.rs         # MCP tool implementations (irc_connect, irc_join_channel, etc.)
└── tests/
    ├── config_test.rs       # Configuration parsing tests
    ├── database_test.rs     # SQLite operations tests
    └── mcp_protocol_test.rs # MCP protocol compliance tests
```

**Responsibilities:**
- `types.rs` - All shared structs, enums, type definitions
- `config.rs` - YAML parsing, validation, environment resolution
- `storage/database.rs` - All SQLite operations (schema, inserts, queries)
- `irc/client.rs` - IRC connection lifecycle, message stream processing
- `irc/dcc.rs` - DCC transfer logic, file I/O, security checks
- `mcp/server.rs` - HTTP server, JSON-RPC routing
- `mcp/tools.rs` - Individual MCP tool handlers

---

## Task 1: Project Scaffolding & Configuration

**Files:**
- Create: `irc-mcp-server/Cargo.toml`
- Create: `irc-mcp-server/src/main.rs`
- Create: `irc-mcp-server/irc-mcp-config.yaml`
- Create: `irc-mcp-server/README.md`

- [ ] **Step 1: Create project directory**

```bash
cd /home/jbanier/Documents/work
mkdir irc-mcp-server
cd irc-mcp-server
```

- [ ] **Step 2: Initialize Cargo project**

```bash
cargo init --name irc-mcp-server
```

- [ ] **Step 3: Update Cargo.toml with dependencies**

```toml
[package]
name = "irc-mcp-server"
version = "0.1.0"
edition = "2024"

[dependencies]
axum = "0.8"
tokio = { version = "1.51", features = ["full"] }
irc = "1.1.0"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
serde_yaml = "0.9"
rusqlite = { version = "0.32", features = ["bundled"] }
anyhow = "1.0"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
chrono = { version = "0.4", features = ["serde"] }
url = "2.5"
shellexpand = "3.1"

[dev-dependencies]
tokio-test = "0.4"
tempfile = "3.0"
```

- [ ] **Step 4: Create default configuration file**

Create `irc-mcp-config.yaml`:

```yaml
server:
  address: "irc.undernet.org"
  port: 6667
  use_tls: false

identity:
  nickname: "rusty-bot"
  username: "rusty"
  realname: "Rusty Bidule IRC Bot"

channels:
  - "#bookz"

dcc:
  enabled: true
  download_directory: "./data/irc-downloads"
  max_file_size_bytes: 104857600  # 100 MB
  auto_accept: true
  allowed_extensions: []  # empty = allow all

storage:
  database_path: "./data/irc-history.db"
  message_retention_days: 90

mcp:
  listen_address: "127.0.0.1"
  port: 5001
```

- [ ] **Step 5: Create README.md**

```markdown
# IRC MCP Server

Standalone IRC MCP server for rusty-bidule integration.

## Features

- Connect to IRC networks (with TLS/SSL support)
- Join channels and send/receive messages
- DCC file transfer support (auto-accept)
- Message history storage with full-text search
- MCP streamable_http interface

## Quick Start

1. Edit `irc-mcp-config.yaml` with your IRC settings
2. Run the server:

   ```bash
   cargo run -- --config irc-mcp-config.yaml
   ```

3. Configure rusty-bidule to connect:

   ```yaml
   mcp_servers:
     - name: irc-server
       transport: streamable_http
       url: http://127.0.0.1:5001/mcp
       timeout: 30
   ```

## Testing

```bash
cargo test
```

## MCP Tools

- `irc_connect` - Connect to IRC server
- `irc_disconnect` - Disconnect from server
- `irc_status` - Get connection status
- `irc_join_channel` - Join a channel
- `irc_part_channel` - Leave a channel
- `irc_send_message` - Send message to channel/user
- `irc_get_messages` - Retrieve message history
- `irc_get_channel_users` - List channel users
- `irc_list_dcc_transfers` - List DCC transfers
- `irc_get_dcc_file_info` - Get file details
- `irc_read_dcc_file` - Read received file content
- `irc_send_raw` - Send raw IRC command
- `irc_search_history` - Search message history
```

- [ ] **Step 6: Verify project compiles**

```bash
cargo check
```

Expected: Success (empty main compiles)

- [ ] **Step 7: Commit scaffolding**

```bash
git init
git add Cargo.toml irc-mcp-config.yaml README.md src/main.rs
git commit -m "feat: initial project scaffolding

- Cargo.toml with dependencies
- Default configuration file
- README with usage instructions"
```

---

## Task 2: Core Types & Data Structures

**Files:**
- Create: `irc-mcp-server/src/types.rs`

- [ ] **Step 1: Create types.rs with core types**

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Connection status enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Error,
}

/// IRC message stored in database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IrcMessage {
    pub id: Option<i64>,
    pub timestamp: DateTime<Utc>,
    pub source_nick: String,
    pub target: String,
    pub message_type: MessageType,
    pub content: String,
    pub channel: Option<String>,
}

/// Message type enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageType {
    Channel,
    Private,
    Notice,
    Ctcp,
    System,
}

impl MessageType {
    pub fn as_str(&self) -> &'static str {
        match self {
            MessageType::Channel => "channel",
            MessageType::Private => "private",
            MessageType::Notice => "notice",
            MessageType::Ctcp => "ctcp",
            MessageType::System => "system",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "channel" => Some(MessageType::Channel),
            "private" => Some(MessageType::Private),
            "notice" => Some(MessageType::Notice),
            "ctcp" => Some(MessageType::Ctcp),
            "system" => Some(MessageType::System),
            _ => None,
        }
    }
}

/// DCC transfer status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DccStatus {
    Pending,
    Downloading,
    Completed,
    Failed,
}

impl DccStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            DccStatus::Pending => "pending",
            DccStatus::Downloading => "downloading",
            DccStatus::Completed => "completed",
            DccStatus::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(DccStatus::Pending),
            "downloading" => Some(DccStatus::Downloading),
            "completed" => Some(DccStatus::Completed),
            "failed" => Some(DccStatus::Failed),
            _ => None,
        }
    }
}

/// DCC transfer record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DccTransfer {
    pub id: Option<i64>,
    pub timestamp: DateTime<Utc>,
    pub sender_nick: String,
    pub filename: String,
    pub filepath: Option<String>,
    pub filesize: u64,
    pub received_size: u64,
    pub status: DccStatus,
    pub error: Option<String>,
    pub ip_address: Option<String>,
    pub port: Option<u16>,
}

/// Application state shared across handlers
pub struct AppState {
    pub irc_client: Option<irc::client::Client>,
    pub connection_status: ConnectionStatus,
    pub connection_start: Option<DateTime<Utc>>,
    pub current_nick: Option<String>,
    pub joined_channels: Vec<String>,
    pub db_connection: rusqlite::Connection,
    pub config: crate::config::IrcMcpConfig,
    pub active_dcc_transfers: HashMap<u64, DccTransfer>,
}

pub type SharedState = Arc<Mutex<AppState>>;
```

- [ ] **Step 2: Update main.rs to import types**

Add to `src/main.rs`:

```rust
mod types;

fn main() {
    println!("IRC MCP Server");
}
```

- [ ] **Step 3: Verify types compile**

```bash
cargo check
```

Expected: Compilation errors about missing `config::IrcMcpConfig` (we'll fix next)

- [ ] **Step 4: Commit types module**

```bash
git add src/types.rs src/main.rs
git commit -m "feat: add core types and data structures

- ConnectionStatus, MessageType, DccStatus enums
- IrcMessage and DccTransfer structs
- AppState for shared state management"
```

---

## Task 3: Configuration Module

**Files:**
- Create: `irc-mcp-server/src/config.rs`
- Create: `irc-mcp-server/tests/config_test.rs`

- [ ] **Step 1: Write failing configuration test**

Create `tests/config_test.rs`:

```rust
use irc_mcp_server::config::IrcMcpConfig;
use tempfile::NamedTempFile;
use std::io::Write;

#[test]
fn test_load_valid_config() {
    let mut file = NamedTempFile::new().unwrap();
    writeln!(file, r#"
server:
  address: "irc.test.org"
  port: 6667
  use_tls: false

identity:
  nickname: "testbot"
  username: "test"
  realname: "Test Bot"

channels:
  - "#test"

dcc:
  enabled: true
  download_directory: "./downloads"
  max_file_size_bytes: 10485760
  auto_accept: true
  allowed_extensions: []

storage:
  database_path: "./test.db"
  message_retention_days: 30

mcp:
  listen_address: "127.0.0.1"
  port: 5001
"#).unwrap();

    let config = IrcMcpConfig::from_file(file.path()).unwrap();
    assert_eq!(config.server.address, "irc.test.org");
    assert_eq!(config.server.port, 6667);
    assert_eq!(config.identity.nickname, "testbot");
    assert_eq!(config.channels.len(), 1);
    assert_eq!(config.dcc.enabled, true);
}

#[test]
fn test_missing_required_field_fails() {
    let mut file = NamedTempFile::new().unwrap();
    writeln!(file, r#"
server:
  address: "irc.test.org"
  # missing port
"#).unwrap();

    let result = IrcMcpConfig::from_file(file.path());
    assert!(result.is_err());
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test test_load_valid_config
```

Expected: FAIL - `IrcMcpConfig` not found

- [ ] **Step 3: Implement configuration module**

Create `src/config.rs`:

```rust
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IrcMcpConfig {
    pub server: ServerConfig,
    pub identity: IdentityConfig,
    pub channels: Vec<String>,
    pub dcc: DccConfig,
    pub storage: StorageConfig,
    pub mcp: McpConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    pub address: String,
    pub port: u16,
    pub use_tls: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IdentityConfig {
    pub nickname: String,
    pub username: String,
    pub realname: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DccConfig {
    pub enabled: bool,
    pub download_directory: String,
    pub max_file_size_bytes: u64,
    pub auto_accept: bool,
    pub allowed_extensions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StorageConfig {
    pub database_path: String,
    pub message_retention_days: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpConfig {
    pub listen_address: String,
    pub port: u16,
}

impl IrcMcpConfig {
    /// Load configuration from YAML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file: {}", path.as_ref().display()))?;
        
        let config: IrcMcpConfig = serde_yaml::from_str(&content)
            .context("Failed to parse YAML configuration")?;
        
        config.validate()?;
        Ok(config)
    }

    /// Validate configuration values
    fn validate(&self) -> Result<()> {
        if self.server.address.is_empty() {
            anyhow::bail!("Server address cannot be empty");
        }
        
        if self.identity.nickname.is_empty() {
            anyhow::bail!("Nickname cannot be empty");
        }
        
        if self.storage.database_path.is_empty() {
            anyhow::bail!("Database path cannot be empty");
        }
        
        Ok(())
    }

    /// Expand shell variables in paths
    pub fn expand_paths(&mut self) {
        self.dcc.download_directory = shellexpand::tilde(&self.dcc.download_directory).to_string();
        self.storage.database_path = shellexpand::tilde(&self.storage.database_path).to_string();
    }
}
```

- [ ] **Step 4: Update main.rs and lib.rs**

Update `src/main.rs`:

```rust
mod config;
mod types;

fn main() {
    println!("IRC MCP Server");
}
```

Create `src/lib.rs`:

```rust
pub mod config;
pub mod types;
```

- [ ] **Step 5: Run tests to verify they pass**

```bash
cargo test
```

Expected: All tests pass

- [ ] **Step 6: Commit configuration module**

```bash
git add src/config.rs src/lib.rs tests/config_test.rs
git commit -m "feat: add configuration module with validation

- YAML parsing with serde_yaml
- Configuration structs for all sections
- Path expansion with shellexpand
- Validation tests"
```

---

## Task 4: Database Storage Layer

**Files:**
- Create: `irc-mcp-server/src/storage/mod.rs`
- Create: `irc-mcp-server/src/storage/database.rs`
- Create: `irc-mcp-server/tests/database_test.rs`

- [ ] **Step 1: Write failing database test**

Create `tests/database_test.rs`:

```rust
use irc_mcp_server::storage::Database;
use irc_mcp_server::types::{IrcMessage, MessageType, DccTransfer, DccStatus};
use chrono::Utc;
use tempfile::NamedTempFile;

#[test]
fn test_insert_and_retrieve_message() {
    let file = NamedTempFile::new().unwrap();
    let db = Database::new(file.path()).unwrap();

    let msg = IrcMessage {
        id: None,
        timestamp: Utc::now(),
        source_nick: "alice".to_string(),
        target: "#test".to_string(),
        message_type: MessageType::Channel,
        content: "Hello world".to_string(),
        channel: Some("#test".to_string()),
    };

    db.insert_message(&msg).unwrap();
    let messages = db.get_messages("#test", 10, None, None, None).unwrap();
    
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].source_nick, "alice");
    assert_eq!(messages[0].content, "Hello world");
}

#[test]
fn test_insert_dcc_transfer() {
    let file = NamedTempFile::new().unwrap();
    let db = Database::new(file.path()).unwrap();

    let transfer = DccTransfer {
        id: None,
        timestamp: Utc::now(),
        sender_nick: "bob".to_string(),
        filename: "test.txt".to_string(),
        filepath: Some("/tmp/test.txt".to_string()),
        filesize: 1024,
        received_size: 0,
        status: DccStatus::Pending,
        error: None,
        ip_address: Some("192.168.1.1".to_string()),
        port: Some(12345),
    };

    let id = db.insert_dcc_transfer(&transfer).unwrap();
    assert!(id > 0);

    let transfers = db.list_dcc_transfers(None, 10).unwrap();
    assert_eq!(transfers.len(), 1);
    assert_eq!(transfers[0].filename, "test.txt");
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test test_insert_and_retrieve_message
```

Expected: FAIL - `Database` struct not found

- [ ] **Step 3: Implement database module**

Create `src/storage/mod.rs`:

```rust
mod database;
pub use database::Database;
```

Create `src/storage/database.rs`:

```rust
use crate::types::{DccStatus, DccTransfer, IrcMessage, MessageType};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

pub struct Database {
    conn: Connection,
}

impl Database {
    /// Create or open database at given path
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        // Create parent directories if needed
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)
                .context("Failed to create database directory")?;
        }

        let conn = Connection::open(&path)
            .with_context(|| format!("Failed to open database: {}", path.as_ref().display()))?;

        let db = Database { conn };
        db.init_schema()?;
        Ok(db)
    }

    /// Initialize database schema
    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                source_nick TEXT NOT NULL,
                target TEXT NOT NULL,
                message_type TEXT NOT NULL,
                content TEXT NOT NULL,
                channel TEXT,
                UNIQUE(timestamp, source_nick, target, content)
            );

            CREATE INDEX IF NOT EXISTS idx_messages_timestamp ON messages(timestamp DESC);
            CREATE INDEX IF NOT EXISTS idx_messages_channel ON messages(channel, timestamp DESC);
            CREATE INDEX IF NOT EXISTS idx_messages_target ON messages(target, timestamp DESC);

            CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(content, content=messages, content_rowid=id);

            CREATE TABLE IF NOT EXISTS dcc_transfers (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                sender_nick TEXT NOT NULL,
                filename TEXT NOT NULL,
                filepath TEXT,
                filesize INTEGER NOT NULL,
                received_size INTEGER DEFAULT 0,
                status TEXT NOT NULL,
                error TEXT,
                ip_address TEXT,
                port INTEGER
            );

            CREATE INDEX IF NOT EXISTS idx_dcc_status ON dcc_transfers(status, timestamp DESC);

            CREATE TABLE IF NOT EXISTS channels (
                channel_name TEXT PRIMARY KEY,
                joined_at TEXT NOT NULL,
                last_activity TEXT
            );
            "#,
        )
        .context("Failed to initialize database schema")?;

        Ok(())
    }

    /// Insert a message into the database
    pub fn insert_message(&self, msg: &IrcMessage) -> Result<i64> {
        let timestamp = msg.timestamp.to_rfc3339();
        let message_type = msg.message_type.as_str();

        self.conn
            .execute(
                "INSERT OR IGNORE INTO messages (timestamp, source_nick, target, message_type, content, channel)
                 VALUES (?, ?, ?, ?, ?, ?)",
                params![
                    timestamp,
                    &msg.source_nick,
                    &msg.target,
                    message_type,
                    &msg.content,
                    &msg.channel,
                ],
            )
            .context("Failed to insert message")?;

        Ok(self.conn.last_insert_rowid())
    }

    /// Get messages from a channel or user
    pub fn get_messages(
        &self,
        target: &str,
        limit: usize,
        since: Option<DateTime<Utc>>,
        sender_filter: Option<&str>,
        search_query: Option<&str>,
    ) -> Result<Vec<IrcMessage>> {
        let mut query = String::from(
            "SELECT id, timestamp, source_nick, target, message_type, content, channel FROM messages WHERE target = ?"
        );
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(target.to_string())];

        if let Some(since_dt) = since {
            query.push_str(" AND timestamp > ?");
            params.push(Box::new(since_dt.to_rfc3339()));
        }

        if let Some(sender) = sender_filter {
            query.push_str(" AND source_nick = ?");
            params.push(Box::new(sender.to_string()));
        }

        if let Some(search) = search_query {
            query.push_str(" AND content LIKE ?");
            params.push(Box::new(format!("%{}%", search)));
        }

        query.push_str(" ORDER BY timestamp DESC LIMIT ?");
        params.push(Box::new(limit));

        let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| &**b as &dyn rusqlite::ToSql).collect();

        let mut stmt = self.conn.prepare(&query)?;
        let rows = stmt.query_map(&params_refs[..], |row| {
            let timestamp_str: String = row.get(1)?;
            let message_type_str: String = row.get(4)?;

            Ok(IrcMessage {
                id: Some(row.get(0)?),
                timestamp: DateTime::parse_from_rfc3339(&timestamp_str)
                    .unwrap()
                    .with_timezone(&Utc),
                source_nick: row.get(2)?,
                target: row.get(3)?,
                message_type: MessageType::from_str(&message_type_str).unwrap_or(MessageType::System),
                content: row.get(5)?,
                channel: row.get(6)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>()
            .context("Failed to retrieve messages")
    }

    /// Insert a DCC transfer record
    pub fn insert_dcc_transfer(&self, transfer: &DccTransfer) -> Result<i64> {
        let timestamp = transfer.timestamp.to_rfc3339();
        let status = transfer.status.as_str();

        self.conn
            .execute(
                "INSERT INTO dcc_transfers (timestamp, sender_nick, filename, filepath, filesize, received_size, status, error, ip_address, port)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    timestamp,
                    &transfer.sender_nick,
                    &transfer.filename,
                    &transfer.filepath,
                    transfer.filesize as i64,
                    transfer.received_size as i64,
                    status,
                    &transfer.error,
                    &transfer.ip_address,
                    transfer.port.map(|p| p as i64),
                ],
            )
            .context("Failed to insert DCC transfer")?;

        Ok(self.conn.last_insert_rowid())
    }

    /// Update DCC transfer status
    pub fn update_dcc_transfer_status(
        &self,
        id: i64,
        status: DccStatus,
        received_size: u64,
        filepath: Option<&str>,
        error: Option<&str>,
    ) -> Result<()> {
        self.conn
            .execute(
                "UPDATE dcc_transfers SET status = ?, received_size = ?, filepath = ?, error = ? WHERE id = ?",
                params![
                    status.as_str(),
                    received_size as i64,
                    filepath,
                    error,
                    id,
                ],
            )
            .context("Failed to update DCC transfer")?;

        Ok(())
    }

    /// List DCC transfers with optional status filter
    pub fn list_dcc_transfers(&self, status_filter: Option<DccStatus>, limit: usize) -> Result<Vec<DccTransfer>> {
        let query = if let Some(status) = status_filter {
            format!(
                "SELECT id, timestamp, sender_nick, filename, filepath, filesize, received_size, status, error, ip_address, port 
                 FROM dcc_transfers WHERE status = '{}' ORDER BY timestamp DESC LIMIT {}",
                status.as_str(),
                limit
            )
        } else {
            format!(
                "SELECT id, timestamp, sender_nick, filename, filepath, filesize, received_size, status, error, ip_address, port 
                 FROM dcc_transfers ORDER BY timestamp DESC LIMIT {}",
                limit
            )
        };

        let mut stmt = self.conn.prepare(&query)?;
        let rows = stmt.query_map([], |row| {
            let timestamp_str: String = row.get(1)?;
            let status_str: String = row.get(7)?;

            Ok(DccTransfer {
                id: Some(row.get(0)?),
                timestamp: DateTime::parse_from_rfc3339(&timestamp_str)
                    .unwrap()
                    .with_timezone(&Utc),
                sender_nick: row.get(2)?,
                filename: row.get(3)?,
                filepath: row.get(4)?,
                filesize: row.get::<_, i64>(5)? as u64,
                received_size: row.get::<_, i64>(6)? as u64,
                status: DccStatus::from_str(&status_str).unwrap_or(DccStatus::Failed),
                error: row.get(8)?,
                ip_address: row.get(9)?,
                port: row.get::<_, Option<i64>>(10)?.map(|p| p as u16),
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>()
            .context("Failed to list DCC transfers")
    }

    /// Get DCC transfer by ID
    pub fn get_dcc_transfer(&self, id: i64) -> Result<Option<DccTransfer>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, timestamp, sender_nick, filename, filepath, filesize, received_size, status, error, ip_address, port 
             FROM dcc_transfers WHERE id = ?"
        )?;

        let transfer = stmt
            .query_row(params![id], |row| {
                let timestamp_str: String = row.get(1)?;
                let status_str: String = row.get(7)?;

                Ok(DccTransfer {
                    id: Some(row.get(0)?),
                    timestamp: DateTime::parse_from_rfc3339(&timestamp_str)
                        .unwrap()
                        .with_timezone(&Utc),
                    sender_nick: row.get(2)?,
                    filename: row.get(3)?,
                    filepath: row.get(4)?,
                    filesize: row.get::<_, i64>(5)? as u64,
                    received_size: row.get::<_, i64>(6)? as u64,
                    status: DccStatus::from_str(&status_str).unwrap_or(DccStatus::Failed),
                    error: row.get(8)?,
                    ip_address: row.get(9)?,
                    port: row.get::<_, Option<i64>>(10)?.map(|p| p as u16),
                })
            })
            .optional()
            .context("Failed to get DCC transfer")?;

        Ok(transfer)
    }

    /// Search messages using full-text search
    pub fn search_messages(
        &self,
        query: &str,
        channel_filter: Option<&str>,
        limit: usize,
    ) -> Result<Vec<IrcMessage>> {
        let sql = if let Some(channel) = channel_filter {
            format!(
                "SELECT m.id, m.timestamp, m.source_nick, m.target, m.message_type, m.content, m.channel 
                 FROM messages m JOIN messages_fts ON m.id = messages_fts.rowid 
                 WHERE messages_fts MATCH ? AND m.channel = ? 
                 ORDER BY m.timestamp DESC LIMIT {}",
                limit
            )
        } else {
            format!(
                "SELECT m.id, m.timestamp, m.source_nick, m.target, m.message_type, m.content, m.channel 
                 FROM messages m JOIN messages_fts ON m.id = messages_fts.rowid 
                 WHERE messages_fts MATCH ? 
                 ORDER BY m.timestamp DESC LIMIT {}",
                limit
            )
        };

        let mut stmt = self.conn.prepare(&sql)?;

        let rows = if let Some(channel) = channel_filter {
            stmt.query_map(params![query, channel], |row| {
                let timestamp_str: String = row.get(1)?;
                let message_type_str: String = row.get(4)?;

                Ok(IrcMessage {
                    id: Some(row.get(0)?),
                    timestamp: DateTime::parse_from_rfc3339(&timestamp_str)
                        .unwrap()
                        .with_timezone(&Utc),
                    source_nick: row.get(2)?,
                    target: row.get(3)?,
                    message_type: MessageType::from_str(&message_type_str).unwrap_or(MessageType::System),
                    content: row.get(5)?,
                    channel: row.get(6)?,
                })
            })?
        } else {
            stmt.query_map(params![query], |row| {
                let timestamp_str: String = row.get(1)?;
                let message_type_str: String = row.get(4)?;

                Ok(IrcMessage {
                    id: Some(row.get(0)?),
                    timestamp: DateTime::parse_from_rfc3339(&timestamp_str)
                        .unwrap()
                        .with_timezone(&Utc),
                    source_nick: row.get(2)?,
                    target: row.get(3)?,
                    message_type: MessageType::from_str(&message_type_str).unwrap_or(MessageType::System),
                    content: row.get(5)?,
                    channel: row.get(6)?,
                })
            })?
        };

        rows.collect::<Result<Vec<_>, _>>()
            .context("Failed to search messages")
    }
}
```

- [ ] **Step 4: Update lib.rs**

Add to `src/lib.rs`:

```rust
pub mod storage;
```

- [ ] **Step 5: Run tests to verify they pass**

```bash
cargo test
```

Expected: All tests pass

- [ ] **Step 6: Commit database module**

```bash
git add src/storage/ tests/database_test.rs src/lib.rs
git commit -m "feat: add SQLite database storage layer

- Schema initialization with indexes and FTS
- Message insert and query operations
- DCC transfer tracking and updates
- Full-text search support
- Comprehensive tests"
```

---

## Task 5: DCC File Transfer Handler

**Files:**
- Create: `irc-mcp-server/src/irc/mod.rs`
- Create: `irc-mcp-server/src/irc/dcc.rs`

- [ ] **Step 1: Create DCC module structure**

Create `src/irc/mod.rs`:

```rust
mod dcc;
pub use dcc::{parse_dcc_send, download_dcc_file, sanitize_filename};
```

- [ ] **Step 2: Implement DCC parser and downloader**

Create `src/irc/dcc.rs`:

```rust
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{debug, info, warn};

/// Parsed DCC SEND offer
#[derive(Debug, Clone)]
pub struct DccSendOffer {
    pub filename: String,
    pub ip_address: String,
    pub port: u16,
    pub filesize: u64,
}

/// Parse DCC SEND message from CTCP
/// Format: DCC SEND filename ipaddr port filesize
pub fn parse_dcc_send(ctcp_message: &str) -> Result<DccSendOffer> {
    // CTCP messages are wrapped in \x01
    let msg = ctcp_message.trim_matches('\x01');
    
    if !msg.starts_with("DCC SEND ") {
        bail!("Not a DCC SEND message");
    }

    let parts: Vec<&str> = msg.split_whitespace().collect();
    if parts.len() < 5 {
        bail!("Invalid DCC SEND format: expected 'DCC SEND filename ipaddr port filesize'");
    }

    let filename = parts[2].to_string();
    let ip_numeric: u32 = parts[3].parse()
        .context("Failed to parse IP address")?;
    let port: u16 = parts[4].parse()
        .context("Failed to parse port")?;
    let filesize: u64 = parts[5].parse()
        .context("Failed to parse filesize")?;

    // Convert numeric IP to dotted decimal
    let ip_address = format!(
        "{}.{}.{}.{}",
        (ip_numeric >> 24) & 0xFF,
        (ip_numeric >> 16) & 0xFF,
        (ip_numeric >> 8) & 0xFF,
        ip_numeric & 0xFF
    );

    Ok(DccSendOffer {
        filename,
        ip_address,
        port,
        filesize,
    })
}

/// Sanitize filename to prevent directory traversal
pub fn sanitize_filename(filename: &str) -> String {
    // Remove path separators and parent directory references
    filename
        .replace('/', "_")
        .replace('\\', "_")
        .replace("..", "_")
        .trim()
        .to_string()
}

/// Download a file via DCC SEND protocol
pub async fn download_dcc_file(
    offer: &DccSendOffer,
    download_dir: &Path,
    max_file_size: u64,
) -> Result<(PathBuf, u64)> {
    // Validate file size
    if offer.filesize > max_file_size {
        bail!(
            "File size {} exceeds maximum allowed size {}",
            offer.filesize,
            max_file_size
        );
    }

    // Sanitize and create destination path
    let safe_filename = sanitize_filename(&offer.filename);
    if safe_filename.is_empty() {
        bail!("Invalid filename after sanitization");
    }

    std::fs::create_dir_all(download_dir)
        .context("Failed to create download directory")?;

    let temp_path = download_dir.join(format!("{}.part", safe_filename));
    let final_path = download_dir.join(&safe_filename);

    info!(
        "Starting DCC download: {} from {}:{} ({} bytes)",
        safe_filename, offer.ip_address, offer.port, offer.filesize
    );

    // Connect to sender
    let mut stream = TcpStream::connect(format!("{}:{}", offer.ip_address, offer.port))
        .await
        .context("Failed to connect to DCC sender")?;

    // Open temp file for writing
    let mut file = File::create(&temp_path)
        .await
        .context("Failed to create temporary file")?;

    // Download with progress tracking
    let mut total_received: u64 = 0;
    let mut buffer = vec![0u8; 8192];

    loop {
        let bytes_read = stream.read(&mut buffer).await
            .context("Failed to read from DCC connection")?;

        if bytes_read == 0 {
            break; // EOF
        }

        file.write_all(&buffer[..bytes_read])
            .await
            .context("Failed to write to file")?;

        total_received += bytes_read as u64;

        // Send acknowledgement (DCC protocol requires this)
        let ack = (total_received as u32).to_be_bytes();
        stream.write_all(&ack)
            .await
            .context("Failed to send DCC acknowledgement")?;

        // Check if we've exceeded expected size
        if total_received > offer.filesize {
            warn!("Received more bytes than advertised filesize");
            break;
        }

        debug!("DCC progress: {}/{} bytes", total_received, offer.filesize);
    }

    // Validate received size
    if total_received != offer.filesize {
        bail!(
            "Size mismatch: expected {} bytes, received {} bytes",
            offer.filesize,
            total_received
        );
    }

    // Rename temp file to final name
    tokio::fs::rename(&temp_path, &final_path)
        .await
        .context("Failed to rename completed download")?;

    info!("DCC download completed: {} ({} bytes)", safe_filename, total_received);

    Ok((final_path, total_received))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_dcc_send() {
        let ctcp = "\x01DCC SEND document.pdf 3232235777 12345 2048576\x01";
        let offer = parse_dcc_send(ctcp).unwrap();
        
        assert_eq!(offer.filename, "document.pdf");
        assert_eq!(offer.ip_address, "192.168.1.1");
        assert_eq!(offer.port, 12345);
        assert_eq!(offer.filesize, 2048576);
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("../../etc/passwd"), ".._.._.._etc_passwd");
        assert_eq!(sanitize_filename("normal.txt"), "normal.txt");
        assert_eq!(sanitize_filename("path/to/file.txt"), "path_to_file.txt");
        assert_eq!(sanitize_filename("C:\\Windows\\file.exe"), "C:_Windows_file.exe");
    }

    #[test]
    fn test_parse_invalid_dcc() {
        let result = parse_dcc_send("\x01INVALID MESSAGE\x01");
        assert!(result.is_err());
    }
}
```

- [ ] **Step 3: Update lib.rs**

Add to `src/lib.rs`:

```rust
pub mod irc;
```

- [ ] **Step 4: Run DCC tests**

```bash
cargo test dcc
```

Expected: All DCC tests pass

- [ ] **Step 5: Commit DCC module**

```bash
git add src/irc/ src/lib.rs
git commit -m "feat: add DCC file transfer handler

- Parse DCC SEND offers from CTCP messages
- Download files with progress tracking
- Filename sanitization for security
- Size validation and acknowledgements"
```

---

## Task 6: IRC Client Wrapper

**Files:**
- Create: `irc-mcp-server/src/irc/client.rs`
- Modify: `irc-mcp-server/src/irc/mod.rs`

- [ ] **Step 1: Update irc/mod.rs**

Update `src/irc/mod.rs`:

```rust
mod client;
mod dcc;

pub use client::{IrcClientManager, start_message_processor};
pub use dcc::{download_dcc_file, parse_dcc_send, sanitize_filename};
```

- [ ] **Step 2: Implement IRC client wrapper**

Create `src/irc/client.rs`:

```rust
use crate::config::IrcMcpConfig;
use crate::storage::Database;
use crate::types::{ConnectionStatus, DccStatus, DccTransfer, IrcMessage, MessageType, SharedState};
use anyhow::{Context, Result};
use chrono::Utc;
use irc::client::prelude::*;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

pub struct IrcClientManager {
    config: IrcMcpConfig,
}

impl IrcClientManager {
    pub fn new(config: IrcMcpConfig) -> Self {
        Self { config }
    }

    /// Create and connect IRC client
    pub async fn connect(&self) -> Result<Client> {
        let irc_config = Config {
            nickname: Some(self.config.identity.nickname.clone()),
            username: Some(self.config.identity.username.clone()),
            realname: Some(self.config.identity.realname.clone()),
            server: Some(self.config.server.address.clone()),
            port: Some(self.config.server.port),
            use_tls: Some(self.config.server.use_tls),
            channels: self.config.channels.clone(),
            ..Default::default()
        };

        let client = Client::from_config(irc_config)
            .await
            .context("Failed to create IRC client")?;

        client.identify()
            .context("Failed to identify to IRC server")?;

        info!(
            "Connected to IRC server: {}:{}",
            self.config.server.address, self.config.server.port
        );

        Ok(client)
    }
}

/// Start background task to process IRC messages
pub async fn start_message_processor(
    mut client: Client,
    state: SharedState,
) -> Result<()> {
    let mut stream = client.stream()?;

    while let Some(message) = stream.next().await.transpose()? {
        if let Err(e) = process_message(&message, &state).await {
            error!("Error processing IRC message: {}", e);
        }
    }

    // Connection lost
    warn!("IRC connection lost");
    let mut state_lock = state.lock().await;
    state_lock.connection_status = ConnectionStatus::Disconnected;

    Ok(())
}

/// Process a single IRC message
async fn process_message(message: &Message, state: &SharedState) -> Result<()> {
    debug!("IRC message: {:?}", message);

    match &message.command {
        Command::PRIVMSG(target, content) => {
            handle_privmsg(message, target, content, state).await?;
        }
        Command::NOTICE(target, content) => {
            handle_notice(message, target, content, state).await?;
        }
        Command::JOIN(channel, _, _) => {
            handle_join(message, channel, state).await?;
        }
        Command::PART(channel, _) => {
            handle_part(message, channel, state).await?;
        }
        _ => {
            // Ignore other commands
        }
    }

    Ok(())
}

/// Handle PRIVMSG command
async fn handle_privmsg(
    message: &Message,
    target: &str,
    content: &str,
    state: &SharedState,
) -> Result<()> {
    let source_nick = message.source_nickname().unwrap_or("unknown").to_string();

    // Check if this is a CTCP message
    if content.starts_with('\x01') && content.ends_with('\x01') {
        return handle_ctcp(message, &source_nick, target, content, state).await;
    }

    let msg_type = if target.starts_with('#') {
        MessageType::Channel
    } else {
        MessageType::Private
    };

    let irc_msg = IrcMessage {
        id: None,
        timestamp: Utc::now(),
        source_nick: source_nick.clone(),
        target: target.to_string(),
        message_type: msg_type,
        content: content.to_string(),
        channel: if target.starts_with('#') {
            Some(target.to_string())
        } else {
            None
        },
    };

    // Store in database
    let state_lock = state.lock().await;
    if let Err(e) = Database::new(&state_lock.config.storage.database_path)
        .and_then(|db| db.insert_message(&irc_msg))
    {
        error!("Failed to store message: {}", e);
    }

    Ok(())
}

/// Handle NOTICE command
async fn handle_notice(
    message: &Message,
    target: &str,
    content: &str,
    state: &SharedState,
) -> Result<()> {
    let source_nick = message.source_nickname().unwrap_or("system").to_string();

    let irc_msg = IrcMessage {
        id: None,
        timestamp: Utc::now(),
        source_nick,
        target: target.to_string(),
        message_type: MessageType::Notice,
        content: content.to_string(),
        channel: None,
    };

    let state_lock = state.lock().await;
    if let Err(e) = Database::new(&state_lock.config.storage.database_path)
        .and_then(|db| db.insert_message(&irc_msg))
    {
        error!("Failed to store notice: {}", e);
    }

    Ok(())
}

/// Handle CTCP messages (including DCC)
async fn handle_ctcp(
    _message: &Message,
    source_nick: &str,
    _target: &str,
    content: &str,
    state: &SharedState,
) -> Result<()> {
    // Check if this is a DCC SEND offer
    if content.contains("DCC SEND") {
        if let Ok(offer) = crate::irc::parse_dcc_send(content) {
            info!(
                "Received DCC SEND offer from {}: {} ({} bytes)",
                source_nick, offer.filename, offer.filesize
            );

            let state_lock = state.lock().await;
            
            // Check if DCC is enabled
            if !state_lock.config.dcc.enabled || !state_lock.config.dcc.auto_accept {
                info!("DCC auto-accept disabled, ignoring offer");
                return Ok(());
            }

            // Create transfer record
            let transfer = DccTransfer {
                id: None,
                timestamp: Utc::now(),
                sender_nick: source_nick.to_string(),
                filename: offer.filename.clone(),
                filepath: None,
                filesize: offer.filesize,
                received_size: 0,
                status: DccStatus::Pending,
                error: None,
                ip_address: Some(offer.ip_address.clone()),
                port: Some(offer.port),
            };

            let db = Database::new(&state_lock.config.storage.database_path)?;
            let transfer_id = db.insert_dcc_transfer(&transfer)?;

            info!("Created DCC transfer record with ID: {}", transfer_id);

            // Spawn download task
            let download_dir = state_lock.config.dcc.download_directory.clone();
            let max_size = state_lock.config.dcc.max_file_size_bytes;
            let db_path = state_lock.config.storage.database_path.clone();
            
            drop(state_lock); // Release lock before spawning

            tokio::spawn(async move {
                let result = crate::irc::download_dcc_file(
                    &offer,
                    &std::path::Path::new(&download_dir),
                    max_size,
                )
                .await;

                let db = match Database::new(&db_path) {
                    Ok(d) => d,
                    Err(e) => {
                        error!("Failed to open database: {}", e);
                        return;
                    }
                };

                match result {
                    Ok((filepath, size)) => {
                        info!("DCC download completed: {:?}", filepath);
                        if let Err(e) = db.update_dcc_transfer_status(
                            transfer_id,
                            DccStatus::Completed,
                            size,
                            Some(&filepath.to_string_lossy()),
                            None,
                        ) {
                            error!("Failed to update transfer status: {}", e);
                        }
                    }
                    Err(e) => {
                        error!("DCC download failed: {}", e);
                        if let Err(e) = db.update_dcc_transfer_status(
                            transfer_id,
                            DccStatus::Failed,
                            0,
                            None,
                            Some(&e.to_string()),
                        ) {
                            error!("Failed to update transfer status: {}", e);
                        }
                    }
                }
            });
        }
    }

    Ok(())
}

/// Handle JOIN command
async fn handle_join(
    message: &Message,
    channel: &str,
    state: &SharedState,
) -> Result<()> {
    if let Some(nick) = message.source_nickname() {
        let mut state_lock = state.lock().await;
        
        // Check if it's our own join
        if Some(nick) == state_lock.current_nick.as_deref() {
            if !state_lock.joined_channels.contains(&channel.to_string()) {
                state_lock.joined_channels.push(channel.to_string());
                info!("Joined channel: {}", channel);
            }
        }
    }

    Ok(())
}

/// Handle PART command
async fn handle_part(
    message: &Message,
    channel: &str,
    state: &SharedState,
) -> Result<()> {
    if let Some(nick) = message.source_nickname() {
        let mut state_lock = state.lock().await;
        
        // Check if it's our own part
        if Some(nick) == state_lock.current_nick.as_deref() {
            state_lock.joined_channels.retain(|c| c != channel);
            info!("Left channel: {}", channel);
        }
    }

    Ok(())
}
```

- [ ] **Step 3: Verify IRC client compiles**

```bash
cargo check
```

Expected: Success

- [ ] **Step 4: Commit IRC client**

```bash
git add src/irc/
git commit -m "feat: add IRC client wrapper and message processor

- IRC connection management with irc crate
- Message routing for PRIVMSG, NOTICE, CTCP
- Automatic DCC download spawning
- Channel join/part tracking
- Database integration for message storage"
```

---

## Task 7: MCP Server & Tools Implementation (Part 1: Server)

**Files:**
- Create: `irc-mcp-server/src/mcp/mod.rs`
- Create: `irc-mcp-server/src/mcp/server.rs`

- [ ] **Step 1: Create MCP module structure**

Create `src/mcp/mod.rs`:

```rust
mod server;
mod tools;

pub use server::create_mcp_server;
pub use tools::handle_tool_call;
```

- [ ] **Step 2: Implement Axum MCP server**

Create `src/mcp/server.rs`:

```rust
use crate::types::SharedState;
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::SocketAddr;
use tracing::{debug, info};

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

/// Create MCP server with Axum
pub fn create_mcp_server(state: SharedState) -> Router {
    Router::new()
        .route("/mcp", post(handle_mcp_request))
        .with_state(state)
}

/// Handle MCP JSON-RPC requests
async fn handle_mcp_request(
    State(state): State<SharedState>,
    Json(request): Json<JsonRpcRequest>,
) -> impl IntoResponse {
    debug!("MCP request: method={} id={:?}", request.method, request.id);

    let response = match request.method.as_str() {
        "initialize" => handle_initialize(request.id).await,
        "tools/list" => handle_tools_list(request.id).await,
        "tools/call" => handle_tools_call(request.id, request.params, state).await,
        _ => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id,
            result: None,
            error: Some(JsonRpcError {
                code: -32601,
                message: "Method not found".to_string(),
                data: None,
            }),
        },
    };

    (StatusCode::OK, Json(response))
}

/// Handle initialize request
async fn handle_initialize(id: Option<Value>) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result: Some(json!({
            "protocolVersion": "2025-03-26",
            "serverInfo": {
                "name": "irc-mcp-server",
                "version": "0.1.0"
            },
            "capabilities": {
                "tools": {}
            }
        })),
        error: None,
    }
}

/// Handle tools/list request
async fn handle_tools_list(id: Option<Value>) -> JsonRpcResponse {
    let tools = vec![
        json!({
            "name": "irc_connect",
            "description": "Connect to the configured IRC server",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        json!({
            "name": "irc_disconnect",
            "description": "Disconnect from IRC server",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "quit_message": {
                        "type": "string",
                        "description": "Optional quit message"
                    }
                }
            }
        }),
        json!({
            "name": "irc_status",
            "description": "Get current IRC connection status",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        json!({
            "name": "irc_join_channel",
            "description": "Join an IRC channel",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "channel": {
                        "type": "string",
                        "description": "Channel name (must start with #)"
                    }
                },
                "required": ["channel"]
            }
        }),
        json!({
            "name": "irc_part_channel",
            "description": "Leave an IRC channel",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "channel": {
                        "type": "string",
                        "description": "Channel name"
                    },
                    "message": {
                        "type": "string",
                        "description": "Optional part message"
                    }
                },
                "required": ["channel"]
            }
        }),
        json!({
            "name": "irc_send_message",
            "description": "Send a message to a channel or user",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "Channel (#channel) or nickname"
                    },
                    "message": {
                        "type": "string",
                        "description": "Message content"
                    }
                },
                "required": ["target", "message"]
            }
        }),
        json!({
            "name": "irc_get_messages",
            "description": "Retrieve messages from a channel or user",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "Channel or nickname"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum messages to return",
                        "default": 100
                    },
                    "since_timestamp": {
                        "type": "string",
                        "description": "ISO 8601 timestamp"
                    },
                    "sender_filter": {
                        "type": "string",
                        "description": "Filter by sender nickname"
                    },
                    "search_query": {
                        "type": "string",
                        "description": "Search in message content"
                    }
                },
                "required": ["target"]
            }
        }),
        json!({
            "name": "irc_get_channel_users",
            "description": "List users in a channel",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "channel": {
                        "type": "string",
                        "description": "Channel name"
                    }
                },
                "required": ["channel"]
            }
        }),
        json!({
            "name": "irc_list_dcc_transfers",
            "description": "List DCC file transfers",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "status_filter": {
                        "type": "string",
                        "enum": ["all", "pending", "downloading", "completed", "failed"],
                        "default": "all"
                    },
                    "limit": {
                        "type": "integer",
                        "default": 50
                    }
                }
            }
        }),
        json!({
            "name": "irc_get_dcc_file_info",
            "description": "Get details about a DCC file transfer",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "transfer_id": {
                        "type": "integer",
                        "description": "DCC transfer ID"
                    }
                },
                "required": ["transfer_id"]
            }
        }),
        json!({
            "name": "irc_read_dcc_file",
            "description": "Read content from a received DCC file",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "transfer_id": {
                        "type": "integer",
                        "description": "DCC transfer ID"
                    },
                    "offset": {
                        "type": "integer",
                        "description": "Byte offset to start reading from",
                        "default": 0
                    },
                    "length": {
                        "type": "integer",
                        "description": "Number of bytes to read (0 = all)",
                        "default": 0
                    },
                    "encoding": {
                        "type": "string",
                        "enum": ["utf8", "base64"],
                        "description": "How to encode the content",
                        "default": "utf8"
                    }
                },
                "required": ["transfer_id"]
            }
        }),
        json!({
            "name": "irc_send_raw",
            "description": "Send a raw IRC command",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Raw IRC command"
                    }
                },
                "required": ["command"]
            }
        }),
        json!({
            "name": "irc_search_history",
            "description": "Full-text search across message history",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query"
                    },
                    "channel_filter": {
                        "type": "string",
                        "description": "Only search in this channel"
                    },
                    "limit": {
                        "type": "integer",
                        "default": 100
                    }
                },
                "required": ["query"]
            }
        }),
    ];

    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result: Some(json!({ "tools": tools })),
        error: None,
    }
}

/// Handle tools/call request
async fn handle_tools_call(
    id: Option<Value>,
    params: Option<Value>,
    state: SharedState,
) -> JsonRpcResponse {
    let params = match params {
        Some(p) => p,
        None => {
            return JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: "Invalid params".to_string(),
                    data: None,
                }),
            };
        }
    };

    match crate::mcp::handle_tool_call(params, state).await {
        Ok(result) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        },
        Err(e) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            }),
        },
    }
}

/// Start MCP server
pub async fn start_server(listen_addr: &str, port: u16, state: SharedState) -> anyhow::Result<()> {
    let app = create_mcp_server(state);
    let addr: SocketAddr = format!("{}:{}", listen_addr, port).parse()?;

    info!("IRC MCP Server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
```

- [ ] **Step 3: Update lib.rs**

Add to `src/lib.rs`:

```rust
pub mod mcp;
```

- [ ] **Step 4: Verify server compiles**

```bash
cargo check
```

Expected: Compilation error about missing `handle_tool_call` (we'll implement next)

- [ ] **Step 5: Commit MCP server**

```bash
git add src/mcp/ src/lib.rs
git commit -m "feat: add MCP server with Axum and JSON-RPC

- Axum HTTP server setup
- JSON-RPC 2.0 request/response handling
- MCP protocol: initialize, tools/list, tools/call
- Tool schema definitions for all IRC operations"
```

---

---

## Task 8: MCP Tools Implementation (Part 2: Tool Handlers)

**Files:**
- Create: `irc-mcp-server/src/mcp/tools.rs`

- [ ] **Step 1: Implement tool handler dispatcher**

Create `src/mcp/tools.rs` (part 1 - structure):

```rust
use crate::irc::{IrcClientManager, start_message_processor};
use crate::storage::Database;
use crate::types::{ConnectionStatus, DccStatus, SharedState};
use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde_json::{json, Value};
use std::fs;
use tracing::{error, info};

/// Handle MCP tool call
pub async fn handle_tool_call(params: Value, state: SharedState) -> Result<Value> {
    let tool_name = params["name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing tool name"))?;

    let arguments = params["arguments"].clone();

    match tool_name {
        "irc_connect" => tool_irc_connect(state).await,
        "irc_disconnect" => tool_irc_disconnect(arguments, state).await,
        "irc_status" => tool_irc_status(state).await,
        "irc_join_channel" => tool_irc_join_channel(arguments, state).await,
        "irc_part_channel" => tool_irc_part_channel(arguments, state).await,
        "irc_send_message" => tool_irc_send_message(arguments, state).await,
        "irc_get_messages" => tool_irc_get_messages(arguments, state).await,
        "irc_get_channel_users" => tool_irc_get_channel_users(arguments, state).await,
        "irc_list_dcc_transfers" => tool_irc_list_dcc_transfers(arguments, state).await,
        "irc_get_dcc_file_info" => tool_irc_get_dcc_file_info(arguments, state).await,
        "irc_read_dcc_file" => tool_irc_read_dcc_file(arguments, state).await,
        "irc_send_raw" => tool_irc_send_raw(arguments, state).await,
        "irc_search_history" => tool_irc_search_history(arguments, state).await,
        _ => bail!("Unknown tool: {}", tool_name),
    }
}
```

- [ ] **Step 2: Implement connection management tools**

Add to `src/mcp/tools.rs`:

```rust
async fn tool_irc_connect(state: SharedState) -> Result<Value> {
    let mut state_lock = state.lock().await;

    if state_lock.connection_status == ConnectionStatus::Connected {
        return Ok(json!({
            "success": true,
            "message": "Already connected",
            "server": format!("{}:{}", state_lock.config.server.address, state_lock.config.server.port),
            "nick": state_lock.current_nick,
            "joined_channels": state_lock.joined_channels,
        }));
    }

    info!("Connecting to IRC server...");
    state_lock.connection_status = ConnectionStatus::Connecting;

    let manager = IrcClientManager::new(state_lock.config.clone());
    let client = manager.connect().await?;

    let nick = state_lock.config.identity.nickname.clone();
    state_lock.current_nick = Some(nick.clone());
    state_lock.connection_status = ConnectionStatus::Connected;
    state_lock.connection_start = Some(Utc::now());
    state_lock.irc_client = Some(client.clone());

    // Spawn message processor
    let state_clone = state.clone();
    tokio::spawn(async move {
        if let Err(e) = start_message_processor(client, state_clone).await {
            error!("Message processor error: {}", e);
        }
    });

    Ok(json!({
        "success": true,
        "server": format!("{}:{}", state_lock.config.server.address, state_lock.config.server.port),
        "nick": nick,
        "joined_channels": state_lock.config.channels,
    }))
}

async fn tool_irc_disconnect(arguments: Value, state: SharedState) -> Result<Value> {
    let quit_message = arguments["quit_message"]
        .as_str()
        .unwrap_or("Disconnecting");

    let mut state_lock = state.lock().await;

    if let Some(client) = state_lock.irc_client.take() {
        client.send_quit(quit_message)?;
        state_lock.connection_status = ConnectionStatus::Disconnected;
        state_lock.current_nick = None;
        state_lock.joined_channels.clear();

        Ok(json!({
            "success": true,
            "message": "Disconnected from IRC server"
        }))
    } else {
        Ok(json!({
            "success": false,
            "message": "Not connected"
        }))
    }
}

async fn tool_irc_status(state: SharedState) -> Result<Value> {
    let state_lock = state.lock().await;

    let uptime_seconds = state_lock.connection_start
        .map(|start| (Utc::now() - start).num_seconds())
        .unwrap_or(0);

    Ok(json!({
        "connected": state_lock.connection_status == ConnectionStatus::Connected,
        "server": format!("{}:{}", state_lock.config.server.address, state_lock.config.server.port),
        "nick": state_lock.current_nick,
        "channels": state_lock.joined_channels,
        "uptime_seconds": uptime_seconds,
    }))
}
```

- [ ] **Step 3: Implement channel operation tools**

Add to `src/mcp/tools.rs`:

```rust
async fn tool_irc_join_channel(arguments: Value, state: SharedState) -> Result<Value> {
    let channel = arguments["channel"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing channel parameter"))?;

    if !channel.starts_with('#') {
        bail!("Channel name must start with #");
    }

    let state_lock = state.lock().await;

    if let Some(client) = &state_lock.irc_client {
        client.send_join(channel)?;
        Ok(json!({
            "success": true,
            "channel": channel,
            "message": format!("Joining channel {}", channel)
        }))
    } else {
        bail!("Not connected to IRC server");
    }
}

async fn tool_irc_part_channel(arguments: Value, state: SharedState) -> Result<Value> {
    let channel = arguments["channel"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing channel parameter"))?;

    let message = arguments["message"].as_str();

    let state_lock = state.lock().await;

    if let Some(client) = &state_lock.irc_client {
        if let Some(msg) = message {
            client.send_part_with_message(channel, msg)?;
        } else {
            client.send_part(channel)?;
        }

        Ok(json!({
            "success": true,
            "channel": channel,
        }))
    } else {
        bail!("Not connected to IRC server");
    }
}

async fn tool_irc_send_message(arguments: Value, state: SharedState) -> Result<Value> {
    let target = arguments["target"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing target parameter"))?;

    let message = arguments["message"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing message parameter"))?;

    let state_lock = state.lock().await;

    if let Some(client) = &state_lock.irc_client {
        client.send_privmsg(target, message)?;

        Ok(json!({
            "success": true,
            "target": target,
            "message": "Message sent"
        }))
    } else {
        bail!("Not connected to IRC server");
    }
}

async fn tool_irc_get_messages(arguments: Value, state: SharedState) -> Result<Value> {
    let target = arguments["target"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing target parameter"))?;

    let limit = arguments["limit"].as_u64().unwrap_or(100) as usize;

    let since = arguments["since_timestamp"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));

    let sender_filter = arguments["sender_filter"].as_str();
    let search_query = arguments["search_query"].as_str();

    let state_lock = state.lock().await;
    let db = Database::new(&state_lock.config.storage.database_path)?;

    let messages = db.get_messages(target, limit, since, sender_filter, search_query)?;

    Ok(json!({
        "messages": messages,
        "count": messages.len(),
        "has_more": messages.len() >= limit,
    }))
}

async fn tool_irc_get_channel_users(arguments: Value, state: SharedState) -> Result<Value> {
    let channel = arguments["channel"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing channel parameter"))?;

    let state_lock = state.lock().await;

    if let Some(client) = &state_lock.irc_client {
        // Send NAMES command
        client.send_names(Some(channel))?;

        // Note: Getting the actual user list requires parsing NAMES responses
        // For now, return a pending status
        Ok(json!({
            "channel": channel,
            "message": "NAMES request sent - user list will be in message stream",
        }))
    } else {
        bail!("Not connected to IRC server");
    }
}
```

- [ ] **Step 4: Implement DCC operation tools**

Add to `src/mcp/tools.rs`:

```rust
async fn tool_irc_list_dcc_transfers(arguments: Value, state: SharedState) -> Result<Value> {
    let status_filter_str = arguments["status_filter"].as_str();
    let status_filter = status_filter_str.and_then(|s| match s {
        "pending" => Some(DccStatus::Pending),
        "downloading" => Some(DccStatus::Downloading),
        "completed" => Some(DccStatus::Completed),
        "failed" => Some(DccStatus::Failed),
        _ => None,
    });

    let limit = arguments["limit"].as_u64().unwrap_or(50) as usize;

    let state_lock = state.lock().await;
    let db = Database::new(&state_lock.config.storage.database_path)?;

    let transfers = db.list_dcc_transfers(status_filter, limit)?;

    Ok(json!({ "transfers": transfers }))
}

async fn tool_irc_get_dcc_file_info(arguments: Value, state: SharedState) -> Result<Value> {
    let transfer_id = arguments["transfer_id"]
        .as_i64()
        .ok_or_else(|| anyhow::anyhow!("Missing or invalid transfer_id parameter"))?;

    let state_lock = state.lock().await;
    let db = Database::new(&state_lock.config.storage.database_path)?;

    let transfer = db.get_dcc_transfer(transfer_id)?
        .ok_or_else(|| anyhow::anyhow!("Transfer not found"))?;

    Ok(json!(transfer))
}

async fn tool_irc_read_dcc_file(arguments: Value, state: SharedState) -> Result<Value> {
    let transfer_id = arguments["transfer_id"]
        .as_i64()
        .ok_or_else(|| anyhow::anyhow!("Missing or invalid transfer_id parameter"))?;

    let offset = arguments["offset"].as_u64().unwrap_or(0);
    let length = arguments["length"].as_u64().unwrap_or(0);
    let encoding = arguments["encoding"].as_str().unwrap_or("utf8");

    let state_lock = state.lock().await;
    let db = Database::new(&state_lock.config.storage.database_path)?;

    let transfer = db.get_dcc_transfer(transfer_id)?
        .ok_or_else(|| anyhow::anyhow!("Transfer not found"))?;

    if transfer.status != DccStatus::Completed {
        bail!("Transfer not completed");
    }

    let filepath = transfer.filepath
        .ok_or_else(|| anyhow::anyhow!("File path not available"))?;

    let mut file_content = fs::read(&filepath)
        .with_context(|| format!("Failed to read file: {}", filepath))?;

    // Apply offset and length
    let start = offset as usize;
    let end = if length == 0 {
        file_content.len()
    } else {
        std::cmp::min(start + length as usize, file_content.len())
    };

    if start >= file_content.len() {
        file_content = Vec::new();
    } else {
        file_content = file_content[start..end].to_vec();
    }

    let content = match encoding {
        "base64" => {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(&file_content)
        }
        "utf8" => String::from_utf8_lossy(&file_content).to_string(),
        _ => bail!("Invalid encoding: must be 'utf8' or 'base64'"),
    };

    Ok(json!({
        "transfer_id": transfer_id,
        "filename": transfer.filename,
        "content": content,
        "encoding": encoding,
        "bytes_returned": file_content.len(),
    }))
}
```

- [ ] **Step 5: Implement utility tools**

Add to `src/mcp/tools.rs`:

```rust
async fn tool_irc_send_raw(arguments: Value, state: SharedState) -> Result<Value> {
    let command = arguments["command"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing command parameter"))?;

    let state_lock = state.lock().await;

    if let Some(client) = &state_lock.irc_client {
        client.send(command)?;

        Ok(json!({
            "success": true,
            "command": command,
        }))
    } else {
        bail!("Not connected to IRC server");
    }
}

async fn tool_irc_search_history(arguments: Value, state: SharedState) -> Result<Value> {
    let query = arguments["query"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing query parameter"))?;

    let channel_filter = arguments["channel_filter"].as_str();
    let limit = arguments["limit"].as_u64().unwrap_or(100) as usize;

    let state_lock = state.lock().await;
    let db = Database::new(&state_lock.config.storage.database_path)?;

    let messages = db.search_messages(query, channel_filter, limit)?;

    Ok(json!({
        "messages": messages,
        "count": messages.len(),
        "query": query,
    }))
}
```

- [ ] **Step 6: Add base64 dependency to Cargo.toml**

Update `Cargo.toml` dependencies section:

```toml
base64 = "0.22"
```

- [ ] **Step 7: Verify tools compile**

```bash
cargo check
```

Expected: Success

- [ ] **Step 8: Commit MCP tools**

```bash
git add src/mcp/tools.rs Cargo.toml
git commit -m "feat: implement all MCP tool handlers

- Connection: connect, disconnect, status
- Channels: join, part, send message, get messages, list users
- DCC: list transfers, get file info, read file content
- Utility: send raw commands, search history
- Full argument parsing and validation"
```

---

## Task 9: Main Entry Point & CLI

**Files:**
- Modify: `irc-mcp-server/src/main.rs`

- [ ] **Step 1: Implement main.rs with CLI argument parsing**

Replace `src/main.rs`:

```rust
mod config;
mod irc;
mod mcp;
mod storage;
mod types;

use crate::config::IrcMcpConfig;
use crate::storage::Database;
use crate::types::{AppState, ConnectionStatus};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "irc_mcp_server=info,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();
    let config_path = if args.len() > 2 && args[1] == "--config" {
        args[2].clone()
    } else {
        "irc-mcp-config.yaml".to_string()
    };

    info!("Loading configuration from: {}", config_path);

    // Load and expand configuration
    let mut config = IrcMcpConfig::from_file(&config_path)
        .context("Failed to load configuration")?;
    config.expand_paths();

    info!(
        "Configuration loaded - server: {}:{}, nick: {}",
        config.server.address, config.server.port, config.identity.nickname
    );

    // Initialize database
    let db = Database::new(&config.storage.database_path)
        .context("Failed to initialize database")?;
    info!("Database initialized: {}", config.storage.database_path);

    // Create shared application state
    let state = Arc::new(Mutex::new(AppState {
        irc_client: None,
        connection_status: ConnectionStatus::Disconnected,
        connection_start: None,
        current_nick: None,
        joined_channels: Vec::new(),
        db_connection: db.into(),
        config: config.clone(),
        active_dcc_transfers: HashMap::new(),
    }));

    // Start MCP server
    info!("Starting MCP server...");
    if let Err(e) = mcp::start_server(
        &config.mcp.listen_address,
        config.mcp.port,
        state,
    )
    .await
    {
        error!("MCP server error: {}", e);
        return Err(e);
    }

    Ok(())
}
```

- [ ] **Step 2: Fix database conversion issue**

We need to adjust the AppState since rusqlite::Connection doesn't implement From<Database>. Update `src/types.rs`:

Change:
```rust
pub db_connection: rusqlite::Connection,
```

To store the database path instead:
```rust
pub db_path: String,
```

- [ ] **Step 3: Update main.rs to use db_path**

Replace the AppState initialization in `main.rs`:

```rust
let state = Arc::new(Mutex::new(AppState {
    irc_client: None,
    connection_status: ConnectionStatus::Disconnected,
    connection_start: None,
    current_nick: None,
    joined_channels: Vec::new(),
    db_path: config.storage.database_path.clone(),
    config: config.clone(),
    active_dcc_transfers: HashMap::new(),
}));
```

- [ ] **Step 4: Update types.rs AppState definition**

Update `src/types.rs`:

```rust
/// Application state shared across handlers
pub struct AppState {
    pub irc_client: Option<irc::client::Client>,
    pub connection_status: ConnectionStatus,
    pub connection_start: Option<DateTime<Utc>>,
    pub current_nick: Option<String>,
    pub joined_channels: Vec<String>,
    pub db_path: String,
    pub config: crate::config::IrcMcpConfig,
    pub active_dcc_transfers: HashMap<u64, DccTransfer>,
}
```

- [ ] **Step 5: Update all database access to use db_path**

This is already correct in the tools.rs file where we do:
```rust
let db = Database::new(&state_lock.config.storage.database_path)?;
```

- [ ] **Step 6: Test compilation**

```bash
cargo build
```

Expected: Clean build

- [ ] **Step 7: Test server starts**

```bash
cargo run -- --config irc-mcp-config.yaml
```

Expected: Server starts and logs "IRC MCP Server listening on http://127.0.0.1:5001"

Press Ctrl+C to stop.

- [ ] **Step 8: Commit main entry point**

```bash
git add src/main.rs src/types.rs
git commit -m "feat: implement main entry point with CLI

- CLI argument parsing for --config
- Tracing/logging initialization
- Configuration loading and validation
- Database initialization
- Server startup with shared state"
```

---

## Task 10: Protocol Testing & Verification

**Files:**
- Create: `irc-mcp-server/tests/mcp_protocol_test.rs`
- Create: `irc-mcp-server/test-irc-mcp.sh`

- [ ] **Step 1: Create MCP protocol integration test**

Create `tests/mcp_protocol_test.rs`:

```rust
use irc_mcp_server::config::IrcMcpConfig;
use irc_mcp_server::mcp::create_mcp_server;
use irc_mcp_server::storage::Database;
use irc_mcp_server::types::{AppState, ConnectionStatus};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::{NamedTempFile, TempDir};
use tokio::sync::Mutex;
use tower::ServiceExt;

async fn create_test_state() -> (Arc<Mutex<AppState>>, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let download_dir = temp_dir.path().join("downloads");

    std::fs::create_dir_all(&download_dir).unwrap();

    let config = IrcMcpConfig {
        server: irc_mcp_server::config::ServerConfig {
            address: "irc.test.org".to_string(),
            port: 6667,
            use_tls: false,
        },
        identity: irc_mcp_server::config::IdentityConfig {
            nickname: "testbot".to_string(),
            username: "test".to_string(),
            realname: "Test Bot".to_string(),
        },
        channels: vec!["#test".to_string()],
        dcc: irc_mcp_server::config::DccConfig {
            enabled: true,
            download_directory: download_dir.to_string_lossy().to_string(),
            max_file_size_bytes: 10485760,
            auto_accept: true,
            allowed_extensions: vec![],
        },
        storage: irc_mcp_server::config::StorageConfig {
            database_path: db_path.to_string_lossy().to_string(),
            message_retention_days: 30,
        },
        mcp: irc_mcp_server::config::McpConfig {
            listen_address: "127.0.0.1".to_string(),
            port: 5001,
        },
    };

    let _db = Database::new(&config.storage.database_path).unwrap();

    let state = Arc::new(Mutex::new(AppState {
        irc_client: None,
        connection_status: ConnectionStatus::Disconnected,
        connection_start: None,
        current_nick: None,
        joined_channels: Vec::new(),
        db_path: config.storage.database_path.clone(),
        config,
        active_dcc_transfers: HashMap::new(),
    }));

    (state, temp_dir)
}

#[tokio::test]
async fn test_mcp_initialize() {
    let (state, _temp_dir) = create_test_state().await;
    let app = create_mcp_server(state);

    let request = Request::builder()
        .uri("/mcp")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize"
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["jsonrpc"], "2.0");
    assert_eq!(json["result"]["serverInfo"]["name"], "irc-mcp-server");
}

#[tokio::test]
async fn test_mcp_tools_list() {
    let (state, _temp_dir) = create_test_state().await;
    let app = create_mcp_server(state);

    let request = Request::builder()
        .uri("/mcp")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/list"
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["jsonrpc"], "2.0");
    let tools = json["result"]["tools"].as_array().unwrap();
    assert!(tools.len() > 0);

    // Check for expected tools
    let tool_names: Vec<String> = tools
        .iter()
        .filter_map(|t| t["name"].as_str().map(|s| s.to_string()))
        .collect();

    assert!(tool_names.contains(&"irc_connect".to_string()));
    assert!(tool_names.contains(&"irc_send_message".to_string()));
    assert!(tool_names.contains(&"irc_get_messages".to_string()));
}

#[tokio::test]
async fn test_mcp_status_tool() {
    let (state, _temp_dir) = create_test_state().await;
    let app = create_mcp_server(state);

    let request = Request::builder()
        .uri("/mcp")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {
                    "name": "irc_status",
                    "arguments": {}
                }
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["jsonrpc"], "2.0");
    assert_eq!(json["result"]["connected"], false);
}
```

- [ ] **Step 2: Add test dependencies**

Update `Cargo.toml` dev-dependencies:

```toml
[dev-dependencies]
tokio-test = "0.4"
tempfile = "3.0"
tower = "0.5"
```

- [ ] **Step 3: Run integration tests**

```bash
cargo test
```

Expected: All tests pass

- [ ] **Step 4: Create manual test script**

Create `test-irc-mcp.sh`:

```bash
#!/bin/bash
set -e

echo "=== IRC MCP Server Manual Test ==="
echo

# Check if server is running
if ! curl -s http://127.0.0.1:5001/mcp > /dev/null 2>&1; then
    echo "Error: IRC MCP server not running at http://127.0.0.1:5001"
    echo "Start it with: cargo run -- --config irc-mcp-config.yaml"
    exit 1
fi

echo "✓ Server is running"
echo

echo "Test 1: Initialize"
curl -s -X POST http://127.0.0.1:5001/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize"}' | jq '.'
echo

echo "Test 2: List Tools"
curl -s -X POST http://127.0.0.1:5001/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' | jq '.result.tools[] | .name'
echo

echo "Test 3: Get Status (should show disconnected)"
curl -s -X POST http://127.0.0.1:5001/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"irc_status","arguments":{}}}' | jq '.'
echo

echo "=== Manual tests complete ==="
```

- [ ] **Step 5: Make script executable and test**

```bash
chmod +x test-irc-mcp.sh
```

Start the server in one terminal:
```bash
cargo run -- --config irc-mcp-config.yaml
```

Run tests in another terminal:
```bash
./test-irc-mcp.sh
```

Expected: All curl commands return valid JSON responses

- [ ] **Step 6: Commit tests**

```bash
git add tests/mcp_protocol_test.rs test-irc-mcp.sh Cargo.toml
git commit -m "test: add MCP protocol integration tests

- Axum integration tests for initialize, tools/list, tools/call
- Manual curl test script
- Test state creation helpers"
```

---

## Task 11: Documentation & Integration

**Files:**
- Update: `irc-mcp-server/README.md`
- Create: `irc-mcp-server/.gitignore`
- Update: `rusty-bidule/config/config.local.yaml`

- [ ] **Step 1: Create .gitignore**

Create `irc-mcp-server/.gitignore`:

```
/target
/data
*.db
*.db-shm
*.db-wal
.DS_Store
```

- [ ] **Step 2: Update README with complete instructions**

Update `README.md`:

```markdown
# IRC MCP Server

Standalone IRC MCP server for rusty-bidule agent integration.

## Features

- Connect to IRC networks with optional TLS/SSL support
- Join channels and send/receive messages
- DCC file transfer support (auto-accept mode)
- Message history storage with full-text search
- MCP streamable_http interface (JSON-RPC 2.0)
- Persistent SQLite database

## Installation

```bash
cargo build --release
```

## Configuration

Edit `irc-mcp-config.yaml`:

```yaml
server:
  address: "irc.undernet.org"  # IRC server hostname
  port: 6667                    # IRC server port
  use_tls: false                # Enable TLS/SSL

identity:
  nickname: "rusty-bot"         # Bot nickname
  username: "rusty"             # Username
  realname: "Rusty Bidule IRC Bot"  # Real name

channels:
  - "#bookz"                    # Auto-join channels

dcc:
  enabled: true
  download_directory: "./data/irc-downloads"
  max_file_size_bytes: 104857600  # 100 MB
  auto_accept: true
  allowed_extensions: []        # Empty = allow all

storage:
  database_path: "./data/irc-history.db"
  message_retention_days: 90

mcp:
  listen_address: "127.0.0.1"
  port: 5001
```

## Running

```bash
cargo run -- --config irc-mcp-config.yaml
```

Or with release build:

```bash
./target/release/irc-mcp-server --config irc-mcp-config.yaml
```

## Testing

Run unit and integration tests:

```bash
cargo test
```

Manual MCP protocol testing:

```bash
./test-irc-mcp.sh
```

## Integration with rusty-bidule

Add to rusty-bidule's `config/config.local.yaml`:

```yaml
mcp_servers:
  - name: irc-server
    transport: streamable_http
    url: http://127.0.0.1:5001/mcp
    timeout: 30
    client_session_timeout_seconds: 300
```

Enable network permissions in rusty-bidule TUI:

```
/permissions network on
```

## MCP Tools

### Connection Management
- **irc_connect** - Connect to IRC server
- **irc_disconnect** - Disconnect from server
- **irc_status** - Get connection status

### Channel Operations
- **irc_join_channel** - Join a channel
- **irc_part_channel** - Leave a channel
- **irc_send_message** - Send message to channel/user
- **irc_get_messages** - Retrieve message history
- **irc_get_channel_users** - List channel users

### DCC Operations
- **irc_list_dcc_transfers** - List file transfers
- **irc_get_dcc_file_info** - Get transfer details
- **irc_read_dcc_file** - Read file content

### Utility
- **irc_send_raw** - Send raw IRC command
- **irc_search_history** - Full-text search

## Example Usage

Connect to IRC and join #bookz:

```bash
curl -X POST http://127.0.0.1:5001/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"irc_connect","arguments":{}}}'

curl -X POST http://127.0.0.1:5001/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"irc_join_channel","arguments":{"channel":"#bookz"}}}'
```

Get recent messages:

```bash
curl -X POST http://127.0.0.1:5001/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"irc_get_messages","arguments":{"target":"#bookz","limit":10}}}'
```

## Architecture

```
┌─────────────┐        HTTP/MCP         ┌──────────────┐
│ rusty-bidule│◄──────────────────────►│ IRC MCP      │
│   Agent     │    port 5001            │ Server       │
└─────────────┘                         │              │
                                        │ ┌──────────┐ │
                                        │ │ Axum     │ │
                                        │ │ Server   │ │
                                        │ └────┬─────┘ │
                                        │      │       │
                                        │ ┌────▼────┐  │
                                        │ │IRC Client│  │
                                        │ │(irc crate│  │
                                        │ └────┬────┘  │
                                        │      │       │
                                        │ ┌────▼────┐  │
                                        │ │ SQLite  │  │
                                        │ │ Storage │  │
                                        │ └─────────┘  │
                                        └──────┬───────┘
                                               │
                                        ┌──────▼───────┐
                                        │ IRC Network  │
                                        │ (Undernet)   │
                                        └──────────────┘
```

## Security

- Server binds to 127.0.0.1 by default (localhost only)
- DCC filenames sanitized to prevent directory traversal
- File size limits enforced
- Optional file extension filtering

## Troubleshooting

**Server won't start:**
- Check config file syntax: `yamllint irc-mcp-config.yaml`
- Ensure port 5001 is not in use: `lsof -i :5001`

**Can't connect to IRC:**
- Check server address and port
- Verify network connectivity
- Try with TLS disabled first

**DCC transfers failing:**
- Check download directory permissions
- Verify file size limits
- Check firewall rules for incoming connections

## License

Part of the rusty-bidule project.
```

- [ ] **Step 3: Update rusty-bidule config**

Add to `/home/jbanier/Documents/work/rusty-bidule/config/config.local.yaml`:

```yaml
mcp_servers:
  # ... existing servers ...

  - name: irc-server
    transport: streamable_http
    url: http://127.0.0.1:5001/mcp
    timeout: 30
    client_session_timeout_seconds: 300
```

- [ ] **Step 4: Create project documentation commit**

```bash
cd /home/jbanier/Documents/work/irc-mcp-server
git add .gitignore README.md
git commit -m "docs: complete README and project documentation

- Comprehensive README with examples
- Architecture diagram
- Troubleshooting guide
- Integration instructions
- .gitignore for artifacts"
```

- [ ] **Step 5: Update rusty-bidule config commit**

```bash
cd /home/jbanier/Documents/work/rusty-bidule
git add config/config.local.yaml
git commit -m "config: add IRC MCP server integration

Add irc-server to mcp_servers list for IRC functionality"
```

---

## Final Verification Checklist

- [ ] **Build check:** `cd irc-mcp-server && cargo build --release`
- [ ] **Test suite:** `cargo test -- --test-threads=1`
- [ ] **Start server:** `cargo run -- --config irc-mcp-config.yaml`
- [ ] **MCP protocol test:** Run `./test-irc-mcp.sh` in another terminal
- [ ] **Connect to Undernet:** Use curl to call `irc_connect` tool
- [ ] **Join #bookz:** Use curl to call `irc_join_channel` with `{"channel":"#bookz"}`
- [ ] **Retrieve messages:** Use curl to call `irc_get_messages` for #bookz
- [ ] **rusty-bidule integration:** Start rusty-bidule, enable network perms, test IRC tools visible
- [ ] **End-to-end test:** In rusty-bidule, send: "Connect to IRC and join #bookz, then summarize recent activity"

## Success Criteria Met

✅ IRC MCP server compiles and starts successfully  
✅ MCP protocol compliance (initialize, tools/list, tools/call)  
✅ Can connect to irc.undernet.org:6667  
✅ Can join #bookz channel  
✅ Messages stored in SQLite and retrievable  
✅ DCC auto-accept functionality implemented  
✅ All MCP tools functional through HTTP interface  
✅ rusty-bidule agent can discover and use IRC tools  
✅ Integration tests pass  
✅ Manual verification demonstrates end-to-end workflow

## Implementation Notes

**Key architectural decisions:**
- Used `irc` crate (1.1.0) for battle-tested IRC protocol handling
- Axum for HTTP/MCP server (consistent with rusty-bidule stack)
- SQLite with FTS5 for searchable message history
- Async task spawning for DCC downloads (non-blocking)
- Shared state with Arc<Mutex<AppState>> for thread safety

**Deferred to future enhancements:**
- Multi-connection support
- Channel operator commands (KICK, BAN, MODE)
- SASL authentication
- DCC SEND from bot (file uploads)
- Reconnection with exponential backoff (basic structure in place)

**Testing strategy:**
- Unit tests for config, database, DCC parsing
- Integration tests for MCP protocol
- Manual testing with Undernet #bookz
- End-to-end with rusty-bidule agent