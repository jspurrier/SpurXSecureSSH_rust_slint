use std::error::Error;

type ClipResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

/// Represents a single command line to be pasted into the terminal session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasteCommand {
    /// The sanitized text of the command line.
    pub text: String,
    /// Whether this line should be submitted with Carriage Return (`\r`).
    pub submit: bool,
}

/// Copies text to the system clipboard across Linux (Wayland/X11), macOS, and Windows.
pub fn set_text(text: impl AsRef<str>) -> ClipResult<()> {
    let content = text.as_ref();

    #[cfg(target_os = "linux")]
    {
        // 1. If running under Wayland or wl-copy is installed, try wl-copy first
        if is_wayland() || has_command("wl-copy") {
            if let Ok(mut child) = std::process::Command::new("wl-copy")
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                if let Some(mut stdin) = child.stdin.take() {
                    use std::io::Write;
                    let _ = stdin.write_all(content.as_bytes());
                }
                if let Ok(status) = child.wait() {
                    if status.success() {
                        return Ok(());
                    }
                }
            }
        }

        // 2. Try xclip if available
        if has_command("xclip") {
            if let Ok(mut child) = std::process::Command::new("xclip")
                .args(["-selection", "clipboard"])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                if let Some(mut stdin) = child.stdin.take() {
                    use std::io::Write;
                    let _ = stdin.write_all(content.as_bytes());
                }
                if let Ok(status) = child.wait() {
                    if status.success() {
                        return Ok(());
                    }
                }
            }
        }

        // 3. Try xsel if available
        if has_command("xsel") {
            if let Ok(mut child) = std::process::Command::new("xsel")
                .args(["--clipboard", "--input"])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                if let Some(mut stdin) = child.stdin.take() {
                    use std::io::Write;
                    let _ = stdin.write_all(content.as_bytes());
                }
                if let Ok(status) = child.wait() {
                    if status.success() {
                        return Ok(());
                    }
                }
            }
        }

        // 4. Try native copypasta (X11 / internal)
        use copypasta::{ClipboardContext, ClipboardProvider};
        if let Ok(mut ctx) = ClipboardContext::new() {
            if ctx.set_contents(content.to_string()).is_ok() {
                return Ok(());
            }
        }

        // 5. Last-ditch wl-copy attempt
        if let Ok(mut child) = std::process::Command::new("wl-copy")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                let _ = stdin.write_all(content.as_bytes());
            }
            if let Ok(status) = child.wait() {
                if status.success() {
                    return Ok(());
                }
            }
        }

        Err("Failed to copy text using wl-copy, xclip, xsel, or native clipboard".into())
    }

    #[cfg(target_os = "macos")]
    {
        use copypasta::{ClipboardContext, ClipboardProvider};
        if let Ok(mut ctx) = ClipboardContext::new() {
            if ctx.set_contents(content.to_string()).is_ok() {
                return Ok(());
            }
        }

        // Fallback: pbcopy command on macOS
        if let Ok(mut child) = std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                let _ = stdin.write_all(content.as_bytes());
            }
            if let Ok(status) = child.wait() {
                if status.success() {
                    return Ok(());
                }
            }
        }

        Err("Failed to set clipboard content on macOS".into())
    }

    #[cfg(target_os = "windows")]
    {
        use copypasta::{ClipboardContext, ClipboardProvider};
        // Retry loop to handle Windows clipboard lock contention
        for _ in 0..3 {
            if let Ok(mut ctx) = ClipboardContext::new() {
                if ctx.set_contents(content.to_string()).is_ok() {
                    return Ok(());
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        // Fallback: clip.exe on Windows
        if let Ok(mut child) = std::process::Command::new("clip")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                let _ = stdin.write_all(content.as_bytes());
            }
            if let Ok(status) = child.wait() {
                if status.success() {
                    return Ok(());
                }
            }
        }

        Err("Failed to set clipboard content on Windows".into())
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        use copypasta::{ClipboardContext, ClipboardProvider};
        let mut ctx = ClipboardContext::new()
            .map_err(|e| format!("Failed to access clipboard: {:?}", e))?;
        ctx.set_contents(content.to_string())
            .map_err(|e| format!("Failed to set clipboard content: {:?}", e))?;
        Ok(())
    }
}

/// Retrieves text from the system clipboard across Linux (Wayland/X11), macOS, and Windows.
pub fn get_text() -> ClipResult<String> {
    #[cfg(target_os = "linux")]
    {
        // 1. If running under Wayland or wl-paste is installed, try wl-paste first.
        // NOTE: We deliberately do NOT pass `--no-newline` so that trailing newlines
        // from editors like KWrite or Notepad are accurately preserved.
        if is_wayland() || has_command("wl-paste") {
            if let Ok(out) = std::process::Command::new("wl-paste").output() {
                if out.status.success() {
                    return Ok(String::from_utf8_lossy(&out.stdout).to_string());
                }
            }
        }

        // 2. Try xclip if available
        if has_command("xclip") {
            if let Ok(out) = std::process::Command::new("xclip")
                .args(["-selection", "clipboard", "-o"])
                .output()
            {
                if out.status.success() {
                    return Ok(String::from_utf8_lossy(&out.stdout).to_string());
                }
            }
        }

        // 3. Try xsel if available
        if has_command("xsel") {
            if let Ok(out) = std::process::Command::new("xsel")
                .args(["--clipboard", "--output"])
                .output()
            {
                if out.status.success() {
                    return Ok(String::from_utf8_lossy(&out.stdout).to_string());
                }
            }
        }

        // 4. Try native copypasta
        use copypasta::{ClipboardContext, ClipboardProvider};
        if let Ok(mut ctx) = ClipboardContext::new() {
            if let Ok(text) = ctx.get_contents() {
                return Ok(text);
            }
        }

        // 5. Last-ditch wl-paste attempt
        if let Ok(out) = std::process::Command::new("wl-paste").output() {
            if out.status.success() {
                return Ok(String::from_utf8_lossy(&out.stdout).to_string());
            }
        }

        Err("Failed to read text using wl-paste, xclip, xsel, or native clipboard".into())
    }

    #[cfg(target_os = "macos")]
    {
        use copypasta::{ClipboardContext, ClipboardProvider};
        if let Ok(mut ctx) = ClipboardContext::new() {
            if let Ok(text) = ctx.get_contents() {
                return Ok(text);
            }
        }

        // Fallback: pbpaste command on macOS
        if let Ok(out) = std::process::Command::new("pbpaste").output() {
            if out.status.success() {
                return Ok(String::from_utf8_lossy(&out.stdout).to_string());
            }
        }

        Err("Failed to read clipboard content on macOS".into())
    }

    #[cfg(target_os = "windows")]
    {
        use copypasta::{ClipboardContext, ClipboardProvider};
        // Retry loop to handle Windows clipboard lock contention
        for _ in 0..3 {
            if let Ok(mut ctx) = ClipboardContext::new() {
                if let Ok(text) = ctx.get_contents() {
                    return Ok(text);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        // Fallback: powershell Get-Clipboard
        if let Ok(out) = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", "Get-Clipboard -Raw"])
            .output()
        {
            if out.status.success() {
                return Ok(String::from_utf8_lossy(&out.stdout).to_string());
            }
        }

        Err("Failed to read clipboard content on Windows".into())
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        use copypasta::{ClipboardContext, ClipboardProvider};
        let mut ctx = ClipboardContext::new()
            .map_err(|e| format!("Failed to access clipboard: {:?}", e))?;
        let text = ctx
            .get_contents()
            .map_err(|e| format!("Failed to read clipboard content: {:?}", e))?;
        Ok(text)
    }
}

/// Sanitizes clipboard text to prevent dangerous or unexpected control characters from
/// breaking network devices (e.g. Cisco IOS, Adtran AOS) or Unix shells.
///
/// Features:
/// 1. Strips ANSI escape sequences.
/// 2. Strips UTF-8 Byte Order Marks (BOM `\u{FEFF}`) and zero-width characters.
/// 3. Normalizes Non-Breaking Spaces (NBSP `\u{00A0}`) and Unicode spaces to ASCII `0x20`.
/// 4. Normalizes smart double/single quotes to ASCII `"` and `'`.
/// 5. Normalizes typographic en/em dashes and minus signs to ASCII `-`.
/// 6. Strips dangerous non-printable ASCII control characters (NUL, Bell, Backspace, ESC),
///    while preserving standard tabs (`\t`) and newlines (`\n`, `\r`).
pub fn clean_clipboard_text(raw: &str) -> String {
    if raw.is_empty() {
        return String::new();
    }

    // 1. Strip ANSI escape codes
    let clean_bytes = strip_ansi_escapes::strip(raw.as_bytes());
    let text = String::from_utf8_lossy(&clean_bytes);

    let mut result = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            // Strip BOM and zero-width characters that break CLI parsers
            '\u{FEFF}' | '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{2060}' | '\u{FE0E}'
            | '\u{FE0F}' => {
                // drop
            }
            // Normalize Non-Breaking Spaces and Unicode whitespace to standard ASCII space
            '\u{00A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}' | '\u{202F}' | '\u{205F}'
            | '\u{3000}' => {
                result.push(' ');
            }
            // Normalize smart / curly double quotes to standard ASCII double quote
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{00AB}' | '\u{00BB}' => {
                result.push('"');
            }
            // Normalize smart / curly single quotes / apostrophes to standard ASCII single quote
            '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{2039}' | '\u{203A}' | '\u{2032}'
            | '\u{2035}' => {
                result.push('\'');
            }
            // Normalize dashes, minus signs, and hyphens to standard ASCII hyphen-minus
            '\u{2013}' | '\u{2014}' | '\u{2212}' | '\u{2010}' | '\u{2011}' => {
                result.push('-');
            }
            // Preserve standard whitespace control characters
            '\t' | '\r' | '\n' => {
                result.push(c);
            }
            // Strip other non-printable ASCII control characters (0x00-0x08, 0x0B-0x0C, 0x0E-0x1F, 0x7F) and ESC
            c if c.is_ascii_control() => {
                // drop dangerous control characters like NUL, Bell, Backspace, ESC
            }
            // Keep all other printable unicode / ascii characters
            _ => {
                result.push(c);
            }
        }
    }

    result
}

/// Prepares clipboard text into structured commands ready to be submitted line-by-line.
///
/// Handles:
/// - Windows CRLF (`\r\n`), Linux LF (`\n`), and legacy Mac CR (`\r`) newlines.
/// - Single-line paste without newline: `submit: false` (allows editing before pressing Enter).
/// - Single-line paste with newline: `submit: true` (executes command immediately).
/// - Multi-line configuration blocks (from Notepad, KWrite, etc.): each line is submitted
///   as an individual command (`submit: true`), trimming trailing whitespace and ignoring
///   phantom trailing empty lines.
pub fn prepare_paste_commands(raw: &str) -> Vec<PasteCommand> {
    let cleaned = clean_clipboard_text(raw);
    if cleaned.is_empty() {
        return Vec::new();
    }

    let has_any_newline = cleaned.contains('\n') || cleaned.contains('\r');
    let ends_with_newline = cleaned.ends_with('\n') || cleaned.ends_with('\r');

    // Normalize all line breaks to LF
    let normalized = cleaned.replace("\r\n", "\n").replace('\r', "\n");

    // Single line without any newline (e.g. "show version" or "10.0.0.1")
    if !has_any_newline {
        return vec![PasteCommand {
            text: normalized,
            submit: false,
        }];
    }

    let split_lines: Vec<&str> = normalized.split('\n').collect();

    // If the text ended with a newline, the last element from split('\n') is empty (the terminator)
    let num_lines = split_lines.len();
    let effective_lines =
        if ends_with_newline && num_lines > 1 && split_lines[num_lines - 1].is_empty() {
            &split_lines[..num_lines - 1]
        } else {
            &split_lines[..]
        };

    let mut commands = Vec::with_capacity(effective_lines.len());
    let mut last_was_empty = false;

    for line in effective_lines {
        // Trim trailing whitespace and carriage returns so commands don't carry trailing garbage before \r
        let trimmed = line.trim_end_matches(['\r', ' ', '\t']);

        if trimmed.is_empty() {
            // Avoid spamming consecutive empty enters; preserve single empty enter
            if !last_was_empty && !commands.is_empty() {
                commands.push(PasteCommand {
                    text: String::new(),
                    submit: true,
                });
                last_was_empty = true;
            }
            continue;
        }

        last_was_empty = false;
        commands.push(PasteCommand {
            text: trimmed.to_string(),
            submit: true, // Every line in a multi-line block submits with \r!
        });
    }

    commands
}

/// Formats text for copying back to the system clipboard, using the native line ending
/// convention for the target OS (`\r\n` on Windows for Notepad, `\n` on Linux/macOS for KWrite).
pub fn format_text_for_system_clipboard(text: &str) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    #[cfg(target_os = "windows")]
    {
        normalized.replace('\n', "\r\n")
    }
    #[cfg(not(target_os = "windows"))]
    {
        normalized
    }
}

#[cfg(target_os = "linux")]
fn is_wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE")
            .map(|s| s.eq_ignore_ascii_case("wayland"))
            .unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn has_command(cmd: &str) -> bool {
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let full_path = dir.join(cmd);
            if full_path.is_file() {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kwrite_multiline_paste_lf() {
        let input = "configure terminal\ninterface GigabitEthernet0/1\nip address 192.168.1.1 255.255.255.0\nno shutdown\nexit\n";
        let cmds = prepare_paste_commands(input);
        assert_eq!(cmds.len(), 5);
        assert_eq!(cmds[0].text, "configure terminal");
        assert!(cmds[0].submit);
        assert_eq!(cmds[1].text, "interface GigabitEthernet0/1");
        assert!(cmds[1].submit);
        assert_eq!(cmds[2].text, "ip address 192.168.1.1 255.255.255.0");
        assert!(cmds[2].submit);
        assert_eq!(cmds[3].text, "no shutdown");
        assert!(cmds[3].submit);
        assert_eq!(cmds[4].text, "exit");
        assert!(cmds[4].submit);
    }

    #[test]
    fn test_notepad_multiline_paste_crlf() {
        let input = "hostname Router\r\ninterface vlan 1\r\nip address 10.0.0.1 255.255.255.0\r\nno shut\r\n";
        let cmds = prepare_paste_commands(input);
        assert_eq!(cmds.len(), 4);
        assert_eq!(cmds[0].text, "hostname Router");
        assert!(cmds[0].submit);
        assert_eq!(cmds[1].text, "interface vlan 1");
        assert!(cmds[1].submit);
        assert_eq!(cmds[2].text, "ip address 10.0.0.1 255.255.255.0");
        assert!(cmds[2].submit);
        assert_eq!(cmds[3].text, "no shut");
        assert!(cmds[3].submit);
    }

    #[test]
    fn test_multiline_block_without_trailing_newline() {
        let input = "interface Gi0/1\n no shutdown";
        let cmds = prepare_paste_commands(input);
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0].text, "interface Gi0/1");
        assert!(cmds[0].submit);
        assert_eq!(cmds[1].text, " no shutdown");
        assert!(cmds[1].submit);
    }

    #[test]
    fn test_single_line_without_newline() {
        let input = "show ip interface brief";
        let cmds = prepare_paste_commands(input);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].text, "show ip interface brief");
        assert!(!cmds[0].submit);
    }

    #[test]
    fn test_single_line_with_newline() {
        let input = "show version\n";
        let cmds = prepare_paste_commands(input);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].text, "show version");
        assert!(cmds[0].submit);
    }

    #[test]
    fn test_sanitize_bom_and_zero_width() {
        let input = "\u{FEFF}configure terminal\u{200B}\u{200C}\u{200D}";
        let cleaned = clean_clipboard_text(input);
        assert_eq!(cleaned, "configure terminal");
    }

    #[test]
    fn test_sanitize_nbsp_and_smart_quotes_and_dashes() {
        let input = "description\u{00A0}“Uplink—Core”\u{00A0}‘Active’";
        let cleaned = clean_clipboard_text(input);
        assert_eq!(cleaned, "description \"Uplink-Core\" 'Active'");
    }

    #[test]
    fn test_sanitize_ansi_escapes_and_control_chars() {
        let input = "\x1b[32mshow\x1b[0m \x07version\x00\r\n";
        let cmds = prepare_paste_commands(input);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].text, "show version");
        assert!(cmds[0].submit);
    }

    #[test]
    fn test_intermediate_blank_lines() {
        let input = "interface Gi0/1\n\n\ninterface Gi0/2\n";
        let cmds = prepare_paste_commands(input);
        assert_eq!(cmds.len(), 3);
        assert_eq!(cmds[0].text, "interface Gi0/1");
        assert!(cmds[0].submit);
        assert_eq!(cmds[1].text, "");
        assert!(cmds[1].submit);
        assert_eq!(cmds[2].text, "interface Gi0/2");
        assert!(cmds[2].submit);
    }
}
