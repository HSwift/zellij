mod commands;
mod keys;
mod protocol;
mod screen_monitor;

use protocol::*;
use screen_monitor::ScreenMonitor;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use zellij_tile::prelude::*;

/// Represents a pending command that needs a screenshot after output settles.
#[derive(Debug, Clone)]
struct PendingScreenshot {
    /// The original request envelope (to echo command_id etc.)
    request: MessageEnvelope,
    /// The pane we're watching for output.
    pane_id: PaneId,
    /// How many timer ticks we've elapsed since the last content change.
    ticks_idle: u32,
    /// The last seen content length (to detect changes).
    last_content_hash: u64,
}

/// Zellij Agent Plugin — bridges WebSocket ↔ terminal for the OpenCC AI platform.
struct AgentPlugin {
    /// Zellij session name, used as session_id in the protocol.
    session_id: String,
    /// The currently focused terminal pane.
    focused_pane: Option<PaneId>,
    /// The tab index of the focused pane.
    focused_tab: Option<usize>,
    /// Permissions granted flag.
    permissions_granted: bool,
    /// Screen monitor for `# help` detection.
    screen_monitor: ScreenMonitor,
    /// Pending commands waiting for output to stabilize before capturing screenshot.
    pending_screenshots: Vec<PendingScreenshot>,
    /// Seconds since plugin loaded (approximate, from timer ticks).
    uptime: u64,
    /// The WebSocket connection ID.
    connection_id: Option<u32>,
    /// Track reconnection attempts.
    reconnect_attempts: u32,
    /// Ticks since last heartbeat.
    heartbeat_ticks: u32,
    /// The base WebSocket URL.
    opencc_url: String,
    /// Number of render-settle ticks required before considering output stable.
    settle_threshold: u32,
}

impl Default for AgentPlugin {
    fn default() -> Self {
        Self {
            session_id: String::new(),
            focused_pane: None,
            focused_tab: None,
            permissions_granted: false,
            screen_monitor: ScreenMonitor::new(),
            pending_screenshots: Vec::new(),
            uptime: 0,
            connection_id: None,
            reconnect_attempts: 0,
            heartbeat_ticks: 0,
            opencc_url: "ws://localhost:8000".to_string(),
            settle_threshold: 2, // 2 ticks * 0.2s = ~400ms
        }
    }
}

/// The number of seconds between heartbeats.
const HEARTBEAT_INTERVAL: f64 = 20.0;
/// Fast timer tick in seconds for checking stability.
const TICK_INTERVAL: f64 = 0.2;


register_plugin!(AgentPlugin);

impl ZellijPlugin for AgentPlugin {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        // Request all needed permissions
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
            PermissionType::WriteToStdin,
            PermissionType::ReadPaneContents,
            PermissionType::RunCommands,
            PermissionType::FullHdAccess,
            PermissionType::WebAccess,
        ]);

        // Subscribe to events
        subscribe(&[
            EventType::PaneRenderReport,
            EventType::PermissionRequestResult,
            EventType::PaneUpdate,
            EventType::TabUpdate,
            EventType::Timer,
            EventType::WebSocketConnected,
            EventType::WebSocketMessage,
            EventType::WebSocketError,
            EventType::WebSocketDisconnected,
        ]);

        if let Some(url) = configuration.get("opencc_url") {
            self.opencc_url = url.clone();
        }
        if let Some(sid) = configuration.get("session_id") {
            self.session_id = sid.clone();
        }

        // Hide ourselves — we're a background plugin
        hide_self();

        eprintln!("[zj-agent] Plugin loaded, waiting for permissions...");
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::PermissionRequestResult(status) => {
                match status {
                    PermissionStatus::Granted => {
                        self.permissions_granted = true;
                        eprintln!("[zj-agent] Permissions granted");

                        // Now that we have permissions, get session info
                        if self.session_id.is_empty() {
                            let ids = get_plugin_ids();
                            self.session_id =
                                format!("zellij-{}", ids.zellij_pid);
                        }
                        
                        // Connect WebSocket
                        self.connect_ws();
                    }
                    PermissionStatus::Denied => {
                        eprintln!("[zj-agent] Permissions denied! Plugin will not function.");
                    }
                }
                false
            }
            Event::WebSocketConnected(id, _ctx) => {
                eprintln!("[zj-agent] WebSocket connected: {}", id);
                self.connection_id = Some(id);
                self.reconnect_attempts = 0;
                self.heartbeat_ticks = 0;
                // Start fast tick
                set_timeout(TICK_INTERVAL);
                false
            }
            Event::WebSocketMessage(_id, data, is_binary) => {
                if !is_binary {
                    let text = String::from_utf8_lossy(&data);
                    self.handle_ws_message(&text);
                }
                false
            }
            Event::WebSocketError(_id, error, _ctx) => {
                eprintln!("[zj-agent] WebSocket error: {}", error);
                false
            }
            Event::WebSocketDisconnected(_id, reason, _ctx) => {
                eprintln!("[zj-agent] WebSocket disconnected: {}", reason);
                self.connection_id = None;
                self.reconnect_attempts += 1;
                // Exponential backoff
                let delay = (2u64.pow(self.reconnect_attempts.min(5))) as f64;
                set_timeout(delay);
                false
            }
            Event::Timer(_elapsed) => {
                if self.connection_id.is_none() {
                    // This timer was for reconnection
                    self.connect_ws();
                } else {
                    // This timer is the fast tick
                    self.check_screenshot_ticks();

                    self.heartbeat_ticks += 1;
                    if self.heartbeat_ticks >= (HEARTBEAT_INTERVAL / TICK_INTERVAL) as u32 {
                        self.uptime += HEARTBEAT_INTERVAL as u64;
                        self.send_heartbeat();
                        self.heartbeat_ticks = 0;
                    }

                    // Schedule next tick
                    set_timeout(TICK_INTERVAL);
                }
                false
            }
            Event::TabUpdate(tabs) => {
                // Track the active tab
                for tab in &tabs {
                    if tab.active {
                        self.focused_tab = Some(tab.position);
                    }
                }
                false
            }
            Event::PaneUpdate(manifest) => {
                // Track the focused terminal pane
                if let Some(tab_pos) = self.focused_tab {
                    if let Some(panes) = manifest.panes.get(&tab_pos) {
                        for pane in panes {
                            if pane.is_focused && !pane.is_plugin {
                                self.focused_pane = Some(PaneId::Terminal(pane.id));
                            }
                        }
                    }
                }
                false
            }
            Event::PaneRenderReport(pane_contents) => {
                // 1. Update hashes for pending screenshots
                self.update_screenshot_hashes(&pane_contents);

                // 2. Scan for `# help` triggers
                let triggers = self.screen_monitor.scan_for_help(&pane_contents);
                for (_pane_id, help_payload) in triggers {
                    self.send_help_event(help_payload);
                }

                false
            }
            _ => false,
        }
    }

    fn render(&mut self, _rows: usize, _cols: usize) {
        // Minimal render — we're meant to be hidden
        println!("zj-agent | session: {} | ws: {}",
            self.session_id,
            self.connection_id.map(|id| id.to_string()).unwrap_or_else(|| "none".to_string())
        );
    }
}

impl AgentPlugin {
    fn connect_ws(&self) {
        let ws_url = format!("{}/ws/zellij/{}", self.opencc_url.trim_end_matches('/'), self.session_id);
        eprintln!("[zj-agent] Connecting to WebSocket: {}", ws_url);
        web_socket_open(&ws_url, BTreeMap::new(), BTreeMap::new());
    }

    fn send_ws(&self, payload: &str) {
        if let Some(id) = self.connection_id {
            web_socket_send(id, payload.as_bytes().to_vec(), false);
        }
    }

    fn handle_ws_message(&mut self, payload: &str) {
        // Parse the JSON envelope
        let envelope: MessageEnvelope = match serde_json::from_str(payload) {
            Ok(env) => env,
            Err(e) => {
                eprintln!("[zj-agent] Failed to parse message: {}", e);
                let error_resp = serde_json::json!({
                    "command_id": "",
                    "session_id": &self.session_id,
                    "type": "response",
                    "command": "error",
                    "payload": {},
                    "timestamp": now_iso8601(),
                    "error": format!("Invalid JSON: {}", e)
                });
                self.send_ws(&error_resp.to_string());
                return;
            }
        };

        // Only handle requests
        if envelope.msg_type != MessageType::Request {
            return;
        }

        self.handle_request(envelope);
    }

    /// Get the pane to operate on. Uses explicit pane_id from payload if given,
    /// otherwise falls back to the focused terminal pane.
    fn resolve_pane(&self, payload: &serde_json::Value) -> Result<PaneId, String> {
        if let Some(pane_id_str) = payload.get("pane_id").and_then(|v| v.as_str()) {
            let id: u32 = pane_id_str
                .parse()
                .map_err(|_| format!("Invalid pane_id: {}", pane_id_str))?;
            Ok(PaneId::Terminal(id))
        } else {
            self.focused_pane
                .ok_or_else(|| "No focused terminal pane".to_string())
        }
    }

    /// Get the CWD for file operations.
    fn get_cwd(&self) -> PathBuf {
        if let Some(pane_id) = self.focused_pane {
            get_pane_cwd(pane_id).unwrap_or_else(|_| PathBuf::from("/"))
        } else {
            PathBuf::from("/")
        }
    }

    /// Handle a parsed request envelope.
    fn handle_request(&mut self, envelope: MessageEnvelope) {
        let command = envelope.command.as_str();

        match command {
            // ── Commands that need deferred screenshot ──
            "send_input" => {
                match serde_json::from_value::<SendInputPayload>(envelope.payload.clone()) {
                    Ok(payload) => match self.resolve_pane(&envelope.payload) {
                        Ok(pane_id) => {
                            commands::handle_send_input(pane_id, &payload);
                            self.queue_screenshot(envelope, pane_id);
                        }
                        Err(e) => self.send_error_response(&envelope, &e),
                    },
                    Err(e) => {
                        self.send_error_response(&envelope, &format!("Bad payload: {}", e))
                    }
                }
            }
            "send_key" => {
                match serde_json::from_value::<SendKeyPayload>(envelope.payload.clone()) {
                    Ok(payload) => match self.resolve_pane(&envelope.payload) {
                        Ok(pane_id) => match commands::handle_send_key(pane_id, &payload) {
                            Ok(()) => {
                                self.queue_screenshot(envelope, pane_id);
                            }
                            Err(e) => self.send_error_response(&envelope, &e),
                        },
                        Err(e) => self.send_error_response(&envelope, &e),
                    },
                    Err(e) => {
                        self.send_error_response(&envelope, &format!("Bad payload: {}", e))
                    }
                }
            }
            "forward_key" => {
                match serde_json::from_value::<SendInputPayload>(envelope.payload.clone()) {
                    Ok(payload) => match self.resolve_pane(&envelope.payload) {
                        Ok(pane_id) => {
                            commands::handle_forward_key(pane_id, &payload);
                            self.queue_screenshot(envelope, pane_id);
                        }
                        Err(e) => self.send_error_response(&envelope, &e),
                    },
                    Err(e) => {
                        self.send_error_response(&envelope, &format!("Bad payload: {}", e))
                    }
                }
            }

            // ── Commands with immediate response ──
            "get_screenshot" => match self.resolve_pane(&envelope.payload) {
                Ok(pane_id) => match commands::get_screenshot(pane_id) {
                    Ok(screenshot) => {
                        let resp_payload =
                            serde_json::to_value(ScreenshotResponse { screenshot }).unwrap();
                        self.send_response(&envelope, resp_payload);
                    }
                    Err(e) => self.send_error_response(&envelope, &e),
                },
                Err(e) => self.send_error_response(&envelope, &e),
            },

            "get_cwd" => match self.resolve_pane(&envelope.payload) {
                Ok(pane_id) => match commands::handle_get_cwd(pane_id) {
                    Ok(payload) => self.send_response(&envelope, payload),
                    Err(e) => self.send_error_response(&envelope, &e),
                },
                Err(e) => self.send_error_response(&envelope, &e),
            },

            "get_shell" => match self.resolve_pane(&envelope.payload) {
                Ok(pane_id) => match commands::handle_get_shell(pane_id) {
                    Ok(payload) => self.send_response(&envelope, payload),
                    Err(e) => self.send_error_response(&envelope, &e),
                },
                Err(e) => self.send_error_response(&envelope, &e),
            },

            "read_file" => {
                match serde_json::from_value::<ReadFilePayload>(envelope.payload.clone()) {
                    Ok(payload) => {
                        let cwd = self.get_cwd();
                        match commands::handle_read_file(&cwd, &payload) {
                            Ok(resp) => self.send_response(&envelope, resp),
                            Err(e) => self.send_error_response(&envelope, &e),
                        }
                    }
                    Err(e) => {
                        self.send_error_response(&envelope, &format!("Bad payload: {}", e))
                    }
                }
            }

            "write_file" => {
                match serde_json::from_value::<WriteFilePayload>(envelope.payload.clone()) {
                    Ok(payload) => {
                        let cwd = self.get_cwd();
                        match commands::handle_write_file(&cwd, &payload) {
                            Ok(resp) => self.send_response(&envelope, resp),
                            Err(e) => self.send_error_response(&envelope, &e),
                        }
                    }
                    Err(e) => {
                        self.send_error_response(&envelope, &format!("Bad payload: {}", e))
                    }
                }
            }

            "list_dir" => {
                match serde_json::from_value::<ListDirPayload>(envelope.payload.clone()) {
                    Ok(payload) => {
                        let cwd = self.get_cwd();
                        match commands::handle_list_dir(&cwd, &payload) {
                            Ok(resp) => self.send_response(&envelope, resp),
                            Err(e) => self.send_error_response(&envelope, &e),
                        }
                    }
                    Err(e) => {
                        self.send_error_response(&envelope, &format!("Bad payload: {}", e))
                    }
                }
            }

            _ => {
                self.send_error_response(
                    &envelope,
                    &format!("Unknown command: {}", command),
                );
            }
        }
    }

    /// Queue a pending screenshot capture. The actual screenshot is taken when
    /// the periodic tick determines the output has stabilized.
    fn queue_screenshot(&mut self, request: MessageEnvelope, pane_id: PaneId) {
        self.pending_screenshots.push(PendingScreenshot {
            request,
            pane_id,
            ticks_idle: 0,
            last_content_hash: 0,
        });
    }

    /// Called on PaneRenderReport — reset ticks if output has actually changed.
    fn update_screenshot_hashes(&mut self, pane_contents: &HashMap<PaneId, PaneContents>) {
        for pending in self.pending_screenshots.iter_mut() {
            if let Some(contents) = pane_contents.get(&pending.pane_id) {
                let text = commands::pane_contents_to_text(contents);
                let hash = Self::simple_hash(&text);

                if hash != pending.last_content_hash {
                    pending.last_content_hash = hash;
                    pending.ticks_idle = 0; // Reset idle tracker on change
                }
            }
        }
    }

    /// Called on fast Timer tick — check if any pending screenshots have been idle long enough.
    fn check_screenshot_ticks(&mut self) {
        let mut completed = Vec::new();

        for (i, pending) in self.pending_screenshots.iter_mut().enumerate() {
            pending.ticks_idle += 1;
            if pending.ticks_idle >= self.settle_threshold {
                completed.push(i);
            }
        }

        // Process completed (in reverse to preserve indices)
        for &i in completed.iter().rev() {
            let pending = self.pending_screenshots.remove(i);

            // Capture the screenshot now
            match commands::get_screenshot(pending.pane_id) {
                Ok(screenshot) => {
                    let resp_payload =
                        serde_json::to_value(ScreenshotResponse { screenshot }).unwrap();
                    let resp = MessageEnvelope::response(
                        &pending.request,
                        &self.session_id,
                        resp_payload,
                        None,
                    );
                    self.send_ws(&serde_json::to_string(&resp).unwrap());
                }
                Err(e) => {
                    let resp = MessageEnvelope::response(
                        &pending.request,
                        &self.session_id,
                        serde_json::json!({}),
                        Some(e),
                    );
                    self.send_ws(&serde_json::to_string(&resp).unwrap());
                }
            }
        }
    }

    /// Simple hash function for content change detection.
    fn simple_hash(content: &str) -> u64 {
        let mut hash: u64 = 5381;
        for byte in content.bytes() {
            hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
        }
        hash
    }

    /// Send a success response back via WebSocket.
    fn send_response(
        &self,
        request: &MessageEnvelope,
        payload: serde_json::Value,
    ) {
        let resp = MessageEnvelope::response(request, &self.session_id, payload, None);
        self.send_ws(&serde_json::to_string(&resp).unwrap());
    }

    /// Send an error response back via WebSocket.
    fn send_error_response(
        &self,
        request: &MessageEnvelope,
        error: &str,
    ) {
        let resp = MessageEnvelope::response(
            request,
            &self.session_id,
            serde_json::json!({}),
            Some(error.to_string()),
        );
        self.send_ws(&serde_json::to_string(&resp).unwrap());
    }

    /// Send a heartbeat event.
    fn send_heartbeat(&self) {
        if self.connection_id.is_none() {
            return;
        }

        let cwd = self.get_cwd();
        let shell = self
            .focused_pane
            .and_then(|pid| get_pane_running_command(pid).ok())
            .and_then(|cmd| cmd.first().cloned())
            .and_then(|s| {
                std::path::Path::new(&s)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
            })
            .unwrap_or_else(|| "unknown".to_string());

        let payload = serde_json::to_value(HeartbeatPayload {
            uptime: self.uptime,
            cwd: cwd.to_string_lossy().to_string(),
            shell,
        })
        .unwrap();

        let event = MessageEnvelope::event(&self.session_id, "heartbeat", payload);
        self.send_ws(&serde_json::to_string(&event).unwrap());
    }

    /// Send a help event triggered by screen content detection.
    fn send_help_event(&self, help: HelpEventPayload) {
        if self.connection_id.is_none() {
            return;
        }

        let payload = serde_json::to_value(help).unwrap();
        let event = MessageEnvelope::event(&self.session_id, "help", payload);
        self.send_ws(&serde_json::to_string(&event).unwrap());
    }
}
