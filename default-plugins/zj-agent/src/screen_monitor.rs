use std::collections::HashMap;
use zellij_tile::prelude::{PaneContents, PaneId};

use crate::commands::pane_contents_to_text;
use crate::protocol::HelpEventPayload;

/// Tracks per-pane state to avoid duplicate `# help` triggers.
pub struct ScreenMonitor {
    /// Stores the last detected help line per pane to avoid re-triggering.
    last_help_line: HashMap<PaneId, String>,
}

impl ScreenMonitor {
    pub fn new() -> Self {
        Self {
            last_help_line: HashMap::new(),
        }
    }

    /// Scan pane contents for `# help` trigger lines.
    /// Returns a list of (pane_id, HelpEventPayload) for newly detected triggers.
    pub fn scan_for_help(
        &mut self,
        pane_contents: &HashMap<PaneId, PaneContents>,
    ) -> Vec<(PaneId, HelpEventPayload)> {
        let mut triggers = Vec::new();

        for (pane_id, contents) in pane_contents {
            // Only check terminal panes
            if let PaneId::Plugin(_) = pane_id {
                continue;
            }

            let text = pane_contents_to_text(contents);

            if let Some(help) = Self::find_help_trigger(&text) {
                // De-duplicate: only trigger if this content is different from last time
                let trigger_key = format!("{}", text.len());
                let last = self.last_help_line.get(pane_id);
                if last.map_or(true, |l| l != &trigger_key) {
                    self.last_help_line.insert(pane_id.clone(), trigger_key);
                    triggers.push((pane_id.clone(), help));
                }
            }
        }

        triggers
    }

    /// Check if any line in the content ends with `# help` or `# help <prompt>`.
    /// We look at the last non-empty lines (near the cursor) for the trigger.
    fn find_help_trigger(content: &str) -> Option<HelpEventPayload> {
        // Scan lines from bottom up, looking for the most recent trigger
        for line in content.lines().rev() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Match patterns:
            //   "# help" - bare help request
            //   "# help <prompt>" - help with specific prompt
            //   Also match at end of prompt line: "user@host:~$ # help something"
            if let Some(help_text) = extract_help_from_line(trimmed) {
                return Some(help_text);
            }

            // Only check the last few non-empty lines
            break;
        }
        None
    }
}

/// Extract help payload from a line if it contains `# help`.
/// Looks for `# help` appearing in the line (typically at the end after a prompt).
fn extract_help_from_line(line: &str) -> Option<HelpEventPayload> {
    // Find "# help" anywhere in the line
    let marker = "# help";
    let idx = line.find(marker)?;

    let after_marker = &line[idx + marker.len()..];
    let prompt = after_marker.trim().to_string();

    // Grab some context: the text before "# help" (e.g. the prompt/command output)
    let before = line[..idx].trim();
    let context = if before.is_empty() {
        None
    } else {
        Some(before.to_string())
    };

    Some(HelpEventPayload {
        prompt: if prompt.is_empty() {
            "Help me with this terminal session".to_string()
        } else {
            prompt
        },
        context,
    })
}
