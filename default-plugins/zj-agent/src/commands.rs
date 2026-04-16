use crate::keys;
use crate::protocol::*;
use std::fs;
use std::path::Path;
use zellij_tile::prelude::*;

/// Capture the visible screen content for a pane (plain text, no ANSI).
pub fn get_screenshot(pane_id: PaneId) -> Result<String, String> {
    let contents = get_pane_scrollback(pane_id, false)?;
    Ok(pane_contents_to_text(&contents))
}

/// Convert PaneContents to a single plain-text string (viewport only).
pub fn pane_contents_to_text(contents: &PaneContents) -> String {
    contents
        .viewport
        .iter()
        .map(|line| line.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Handle `send_input`: write chars to pane.
/// The actual screenshot capture is deferred to after render settles.
pub fn handle_send_input(pane_id: PaneId, payload: &SendInputPayload) {
    // repalce "\\n" to "\r"
    let processed_input = payload.input.replace("\\n", "\r");
    write_chars_to_pane_id(&processed_input, pane_id);
}

/// Handle `send_key`: map key name to bytes and write to pane.
pub fn handle_send_key(pane_id: PaneId, payload: &SendKeyPayload) -> Result<(), String> {
    let bytes = keys::key_to_bytes(&payload.key);
    write_to_pane_id(bytes, pane_id);
    Ok(())
}

/// Handle `forward_key`: identical to send_input.
pub fn handle_forward_key(pane_id: PaneId, payload: &SendInputPayload) {
    write_chars_to_pane_id(&payload.input, pane_id);
}

/// Handle `get_cwd`: get the CWD of the active terminal pane.
pub fn handle_get_cwd(pane_id: PaneId) -> Result<serde_json::Value, String> {
    let cwd = get_pane_cwd(pane_id)?;
    Ok(serde_json::to_value(CwdResponse {
        cwd: cwd.to_string_lossy().to_string(),
    })
    .unwrap())
}

/// Handle `get_shell`: get the running command in the active terminal pane.
pub fn handle_get_shell(pane_id: PaneId) -> Result<serde_json::Value, String> {
    let cmd = get_pane_running_command(pane_id)?;
    let shell = cmd.first().cloned().unwrap_or_default();
    // Extract just the binary name
    let shell_name = Path::new(&shell)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or(shell);
    Ok(serde_json::to_value(ShellResponse { shell: shell_name }).unwrap())
}

/// Zellij mounts the host root at `/host` for plugins. This helper maps
/// regular absolute paths into the WASI-friendly `/host/...` structure.
fn map_host_path(path: &Path) -> std::path::PathBuf {
    if path.has_root() && !path.starts_with("/host") {
        if let Ok(stripped) = path.strip_prefix("/") {
            return std::path::Path::new("/host").join(stripped);
        }
    }
    path.to_path_buf()
}

/// Handle `read_file`: read file from host filesystem via std::fs.
pub fn handle_read_file(
    cwd: &Path,
    payload: &ReadFilePayload,
) -> Result<serde_json::Value, String> {
    let mut path = if payload.path.starts_with('/') {
        std::path::PathBuf::from(&payload.path)
    } else {
        cwd.join(&payload.path)
    };
    path = map_host_path(&path);

    let full_content =
        fs::read_to_string(&path).map_err(|e| format!("{}: {}", e, path.display()))?;

    let lines: Vec<&str> = full_content.lines().collect();
    let total_lines = lines.len();

    let start = payload.offset.min(total_lines);
    let end = (start + payload.limit).min(total_lines);
    let selected: String = lines[start..end].join("\n");
    // Add trailing newline if content had one
    let content = if full_content.ends_with('\n') && end == total_lines {
        format!("{}\n", selected)
    } else {
        selected
    };

    Ok(serde_json::to_value(ReadFileResponse {
        content,
        encoding: "utf-8".to_string(),
        total_lines,
    })
    .unwrap())
}

/// Handle `write_file`: write file to host filesystem via std::fs.
pub fn handle_write_file(
    cwd: &Path,
    payload: &WriteFilePayload,
) -> Result<serde_json::Value, String> {
    let mut path = if payload.path.starts_with('/') {
        std::path::PathBuf::from(&payload.path)
    } else {
        cwd.join(&payload.path)
    };
    path = map_host_path(&path);

    // Create parent directories if needed
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directories: {}", e))?;
    }

    let bytes = payload.content.as_bytes();
    fs::write(&path, bytes).map_err(|e| format!("{}: {}", e, path.display()))?;

    Ok(serde_json::to_value(WriteFileResponse {
        success: true,
        bytes_written: bytes.len(),
    })
    .unwrap())
}

/// Handle `list_dir`: list directory contents via std::fs.
pub fn handle_list_dir(
    cwd: &Path,
    payload: &ListDirPayload,
) -> Result<serde_json::Value, String> {
    let mut path = if payload.path.starts_with('/') {
        std::path::PathBuf::from(&payload.path)
    } else {
        cwd.join(&payload.path)
    };
    path = map_host_path(&path);

    let entries_iter =
        fs::read_dir(&path).map_err(|e| format!("{}: {}", e, path.display()))?;

    let mut entries = Vec::new();
    for entry in entries_iter {
        let entry = entry.map_err(|e| format!("Error reading entry: {}", e))?;
        let metadata = entry.metadata().map_err(|e| format!("Error reading metadata: {}", e))?;
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = metadata.is_dir();
        let size = if is_dir { 0 } else { metadata.len() };

        let modified_at = metadata
            .modified()
            .ok()
            .and_then(|t| {
                t.duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .map(|d| {
                        // Simple epoch seconds format
                        format!("{}", d.as_secs())
                    })
            });

        entries.push(DirEntry {
            name,
            is_dir,
            size,
            modified_at,
        });
    }

    // Sort: directories first, then by name
    entries.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name))
    });

    Ok(serde_json::to_value(ListDirResponse { entries }).unwrap())
}
