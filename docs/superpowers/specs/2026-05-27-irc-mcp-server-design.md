# IRC MCP Server Design Specification

**Date:** 2026-05-27  
**Status:** Draft  
**Author:** AI Assistant with user collaboration

## Context

This design addresses the need for rusty-bidule to interact with IRC servers for investigation workflows. The agent needs to connect to IRC networks (with TLS/SSL support), join channels, send and receive messages, handle DCC file transfers, and interact with received files. This capability extends rusty-bidule's tool ecosystem beyond web APIs and local commands into real-time chat protocols.

The primary use case is connecting to IRC channels that may share files via DCC, allowing the agent to monitor conversations, receive files, and interact with the content programmatically.

## Design Overview

The IRC MCP server is a standalone Rust HTTP service that maintains persistent IRC connections and exposes IRC operations through the MCP (Model Context Protocol) interface. Rusty-bidule connects to it via the `streamable_http` transport, similar to the existing csirt-mcp integration.

**Key Design Decisions:**
1. **Standalone HTTP server** - IRC connections persist across rusty-bidule restarts; better separation of concerns
2. **Single persistent connection** - One IRC identity shared across MCP sessions; simpler state management
3. **Auto-accept DCC transfers** - Files automatically saved to designated directory with security controls
4. **Separate configuration** - IRC server has its own config file referenced from rusty-bidule's mcp_servers list
5. **Message history storage** - SQLite database for searchable message logs

## Architecture

### Component Diagram

```
┌─────────────────┐          HTTP/MCP          ┌──────────────────────┐
│                 │◄─────────────────────────►│                      │
│  rusty-bidule   │   streamable_http         │   IRC MCP Server     │
│                 │   (port 5001)             │                      │
└─────────────────┘                            │  ┌────────────────┐  │
                                               │  │  Axum HTTP     │  │
                                               │  │  MCP Handler   │  │
                                               │  └────────┬───────┘  │
                                               │           │          │
                                               │  ┌────────▼───────┐  │
                                               │  │  IRC Client    │  │
                                               │  │  (irc crate)   │  │
                                               │  └────────┬───────┘  │
                                               │           │          │
                                               │  ┌────────▼───────┐  │
                                               │  │  Message       │  │
                                               │  │  Router        │  │
                                               │  └─┬────────────┬─┘  │
                                               │    │            │    │
                                               │  ┌─▼──────┐  ┌──▼─┐ │
                                               │  │ SQLite │  │DCC │ │
                                               │  │History │  │ IO │ │
                                               │  └────────┘  └────┘ │
                                               └──────────────────────┘
                                                        │
                                                        │ IRC Protocol
                                                        │ (TLS/SSL)
                                                        ▼
                                               ┌──────────────────────┐
                                               │   IRC Network        │
                                               │  (irc.undernet.org)  │
                                               └──────────────────────┘
```

### Core Components

**1. HTTP/MCP Layer (Axum)**
- Axum web server at configurable host:port (default: 127.0.0.1:5001)
- MCP endpoint at `/mcp` supporting streamable_http transport
- Implements MCP protocol operations: `initialize`, `tools/list`, `tools/call`
- JSON-RPC 2.0 request/response handling
- Session management per MCP spec (though we use shared IRC connection)

**2. IRC Client Layer**
- Uses `irc` crate (v1.1.0) for protocol handling
- Single `irc::client::Client` instance with persistent connection
- Configured via `irc::client::data::Config` struct:
  - Server hostname and port
  - TLS/SSL settings (optional)
  - Nickname, username, real name
  - Auto-join channels on connect
- Background Tokio task processes incoming IRC messages
- Handles PING/PONG automatically via irc crate
- Reconnection logic with exponential backoff on disconnect

**3. Message Router & Storage**
- Receives all IRC messages from client's message stream
- Classifies messages by type:
  - Channel messages (PRIVMSG to #channel)
  - Private messages (PRIVMSG to nick)
  - CTCP requests (including DCC offers)
  - Server notices and system messages
- Routes DCC SEND offers to DCC handler
- Persists all messages to SQLite database
- Schema: `messages(id, timestamp, source_nick, target, message_type, content, channel)`
- Indexed on timestamp and channel for efficient queries

**4. DCC Transfer Handler**
- Parses `DCC SEND filename ipaddr port filesize` from CTCP messages
- Creates TCP connection to offered address:port
- Downloads file to configured directory with temp extension (.part)
- Validates received size against advertised size
- Renames to final filename on successful completion
- Stores metadata: `dcc_transfers(id, timestamp, sender_nick, filename, filepath, filesize, status, error)`
- Security controls:
  - Configurable max file size limit
  - Path sanitization (prevent directory traversal)
  - Quarantine directory option
  - File type restrictions (optional)

**5. Configuration Manager**
- Loads `irc-mcp-config.yaml` on startup
- Configuration structure:
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

## MCP Tools Specification

### Connection Management

#### `irc_connect`
**Description:** Establish or re-establish connection to the configured IRC server.

**Input Schema:**
```json
{
  "type": "object",
  "properties": {},
  "required": []
}
```

**Output:**
```json
{
  "success": true,
  "server": "irc.undernet.org:6667",
  "nick": "rusty-bot",
  "joined_channels": ["#bookz"]
}
```

#### `irc_disconnect`
**Description:** Gracefully disconnect from IRC server with optional quit message.

**Input Schema:**
```json
{
  "type": "object",
  "properties": {
    "quit_message": {
      "type": "string",
      "description": "Optional quit message",
      "default": "Disconnecting"
    }
  }
}
```

#### `irc_status`
**Description:** Get current connection status and state.

**Output:**
```json
{
  "connected": true,
  "server": "irc.undernet.org:6667",
  "nick": "rusty-bot",
  "channels": ["#bookz"],
  "uptime_seconds": 3600
}
```

### Channel Operations

#### `irc_join_channel`
**Description:** Join an IRC channel.

**Input Schema:**
```json
{
  "type": "object",
  "properties": {
    "channel": {
      "type": "string",
      "description": "Channel name (must start with #)"
    }
  },
  "required": ["channel"]
}
```

#### `irc_part_channel`
**Description:** Leave an IRC channel.

**Input Schema:**
```json
{
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
```

#### `irc_send_message`
**Description:** Send a message to a channel or user.

**Input Schema:**
```json
{
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
```

#### `irc_get_messages`
**Description:** Retrieve messages from a channel or private conversation.

**Input Schema:**
```json
{
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
      "description": "ISO 8601 timestamp - only return messages after this time"
    },
    "sender_filter": {
      "type": "string",
      "description": "Only return messages from this nickname"
    },
    "search_query": {
      "type": "string",
      "description": "Full-text search in message content"
    }
  },
  "required": ["target"]
}
```

**Output:**
```json
{
  "messages": [
    {
      "id": 12345,
      "timestamp": "2026-05-27T10:30:00Z",
      "sender": "alice",
      "target": "#bookz",
      "content": "Check out this file!",
      "message_type": "channel"
    }
  ],
  "count": 1,
  "has_more": false
}
```

#### `irc_get_channel_users`
**Description:** List users currently in a channel.

**Input Schema:**
```json
{
  "type": "object",
  "properties": {
    "channel": {
      "type": "string",
      "description": "Channel name"
    }
  },
  "required": ["channel"]
}
```

### DCC Operations

#### `irc_list_dcc_transfers`
**Description:** List DCC file transfers (pending, in progress, completed, failed).

**Input Schema:**
```json
{
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
```

**Output:**
```json
{
  "transfers": [
    {
      "id": 1,
      "timestamp": "2026-05-27T10:35:00Z",
      "sender": "alice",
      "filename": "document.pdf",
      "filesize": 2048576,
      "status": "completed",
      "filepath": "./data/irc-downloads/document.pdf"
    }
  ]
}
```

#### `irc_get_dcc_file_info`
**Description:** Get detailed information about a received DCC file.

**Input Schema:**
```json
{
  "type": "object",
  "properties": {
    "transfer_id": {
      "type": "integer",
      "description": "DCC transfer ID"
    }
  },
  "required": ["transfer_id"]
}
```

#### `irc_read_dcc_file`
**Description:** Read content from a received DCC file. Supports chunking for large files.

**Input Schema:**
```json
{
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
```

### Utility Operations

#### `irc_send_raw`
**Description:** Send a raw IRC protocol command. Use with caution.

**Input Schema:**
```json
{
  "type": "object",
  "properties": {
    "command": {
      "type": "string",
      "description": "Raw IRC command (e.g., 'WHOIS alice')"
    }
  },
  "required": ["command"]
}
```

#### `irc_search_history`
**Description:** Full-text search across all message history.

**Input Schema:**
```json
{
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
```

## Implementation Details

### Technology Stack
- **Language:** Rust 2024 edition
- **HTTP Server:** axum 0.8+
- **IRC Client:** irc 1.1.0
- **Database:** rusqlite for SQLite
- **Async Runtime:** tokio (full features)
- **Serialization:** serde + serde_json
- **TLS:** native-tls or rustls (via irc crate features)
- **Logging:** tracing + tracing-subscriber

### Project Structure
```
irc-mcp-server/
├── Cargo.toml
├── src/
│   ├── main.rs              # Entry point, server startup
│   ├── config.rs            # Configuration loading and validation
│   ├── mcp/
│   │   ├── mod.rs           # MCP protocol handler
│   │   ├── server.rs        # Axum server setup
│   │   └── tools.rs         # MCP tool implementations
│   ├── irc/
│   │   ├── mod.rs           # IRC client wrapper
│   │   ├── client.rs        # irc crate integration
│   │   └── dcc.rs           # DCC file transfer handler
│   ├── storage/
│   │   ├── mod.rs           # Storage abstraction
│   │   ├── database.rs      # SQLite operations
│   │   └── schema.sql       # Database schema
│   └── types.rs             # Shared types and structs
├── irc-mcp-config.yaml      # Default configuration
└── README.md
```

### Database Schema

```sql
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

CREATE INDEX idx_messages_timestamp ON messages(timestamp DESC);
CREATE INDEX idx_messages_channel ON messages(channel, timestamp DESC);
CREATE INDEX idx_messages_target ON messages(target, timestamp DESC);
CREATE VIRTUAL TABLE messages_fts USING fts5(content, content=messages, content_rowid=id);

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

CREATE INDEX idx_dcc_status ON dcc_transfers(status, timestamp DESC);

CREATE TABLE IF NOT EXISTS channels (
    channel_name TEXT PRIMARY KEY,
    joined_at TEXT NOT NULL,
    last_activity TEXT
);
```

### State Management

**Global State (Arc<Mutex<AppState>>):**
```rust
struct AppState {
    irc_client: Option<irc::client::Client>,
    connection_status: ConnectionStatus,
    db_pool: SqlitePool,
    config: IrcMcpConfig,
    active_dcc_transfers: HashMap<u64, DccTransfer>,
}
```

**Connection Lifecycle:**
1. Server starts with `connection_status = Disconnected`
2. First `irc_connect` call initializes IRC client
3. IRC client spawned in background task
4. Message stream processed in separate task
5. On disconnect: attempt reconnect with exponential backoff
6. Manual `irc_disconnect` sets status and cancels reconnect

### Error Handling

**Error Categories:**
- **Configuration errors** - Invalid config file, missing required fields
- **Connection errors** - Cannot connect to IRC server, TLS failures
- **IRC protocol errors** - Nickname in use, banned from channel, etc.
- **DCC errors** - Transfer timeout, size mismatch, file I/O errors
- **MCP errors** - Invalid tool parameters, tool execution failures

**Error Response Format (MCP):**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32603,
    "message": "IRC connection failed: Connection refused",
    "data": {
      "category": "connection",
      "retryable": true
    }
  }
}
```

### Security Considerations

1. **DCC File Transfer Security:**
   - Validate IP addresses (reject RFC1918 if server is public)
   - Enforce maximum file size limits
   - Sanitize filenames (remove path separators, ..)
   - Optional file type whitelist/blacklist
   - Quarantine directory option for scanning

2. **IRC Command Injection:**
   - Validate channel names (must start with #)
   - Escape message content that could be interpreted as IRC commands
   - Rate limiting on message sends

3. **MCP Access Control:**
   - Bind to 127.0.0.1 by default (localhost only)
   - Optional bearer token authentication
   - Tool-level permission flags (e.g., disable `irc_send_raw`)

4. **Resource Limits:**
   - Connection pool limits
   - Message history retention (auto-delete old messages)
   - DCC transfer concurrency limits
   - SQLite database size limits

## Integration with rusty-bidule

### Configuration in rusty-bidule

Add to `config/config.local.yaml`:

```yaml
mcp_servers:
  - name: irc-server
    transport: streamable_http
    url: http://127.0.0.1:5001/mcp
    timeout: 30
    client_session_timeout_seconds: 300
```

### Agent Permissions

IRC operations require:
- `allow_network: true` - For MCP communication and IRC connections
- No filesystem permissions needed (IRC server manages its own storage)

### Example Agent Workflow

1. Agent starts conversation with IRC investigation recipe
2. Calls `irc_status` → sees connection is down
3. Calls `irc_connect` → establishes connection to Undernet
4. Calls `irc_join_channel("#bookz")` → joins channel
5. Calls `irc_get_messages("#bookz", limit=50)` → reads recent chat
6. Monitors for DCC offers via `irc_list_dcc_transfers()`
7. When file received, calls `irc_get_dcc_file_info(transfer_id)`
8. Calls `irc_read_dcc_file(transfer_id)` → analyzes content
9. Calls `irc_send_message("#bookz", "Thanks for the file!")`

## Testing Strategy

### Unit Tests
- Configuration parsing and validation
- Message routing logic
- DCC filename sanitization
- Error handling paths

### Integration Tests
- MCP protocol compliance (tools/list, tools/call)
- IRC client connection and disconnection
- Channel join/part operations
- Message send/receive roundtrip
- SQLite storage operations

### End-to-End Testing
- Connect to local IRC server (ircd-hybrid or unrealircd in Docker)
- Full workflow: connect → join → send → receive → search history
- DCC transfer test with local sender
- Reconnection after simulated disconnect
- MCP client (rusty-bidule) integration test

### Test Configuration for Undernet

```yaml
server:
  address: "irc.undernet.org"
  port: 6667
  use_tls: false
  
identity:
  nickname: "rusty-test-bot"
  username: "rusty"
  realname: "Test Bot"
  
channels:
  - "#bookz"
  
dcc:
  enabled: true
  download_directory: "./test-downloads"
  max_file_size_bytes: 10485760  # 10 MB for testing
```

## Verification Plan

### Manual Verification Steps

1. **Server Startup:**
   ```bash
   cd irc-mcp-server
   cargo run -- --config irc-mcp-config.yaml
   # Should log: "IRC MCP Server listening on 127.0.0.1:5001"
   ```

2. **MCP Protocol Check:**
   ```bash
   curl -X POST http://127.0.0.1:5001/mcp \
     -H "Content-Type: application/json" \
     -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'
   # Should return list of IRC tools
   ```

3. **IRC Connection:**
   ```bash
   curl -X POST http://127.0.0.1:5001/mcp \
     -H "Content-Type: application/json" \
     -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"irc_connect","arguments":{}}}'
   # Should connect to irc.undernet.org
   ```

4. **Join #bookz:**
   ```bash
   curl -X POST http://127.0.0.1:5001/mcp \
     -H "Content-Type: application/json" \
     -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"irc_join_channel","arguments":{"channel":"#bookz"}}}'
   ```

5. **Retrieve Messages:**
   ```bash
   curl -X POST http://127.0.0.1:5001/mcp \
     -H "Content-Type: application/json" \
     -d '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"irc_get_messages","arguments":{"target":"#bookz","limit":10}}}'
   ```

6. **rusty-bidule Integration:**
   ```bash
   cd rusty-bidule
   # Update config/config.local.yaml with irc-server mcp entry
   cargo run
   # In TUI, enable network permissions:
   /permissions network on
   # Test IRC tools are available - should see irc_* tools in tool list
   # Send prompt: "Connect to IRC and join #bookz, then tell me what people are discussing"
   ```

### Automated Test Checklist

- [ ] Configuration loads successfully with valid YAML
- [ ] Configuration validation catches missing required fields
- [ ] MCP server starts and responds to tools/list
- [ ] IRC client connects to test server
- [ ] JOIN command sent and channel joined
- [ ] PRIVMSG sent successfully
- [ ] Incoming messages stored in database
- [ ] Message retrieval with filters works
- [ ] DCC SEND offer parsed correctly
- [ ] DCC file downloaded and validated
- [ ] Graceful disconnect and cleanup
- [ ] Reconnection after connection loss
- [ ] Full-text search returns correct results

## Future Enhancements

Features intentionally deferred for future iterations:

1. **Multi-connection support** - Multiple IRC servers/identities simultaneously
2. **Channel operator commands** - KICK, BAN, TOPIC, MODE changes
3. **SASL authentication** - For networks requiring auth before registration
4. **DCC CHAT support** - Direct peer-to-peer chat sessions
5. **File upload (DCC SEND from bot)** - Currently only receiving files
6. **IRC bouncer integration** - ZNC or similar persistent connection
7. **CTCP VERSION/TIME responses** - Basic CTCP reply automation
8. **Message threading/context** - Group related messages for agent context
9. **Notification webhooks** - Alert external systems on specific events
10. **Web UI** - Browser interface for monitoring IRC activity

## Dependencies

### Cargo.toml

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

## Open Questions

None - all major design decisions have been addressed through the clarification phase.

## Success Criteria

The implementation is considered successful when:

1. IRC MCP server starts and accepts MCP connections
2. Can connect to irc.undernet.org:6667 successfully
3. Can join #bookz and send/receive messages
4. Messages are stored in SQLite and retrievable with filters
5. DCC file transfers auto-accept and save files correctly
6. rusty-bidule agent can use all IRC tools through MCP interface
7. Connection survives network interruptions with auto-reconnect
8. All critical paths have error handling
9. Integration tests pass with local IRC server
10. Manual testing with Undernet demonstrates end-to-end workflow
