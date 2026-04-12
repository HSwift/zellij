# Zellij WebSocket Plugin API

## Overview

The WebSocket Plugin API enables Zellij plugins to establish persistent, bidirectional WebSocket connections to remote servers. Unlike the existing `web_request` API (which is request/response based), WebSocket connections remain open and can send/receive messages in real-time.

> [!IMPORTANT]
> WebSocket operations require the **`WebAccess`** permission. Plugins must request this permission via `request_permission` and users must grant it before any WebSocket function will work.

## Quick Start

```rust
use zellij_tile::prelude::*;
use std::collections::BTreeMap;

struct MyPlugin {
    connection_id: Option<u32>,
}

impl ZellijPlugin for MyPlugin {
    fn load(&mut self, _config: BTreeMap<String, String>) {
        // 1. Request permission
        request_permission(&[
            PermissionType::WebAccess,
        ]);

        // 2. Subscribe to WebSocket events
        subscribe(&[
            EventType::WebSocketConnected,
            EventType::WebSocketMessage,
            EventType::WebSocketError,
            EventType::WebSocketDisconnected,
        ]);
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::PermissionRequestResult(PermissionStatus::Granted) => {
                // 3. Open a WebSocket connection
                web_socket_open(
                    "wss://echo.websocket.org",
                    BTreeMap::new(),          // headers
                    BTreeMap::new(),          // context
                );
            },
            Event::WebSocketConnected(id, _ctx) => {
                // 4. Store the connection ID and send a message
                self.connection_id = Some(id);
                web_socket_send(id, b"Hello, WebSocket!".to_vec(), false);
            },
            Event::WebSocketMessage(id, data, is_binary) => {
                // 5. Handle incoming messages
                if !is_binary {
                    let text = String::from_utf8_lossy(&data);
                    eprintln!("Received: {}", text);
                }
            },
            Event::WebSocketError(id, error, _ctx) => {
                eprintln!("WebSocket error on {}: {}", id, error);
            },
            Event::WebSocketDisconnected(id, reason, _ctx) => {
                eprintln!("Disconnected {}: {}", id, reason);
                self.connection_id = None;
            },
            _ => {},
        }
        false
    }
}
```

---

## Functions

### `web_socket_open`

Opens a new WebSocket connection.

```rust
pub fn web_socket_open<S: AsRef<str>>(
    url: S,
    headers: BTreeMap<String, String>,
    context: BTreeMap<String, String>,
)
```

| Parameter | Type | Description |
|-----------|------|-------------|
| `url` | `impl AsRef<str>` | WebSocket URL. Must use `ws://` or `wss://` scheme. |
| `headers` | `BTreeMap<String, String>` | Custom HTTP headers for the WebSocket handshake (e.g., `Authorization`). |
| `context` | `BTreeMap<String, String>` | Arbitrary key-value pairs returned verbatim in all subsequent events for this connection. Useful for tagging connections with metadata. |

**Behavior:**
- The connection is established asynchronously in the background.
- On success, a `WebSocketConnected` event is fired with the assigned `connection_id`.
- On failure, a `WebSocketError` event is fired.

**Example:**
```rust
let mut headers = BTreeMap::new();
headers.insert("Authorization".into(), "Bearer my-token".into());

let mut context = BTreeMap::new();
context.insert("purpose".into(), "chat".into());

web_socket_open("wss://api.example.com/ws", headers, context);
```

---

### `web_socket_send`

Sends a message on an established WebSocket connection.

```rust
pub fn web_socket_send(connection_id: u32, message: Vec<u8>, is_binary: bool)
```

| Parameter | Type | Description |
|-----------|------|-------------|
| `connection_id` | `u32` | Connection identifier received in `WebSocketConnected`. |
| `message` | `Vec<u8>` | Message payload bytes. |
| `is_binary` | `bool` | `true` → binary frame, `false` → text frame (bytes are interpreted as UTF-8). |

**Example — Text message:**
```rust
let json = r#"{"action": "subscribe", "channel": "updates"}"#;
web_socket_send(connection_id, json.as_bytes().to_vec(), false);
```

**Example — Binary message:**
```rust
let binary_data: Vec<u8> = vec![0x01, 0x02, 0x03];
web_socket_send(connection_id, binary_data, true);
```

> [!NOTE]
> If `is_binary` is `false` and the bytes are not valid UTF-8, the message is automatically sent as a binary frame instead.

---

### `web_socket_close`

Gracefully closes a WebSocket connection.

```rust
pub fn web_socket_close(connection_id: u32)
```

| Parameter | Type | Description |
|-----------|------|-------------|
| `connection_id` | `u32` | Connection identifier received in `WebSocketConnected`. |

**Behavior:**
- Sends a WebSocket close frame to the server.
- A `WebSocketDisconnected` event is fired with reason `"Closed by plugin"`.

---

## Events

All WebSocket events require subscription via `subscribe()`. The `context` map in each event is the same map passed to `web_socket_open()`.

### `WebSocketConnected`

Fired when a WebSocket connection is successfully established.

```rust
Event::WebSocketConnected(
    connection_id: u32,
    context: BTreeMap<String, String>,
)
```

| Field | Type | Description |
|-------|------|-------------|
| `connection_id` | `u32` | Unique connection identifier. Use this for `web_socket_send` and `web_socket_close`. |
| `context` | `BTreeMap<String, String>` | The context passed to `web_socket_open`. |

---

### `WebSocketMessage`

Fired when a message is received from the WebSocket server.

```rust
Event::WebSocketMessage(
    connection_id: u32,
    message: Vec<u8>,
    is_binary: bool,
)
```

| Field | Type | Description |
|-------|------|-------------|
| `connection_id` | `u32` | Which connection the message came from. |
| `message` | `Vec<u8>` | Raw message bytes. For text messages, decode with `String::from_utf8`. |
| `is_binary` | `bool` | `true` if the server sent a binary frame, `false` for text. |

---

### `WebSocketError`

Fired when an error occurs on a WebSocket connection.

```rust
Event::WebSocketError(
    connection_id: u32,
    error: String,
    context: BTreeMap<String, String>,
)
```

| Field | Type | Description |
|-------|------|-------------|
| `connection_id` | `u32` | Which connection encountered the error. |
| `error` | `String` | Human-readable error description. |
| `context` | `BTreeMap<String, String>` | The context passed to `web_socket_open`. |

> [!NOTE]
> A `WebSocketError` caused by a broken connection is always followed by a `WebSocketDisconnected` event. You don't need to call `web_socket_close` after receiving an error — cleanup is handled automatically.

---

### `WebSocketDisconnected`

Fired when a WebSocket connection is closed, regardless of the cause.

```rust
Event::WebSocketDisconnected(
    connection_id: u32,
    reason: String,
    context: BTreeMap<String, String>,
)
```

| Field | Type | Description |
|-------|------|-------------|
| `connection_id` | `u32` | Which connection was closed. |
| `reason` | `String` | Why the connection closed. Examples: `"Closed by plugin"`, `"Normal closure"`, `"Connection closed"`, `"Disconnected due to error: ..."`. |
| `context` | `BTreeMap<String, String>` | The context passed to `web_socket_open`. |

---

## Connection Lifecycle

```
web_socket_open()
        │
        ├── Success ──► WebSocketConnected(id, ctx)
        │                       │
        │               ┌──────┴──────┐
        │               ▼              ▼
        │       WebSocketMessage   web_socket_send()
        │       (incoming msgs)    (outgoing msgs)
        │               │
        │               ├── Error ──► WebSocketError(id, err, ctx)
        │               │                     │
        │               │                     ▼
        │               ├────────────► WebSocketDisconnected(id, reason, ctx)
        │               │              (always the final event)
        │               │
        │               └── web_socket_close(id) ──► WebSocketDisconnected
        │
        └── Failure ──► WebSocketError(id, err, ctx)
                        (no WebSocketDisconnected follows for connection failures)
```

> [!IMPORTANT]
> `WebSocketDisconnected` is the terminal event — after receiving it, the `connection_id` is no longer valid. The only exception is a connection failure during `web_socket_open`, where only `WebSocketError` is fired because no connection was ever established.

---

## Error Handling Patterns

### Reconnection with Backoff

```rust
struct MyPlugin {
    connection_id: Option<u32>,
    reconnect_attempts: u32,
}

impl ZellijPlugin for MyPlugin {
    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::WebSocketDisconnected(id, reason, ctx) => {
                self.connection_id = None;
                self.reconnect_attempts += 1;
                // Exponential backoff using set_timeout
                let delay_secs = (2u64.pow(self.reconnect_attempts.min(5))) as f64;
                set_timeout(delay_secs);
            },
            Event::Timer(_elapsed) => {
                // Reconnect on timer
                web_socket_open(
                    "wss://api.example.com/ws",
                    BTreeMap::new(),
                    BTreeMap::new(),
                );
            },
            Event::WebSocketConnected(id, _ctx) => {
                self.connection_id = Some(id);
                self.reconnect_attempts = 0; // Reset on success
            },
            _ => {},
        }
        false
    }
}
```

### Multiple Connections

Use the `context` map to distinguish between connections:

```rust
fn open_data_feed(&self) {
    let mut ctx = BTreeMap::new();
    ctx.insert("type".into(), "data".into());
    web_socket_open("wss://data.example.com/feed", BTreeMap::new(), ctx);
}

fn open_control_channel(&self) {
    let mut ctx = BTreeMap::new();
    ctx.insert("type".into(), "control".into());
    web_socket_open("wss://api.example.com/control", BTreeMap::new(), ctx);
}

// In update():
Event::WebSocketMessage(id, data, _is_binary) => {
    // Route by connection_id, or check context in WebSocketConnected
}
```

---

## EventType Subscription Reference

| EventType | When to subscribe |
|-----------|-------------------|
| `EventType::WebSocketConnected` | Always — needed to get the `connection_id`. |
| `EventType::WebSocketMessage` | Always — this is how you receive data. |
| `EventType::WebSocketError` | Recommended — handle errors gracefully. |
| `EventType::WebSocketDisconnected` | Recommended — clean up state when connections close. |

---

## Comparison with `web_request`

| Feature | `web_request` | WebSocket API |
|---------|---------------|---------------|
| Protocol | HTTP/HTTPS | WS/WSS |
| Connection | One-shot | Persistent |
| Direction | Request → Response | Bidirectional |
| Result event | `WebRequestResult` | Multiple events over time |
| Use case | REST APIs, file downloads | Chat, streaming, real-time data |
| Permission | `WebAccess` | `WebAccess` |
