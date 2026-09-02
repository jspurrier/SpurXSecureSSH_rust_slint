use std::error::Error;

type ClipResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

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

    #[cfg(not(target_os = "linux"))]
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
        // 1. If running under Wayland or wl-paste is installed, try wl-paste first
        if is_wayland() || has_command("wl-paste") {
            if let Ok(out) = std::process::Command::new("wl-paste")
                .arg("--no-newline")
                .output()
            {
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
        if let Ok(out) = std::process::Command::new("wl-paste")
            .arg("--no-newline")
            .output()
        {
            if out.status.success() {
                return Ok(String::from_utf8_lossy(&out.stdout).to_string());
            }
        }

        Err("Failed to read text using wl-paste, xclip, xsel, or native clipboard".into())
    }

    #[cfg(not(target_os = "linux"))]
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
