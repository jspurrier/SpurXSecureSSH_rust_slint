# SpurX Secure SSH

**Version 2.0.1**

SpurX Secure SSH is a modern, cross-platform SSH client built with Rust and Tauri, designed for professional system administrators.

## Features

-   **Modern Interface**: Clean tabs, flexible tab grouping, customizable themes (Dracula, Solarized, Dark, Light), and collapsible sidebar navigation.
-   **Session Management**: Organize servers into nested folders, import/export configurations easily.
-   **Master Password Lock**: Encrypt and protect your entire saved sessions list and credentials.
-   **Built-in SSH Key Manager**: Generate and manage secure ED25519 and RSA keypairs directly inside the app.
-   **Multi-Tab Command Broadcasting**: Broadcast shell commands to multiple connected sessions simultaneously for parallel administration.
-   **Integrated SFTP Browser**: Seamlessly transfer files with a dual-pane local and remote file browser.
-   **YModem File Transfer**: Transfer files efficiently over active console connection sessions.
-   **AI Assistant**: Direct integration with Gemini (Google AI), Grok (xAI), and local Ollama models (100% private & offline) in the sidebar.
-   **Comprehensive Logging**: Automatically log session outputs with customizable filenames and timestamps.
-   **Security**: Verification of server host keys, OS secure credential manager integration, and host key management.
-   **Portability**: Run as a portable app (`.exe`, `.AppImage`) or install locally.

## User Guide

### 1. Getting Started
-   **Quick Connect**: Click the lightning bolt icon ⚡ in the toolbar. Enter hostname, username, and password.
-   **New Session**: Click the `+` icon in the sidebar to save a session for later.

### 2. Session Management
-   **Folders**: Right-click the sidebar to create folders. Drag and drop sessions to organize.
-   **Import/Export**: File > Import Sessions to migrate from PuTTY or SecureCRT.

### 3. Security & Host Keys
-   **Passwords**: Passwords are stored in your OS's secure credential manager (Windows Credential Manager / macOS Keychain).
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

## Installation
-   **Installer**: Run the provided `.msi` or setup file. It installs to your local user profile (no admin needed).
-   **Portable**: You can also run the `.exe` directly from the release folder without installing.

---
© 2026 SpurX Technologies. All rights reserved.
