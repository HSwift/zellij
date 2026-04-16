/// Map key names from the protocol to terminal byte sequences.
pub fn key_to_bytes(key_name: &str) -> Vec<u8> {
    match key_name {
        // Basic keys
        "Enter" => vec![b'\r'],
        "Tab" => vec![b'\t'],
        "Escape" => vec![0x1b],
        "Backspace" => vec![0x7f],

        // Arrow keys (standard ANSI)
        "Up" => b"\x1b[A".to_vec(),
        "Down" => b"\x1b[B".to_vec(),
        "Right" => b"\x1b[C".to_vec(),
        "Left" => b"\x1b[D".to_vec(),

        // Navigation
        "Home" => b"\x1b[H".to_vec(),
        "End" => b"\x1b[F".to_vec(),
        "PageUp" => b"\x1b[5~".to_vec(),
        "PageDown" => b"\x1b[6~".to_vec(),

        // Ctrl combos
        "Ctrl-A" => vec![0x01],
        "Ctrl-B" => vec![0x02],
        "Ctrl-C" => vec![0x03],
        "Ctrl-D" => vec![0x04],
        "Ctrl-E" => vec![0x05],
        "Ctrl-F" => vec![0x06],
        "Ctrl-G" => vec![0x07],
        "Ctrl-H" => vec![0x08],
        "Ctrl-K" => vec![0x0b],
        "Ctrl-L" => vec![0x0c],
        "Ctrl-N" => vec![0x0e],
        "Ctrl-P" => vec![0x10],
        "Ctrl-R" => vec![0x12],
        "Ctrl-U" => vec![0x15],
        "Ctrl-W" => vec![0x17],
        "Ctrl-Z" => vec![0x1a],

        // Function keys (xterm-style)
        "F1" => b"\x1bOP".to_vec(),
        "F2" => b"\x1bOQ".to_vec(),
        "F3" => b"\x1bOR".to_vec(),
        "F4" => b"\x1bOS".to_vec(),
        "F5" => b"\x1b[15~".to_vec(),
        "F6" => b"\x1b[17~".to_vec(),
        "F7" => b"\x1b[18~".to_vec(),
        "F8" => b"\x1b[19~".to_vec(),
        "F9" => b"\x1b[20~".to_vec(),
        "F10" => b"\x1b[21~".to_vec(),
        "F11" => b"\x1b[23~".to_vec(),
        "F12" => b"\x1b[24~".to_vec(),

        _ => key_name.as_bytes().to_vec(),
    }
}
