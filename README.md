# SpurX Secure SSH

**Version 2.0.11**

SpurX Secure SSH is a modern, cross-platform SSH client built with Rust and Tauri, designed for professional system administrators.

## Features

-   **Modern Interface**: Clean tabs, flexible tab grouping, customizable themes (Dracula, Solarized, Dark, Light), and collapsible sidebar navigation.
-   **SSH Tunnels & Port Forwarding**: Local, Remote (reverse), and Dynamic (SOCKS5 proxy) tunnel management with live connection monitoring.
-   **Session Management**: Organize servers into nested folders, import/export configurations easily.
-   **Master Password Lock**: Encrypt and protect your entire saved sessions list and credentials.
-   **Built-in SSH Key Manager**: Generate and manage secure ED25519, RSA, and ECDSA keypairs directly inside the app.
-   **Multi-Tab Command Broadcasting**: Broadcast shell commands to multiple connected sessions simultaneously for parallel administration.
-   **Integrated SFTP Browser**: Seamlessly transfer files with a dual-pane local and remote file browser.
-   **YModem File Transfer**: Transfer files efficiently over active console connection sessions.
-   **AI Assistant**: Direct integration with Gemini (Google AI), Grok (xAI), and local Ollama models (100% private & offline) in the sidebar.
-   **Comprehensive Logging**: Automatically log session outputs with customizable filenames and timestamps.
-   **Security**: Verification of server host keys, OS secure credential manager integration, and host key management.
-   **Portability & Packaging**: Available as `.msi` (Windows), `.dmg` (macOS), `.deb`, `.rpm`, `.AppImage`, and Flatpak (`com.spurx.securessh`).

## User Guide

### 1. Getting Started
-   **Quick Connect**: Click the lightning bolt icon ⚡ in the toolbar. Enter hostname, username, and password.
-   **New Session**: Click the `+` icon in the sidebar to save a session for later.

### 2. Session Management
-   **Folders**: Right-click the sidebar to create folders. Drag and drop sessions to organize.
-   **Import/Export**: File > Import Sessions to migrate from PuTTY or SecureCRT.

### 3. Security & Host Keys
-   **Passwords**: Passwords are stored in your OS's secure credential manager (Windows Credential Manager / macOS Keychain / Linux Secret Service).
-   **Host Verification**: The client verifies server host keys to prevent MITM attacks.
-   **Changing Hardware**: If a server's key changes (reinstall), go to **Settings > Security**, search for the host, and delete the old key to allow reconnection.

### 4. Configuration Reset
-   **Factory Reset**: If you need to wipe all settings, close the app and delete the configuration folder:
    -   **Windows**: `%APPDATA%\SpurXSecureSSH`
    -   **Linux/Mac**: `~/.spurx-secure-ssh`

### 5. AI Assistant
-   Click the "Sparkles" icon in the toolbar.
-   Configure your API key in the AI panel settings.
-   Ask for help with CLI commands or analyze terminal output.

## Linux & Flatpak Guide

### Flatpak Permissions (Baked In)
The Flatpak package (`com.spurx.securessh`) includes the following pre-configured permissions:
- **Full Network Access**: Required for opening outbound SSH connections and AI API requests.
- **Home & Host Filesystem Access (`--filesystem=home`, `--filesystem=host`)**: Allows seamless access to `~/.ssh/` keys, `~/.spurx-secure-ssh/` session storage, SFTP local browsing, and session logs.
- **Keyring & Secret Service (`--talk-name=org.freedesktop.secrets`)**: Direct integration with KWallet / GNOME Keyring for secure credential storage.

### KDE Plasma Dark Mode & Theming in Flatpak
Flatpaks run inside an isolated runtime container and use GTK for their window chrome. To ensure the window header and controls match KDE Plasma's dark theme:

1. **Install the Breeze GTK Flatpak theme** (matches KDE Plasma's dark titlebars):
   ```bash
   flatpak install org.gtk.Gtk3theme.Breeze
   ```
2. **Force Dark Titlebars Globally for Flatpaks (Optional)**:
   If your KDE desktop uses a dark color scheme and you want all GTK Flatpak apps to inherit dark window decorations:
   ```bash
   flatpak override --user --env=GTK_THEME=Adwaita:dark
   ```
   *(Or `flatpak override --user --env=GTK_THEME=Breeze-Dark` once the Breeze GTK theme is installed).*

## Installation
-   **Windows**: Run the `.msi` setup file or portable `.exe`.
-   **Linux**: Install via `.rpm` (Fedora), `.deb` (Ubuntu/Debian), run `.AppImage`, or build via Flatpak manifest `com.spurx.securessh.yml`.
-   **macOS**: Open the `.dmg` and drag to Applications.

---
© 2026 SpurX Technologies. All rights reserved.
