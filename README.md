# SpurX Secure SSH

**Version 1.0.0**

SpurX Secure SSH is a modern, cross-platform SSH client built with Rust and Tauri, designed for professional system administrators.

## Features

-   **Modern Interface**: Clean tabs, customizable themes, and sidebar navigation.
-   **Session Management**: Organize servers into folders.
-   **Security**: Encrypted password storage, known_hosts verification, and host key management.
-   **AI Assistant**: Integration with Gemini, Grok, and limited local Ollama support.
-   **SFTP Browser**: Built-in file transfer.
-   **Portability**: Can be run as a portable app or installed locally.

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
