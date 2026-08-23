use slint::Color;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    Light,
    System,
}

impl ThemeMode {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "light" => ThemeMode::Light,
            "system" => ThemeMode::System,
            _ => ThemeMode::Dark,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ThemeMode::Dark => "dark",
            ThemeMode::Light => "light",
            ThemeMode::System => "system",
        }
    }
}

/// Detects the OS / desktop environment dark or light theme preference.
pub fn detect_system_is_dark() -> bool {
    #[cfg(target_os = "linux")]
    {
        // 1. Try XDG Desktop Portal via dbus (standard across GNOME, KDE, wlroots, XFCE)
        if let Ok(output) = Command::new("dbus-send")
            .args([
                "--reply-timeout=200",
                "--print-reply=literal",
                "--dest=org.freedesktop.portal.Desktop",
                "/org/freedesktop/portal/desktop",
                "org.freedesktop.portal.Settings.Read",
                "string:org.freedesktop.appearance",
                "string:color-scheme",
            ])
            .output()
        {
            let out_str = String::from_utf8_lossy(&output.stdout);
            if out_str.contains("uint32 1") {
                return true; // 1 = Prefer Dark
            } else if out_str.contains("uint32 2") {
                return false; // 2 = Prefer Light
            }
        }

        // 2. Try GNOME / Cinnamon / MATE gsettings
        if let Ok(output) = Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "color-scheme"])
            .output()
        {
            let out_str = String::from_utf8_lossy(&output.stdout);
            if out_str.contains("prefer-dark") {
                return true;
            } else if out_str.contains("prefer-light") {
                return false;
            }
        }

        // 3. Try KDE Plasma kreadconfig
        for cmd in &["kreadconfig6", "kreadconfig5"] {
            if let Ok(output) = Command::new(cmd)
                .args(["--group", "General", "--key", "ColorScheme"])
                .output()
            {
                let out_str = String::from_utf8_lossy(&output.stdout).to_lowercase();
                if !out_str.trim().is_empty() {
                    if out_str.contains("dark")
                        || out_str.contains("black")
                        || out_str.contains("breeze-dark")
                    {
                        return true;
                    } else if out_str.contains("light") || out_str.contains("white") {
                        return false;
                    }
                }
            }
        }

        // 4. Check GTK_THEME environment variable
        if let Ok(gtk_theme) = std::env::var("GTK_THEME") {
            let lower = gtk_theme.to_lowercase();
            if lower.contains("dark") {
                return true;
            } else if lower.contains("light") {
                return false;
            }
        }

        // Default to dark on Linux
        true
    }

    #[cfg(target_os = "windows")]
    {
        // Check Windows registry for AppsUseLightTheme
        if let Ok(output) = Command::new("reg")
            .args([
                "query",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
                "/v",
                "AppsUseLightTheme",
            ])
            .output()
        {
            let out_str = String::from_utf8_lossy(&output.stdout);
            if out_str.contains("0x0") {
                return true; // 0 = Dark mode
            } else if out_str.contains("0x1") {
                return false; // 1 = Light mode
            }
        }
        true
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = Command::new("defaults")
            .args(["read", "-g", "AppleInterfaceStyle"])
            .output()
        {
            let out_str = String::from_utf8_lossy(&output.stdout);
            return out_str.trim() == "Dark";
        }
        false
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        true
    }
}

pub fn is_preset_dark(name: &str) -> bool {
    match name.to_lowercase().as_str() {
        "clean light"
        | "paper white"
        | "solarized light"
        | "nord light"
        | "github light"
        | "catppuccin latte"
        | "sepia warm"
        | "rose pine dawn"
        | "high contrast light" => false,
        _ => true,
    }
}

pub fn get_effective_scheme(scheme_str: &str, is_dark: bool) -> String {
    let scheme_is_dark = is_preset_dark(scheme_str);
    if is_dark && !scheme_is_dark {
        "GitHub Dark".to_string()
    } else if !is_dark && scheme_is_dark {
        "Clean Light".to_string()
    } else {
        scheme_str.to_string()
    }
}

#[derive(Debug, Clone)]
pub struct ColorPalette {
    pub is_dark: bool,
    pub bg_primary: (u8, u8, u8),
    pub bg_secondary: (u8, u8, u8),
    pub bg_tertiary: (u8, u8, u8),
    pub bg_elevated: (u8, u8, u8),
    pub bg_hover: (u8, u8, u8),
    pub bg_active: (u8, u8, u8),

    pub accent_primary: (u8, u8, u8),
    pub accent_secondary: (u8, u8, u8),
    pub accent_success: (u8, u8, u8),
    pub accent_warning: (u8, u8, u8),
    pub accent_danger: (u8, u8, u8),

    pub text_primary: (u8, u8, u8),
    pub text_secondary: (u8, u8, u8),
    pub text_muted: (u8, u8, u8),
    pub text_on_accent: (u8, u8, u8),

    pub border_primary: (u8, u8, u8),
    pub border_secondary: (u8, u8, u8),
    pub border_accent: (u8, u8, u8, u8),

    pub term_bg: (u8, u8, u8),
    pub term_fg: (u8, u8, u8),
    pub term_cursor: (u8, u8, u8),
    pub term_selection: (u8, u8, u8),
    pub term_black: (u8, u8, u8),
    pub term_red: (u8, u8, u8),
    pub term_green: (u8, u8, u8),
    pub term_yellow: (u8, u8, u8),
    pub term_blue: (u8, u8, u8),
    pub term_magenta: (u8, u8, u8),
    pub term_cyan: (u8, u8, u8),
    pub term_white: (u8, u8, u8),
}

fn hex_to_rgb(hex: u32) -> (u8, u8, u8) {
    let r = ((hex >> 16) & 0xFF) as u8;
    let g = ((hex >> 8) & 0xFF) as u8;
    let b = (hex & 0xFF) as u8;
    (r, g, b)
}

pub fn get_palette_by_name(name: &str, is_dark: bool) -> ColorPalette {
    match name.to_lowercase().as_str() {
        // ==========================================
        // 9 DARK THEMES
        // ==========================================

        // 1. GitHub Dark (Default)
        "github dark" | "dark" | "default" => ColorPalette {
            is_dark: true,
            bg_primary: hex_to_rgb(0x0a0e14),
            bg_secondary: hex_to_rgb(0x0f1419),
            bg_tertiary: hex_to_rgb(0x151b23),
            bg_elevated: hex_to_rgb(0x1a222d),
            bg_hover: hex_to_rgb(0x232d3b),
            bg_active: hex_to_rgb(0x2d3a4d),
            accent_primary: hex_to_rgb(0x00d4ff),
            accent_secondary: hex_to_rgb(0x7c3aed),
            accent_success: hex_to_rgb(0x10b981),
            accent_warning: hex_to_rgb(0xf59e0b),
            accent_danger: hex_to_rgb(0xef4444),
            text_primary: hex_to_rgb(0xe6edf3),
            text_secondary: hex_to_rgb(0x8b949e),
            text_muted: hex_to_rgb(0x6e7681),
            text_on_accent: hex_to_rgb(0x000000),
            border_primary: hex_to_rgb(0x30363d),
            border_secondary: hex_to_rgb(0x21262d),
            border_accent: (0x00, 0xd4, 0xff, 0x4d),
            term_bg: hex_to_rgb(0x0d1117),
            term_fg: hex_to_rgb(0xc9d1d9),
            term_cursor: hex_to_rgb(0x58a6ff),
            term_selection: hex_to_rgb(0x264f78),
            term_black: hex_to_rgb(0x0d1117),
            term_red: hex_to_rgb(0xff7b72),
            term_green: hex_to_rgb(0x3fb950),
            term_yellow: hex_to_rgb(0xd29922),
            term_blue: hex_to_rgb(0x58a6ff),
            term_magenta: hex_to_rgb(0xbc8cff),
            term_cyan: hex_to_rgb(0x39c5cf),
            term_white: hex_to_rgb(0xc9d1d9),
        },

        // 2. Midnight Navy
        "midnight navy" | "midnight" | "deep blue" => ColorPalette {
            is_dark: true,
            bg_primary: hex_to_rgb(0x080c16),
            bg_secondary: hex_to_rgb(0x0d1527),
            bg_tertiary: hex_to_rgb(0x121e36),
            bg_elevated: hex_to_rgb(0x172747),
            bg_hover: hex_to_rgb(0x1e335c),
            bg_active: hex_to_rgb(0x264175),
            accent_primary: hex_to_rgb(0x38bdf8),
            accent_secondary: hex_to_rgb(0x818cf8),
            accent_success: hex_to_rgb(0x34d399),
            accent_warning: hex_to_rgb(0xfbbf24),
            accent_danger: hex_to_rgb(0xf87171),
            text_primary: hex_to_rgb(0xf0f6fc),
            text_secondary: hex_to_rgb(0x93a4be),
            text_muted: hex_to_rgb(0x64748b),
            text_on_accent: hex_to_rgb(0x030712),
            border_primary: hex_to_rgb(0x1e2f4d),
            border_secondary: hex_to_rgb(0x162238),
            border_accent: (0x38, 0xbd, 0xf8, 0x4d),
            term_bg: hex_to_rgb(0x070b14),
            term_fg: hex_to_rgb(0xe2e8f0),
            term_cursor: hex_to_rgb(0x38bdf8),
            term_selection: hex_to_rgb(0x1d4ed8),
            term_black: hex_to_rgb(0x0a0f1d),
            term_red: hex_to_rgb(0xf87171),
            term_green: hex_to_rgb(0x4ade80),
            term_yellow: hex_to_rgb(0xfacc15),
            term_blue: hex_to_rgb(0x60a5fa),
            term_magenta: hex_to_rgb(0xc084fc),
            term_cyan: hex_to_rgb(0x38bdf8),
            term_white: hex_to_rgb(0xf1f5f9),
        },

        // 3. Cyberpunk
        "cyberpunk" | "neon" => ColorPalette {
            is_dark: true,
            bg_primary: hex_to_rgb(0x08070e),
            bg_secondary: hex_to_rgb(0x0e0d18),
            bg_tertiary: hex_to_rgb(0x161424),
            bg_elevated: hex_to_rgb(0x1f1c33),
            bg_hover: hex_to_rgb(0x2d294a),
            bg_active: hex_to_rgb(0x3b3561),
            accent_primary: hex_to_rgb(0x06b6d4),
            accent_secondary: hex_to_rgb(0xc084fc),
            accent_success: hex_to_rgb(0x10b981),
            accent_warning: hex_to_rgb(0xf59e0b),
            accent_danger: hex_to_rgb(0xf43f5e),
            text_primary: hex_to_rgb(0xf3f4f6),
            text_secondary: hex_to_rgb(0xa78bfa),
            text_muted: hex_to_rgb(0x6b7280),
            text_on_accent: hex_to_rgb(0x000000),
            border_primary: hex_to_rgb(0x2e294e),
            border_secondary: hex_to_rgb(0x1e1b33),
            border_accent: (0x06, 0xb6, 0xd4, 0x5d),
            term_bg: hex_to_rgb(0x0a0912),
            term_fg: hex_to_rgb(0xf3e8ff),
            term_cursor: hex_to_rgb(0xc084fc),
            term_selection: hex_to_rgb(0x4c1d95),
            term_black: hex_to_rgb(0x0e0d18),
            term_red: hex_to_rgb(0xf43f5e),
            term_green: hex_to_rgb(0x34d399),
            term_yellow: hex_to_rgb(0xfbbf24),
            term_blue: hex_to_rgb(0x818cf8),
            term_magenta: hex_to_rgb(0xe879f9),
            term_cyan: hex_to_rgb(0x22d3ee),
            term_white: hex_to_rgb(0xf5f3ff),
        },

        // 4. Emerald Matrix
        "emerald matrix" | "matrix" | "hacker" => ColorPalette {
            is_dark: true,
            bg_primary: hex_to_rgb(0x060d09),
            bg_secondary: hex_to_rgb(0x0b1710),
            bg_tertiary: hex_to_rgb(0x102218),
            bg_elevated: hex_to_rgb(0x172f22),
            bg_hover: hex_to_rgb(0x1f3e2d),
            bg_active: hex_to_rgb(0x28503a),
            accent_primary: hex_to_rgb(0x10b981),
            accent_secondary: hex_to_rgb(0x06b6d4),
            accent_success: hex_to_rgb(0x22c55e),
            accent_warning: hex_to_rgb(0xeab308),
            accent_danger: hex_to_rgb(0xef4444),
            text_primary: hex_to_rgb(0xe2fdf0),
            text_secondary: hex_to_rgb(0x6ee7b7),
            text_muted: hex_to_rgb(0x4b7c65),
            text_on_accent: hex_to_rgb(0x022c22),
            border_primary: hex_to_rgb(0x193c2b),
            border_secondary: hex_to_rgb(0x0f271c),
            border_accent: (0x10, 0xb9, 0x81, 0x50),
            term_bg: hex_to_rgb(0x050a07),
            term_fg: hex_to_rgb(0x86efac),
            term_cursor: hex_to_rgb(0x22c55e),
            term_selection: hex_to_rgb(0x064e3b),
            term_black: hex_to_rgb(0x050a07),
            term_red: hex_to_rgb(0xf87171),
            term_green: hex_to_rgb(0x4ade80),
            term_yellow: hex_to_rgb(0xfacc15),
            term_blue: hex_to_rgb(0x38bdf8),
            term_magenta: hex_to_rgb(0xc084fc),
            term_cyan: hex_to_rgb(0x2dd4bf),
            term_white: hex_to_rgb(0xdcfce7),
        },

        // 5. Tokyo Night
        "tokyo night" | "tokyo" => ColorPalette {
            is_dark: true,
            bg_primary: hex_to_rgb(0x16161e),
            bg_secondary: hex_to_rgb(0x1a1b26),
            bg_tertiary: hex_to_rgb(0x24283b),
            bg_elevated: hex_to_rgb(0x2f354f),
            bg_hover: hex_to_rgb(0x3b4261),
            bg_active: hex_to_rgb(0x48527a),
            accent_primary: hex_to_rgb(0x7aa2f7),
            accent_secondary: hex_to_rgb(0xbb9af7),
            accent_success: hex_to_rgb(0x9ece6a),
            accent_warning: hex_to_rgb(0xe0af68),
            accent_danger: hex_to_rgb(0xf7768e),
            text_primary: hex_to_rgb(0xc0caf5),
            text_secondary: hex_to_rgb(0x9aa5ce),
            text_muted: hex_to_rgb(0x565f89),
            text_on_accent: hex_to_rgb(0x1a1b26),
            border_primary: hex_to_rgb(0x292e42),
            border_secondary: hex_to_rgb(0x1f2335),
            border_accent: (0x7a, 0xa2, 0xf7, 0x50),
            term_bg: hex_to_rgb(0x1a1b26),
            term_fg: hex_to_rgb(0xc0caf5),
            term_cursor: hex_to_rgb(0x7aa2f7),
            term_selection: hex_to_rgb(0x33467c),
            term_black: hex_to_rgb(0x15161e),
            term_red: hex_to_rgb(0xf7768e),
            term_green: hex_to_rgb(0x9ece6a),
            term_yellow: hex_to_rgb(0xe0af68),
            term_blue: hex_to_rgb(0x7aa2f7),
            term_magenta: hex_to_rgb(0xbb9af7),
            term_cyan: hex_to_rgb(0x7dcfff),
            term_white: hex_to_rgb(0xa9b1d6),
        },

        // 6. Nord Slate
        "nord slate" | "nord" => ColorPalette {
            is_dark: true,
            bg_primary: hex_to_rgb(0x181c24),
            bg_secondary: hex_to_rgb(0x242933),
            bg_tertiary: hex_to_rgb(0x2e3440),
            bg_elevated: hex_to_rgb(0x3b4252),
            bg_hover: hex_to_rgb(0x434c5e),
            bg_active: hex_to_rgb(0x4c566a),
            accent_primary: hex_to_rgb(0x88c0d0),
            accent_secondary: hex_to_rgb(0x81a1c1),
            accent_success: hex_to_rgb(0xa3be8c),
            accent_warning: hex_to_rgb(0xebcb8b),
            accent_danger: hex_to_rgb(0xbf616a),
            text_primary: hex_to_rgb(0xeceff4),
            text_secondary: hex_to_rgb(0xd8dee9),
            text_muted: hex_to_rgb(0x7b88a1),
            text_on_accent: hex_to_rgb(0x2e3440),
            border_primary: hex_to_rgb(0x3b4252),
            border_secondary: hex_to_rgb(0x2e3440),
            border_accent: (0x88, 0xc0, 0xd0, 0x50),
            term_bg: hex_to_rgb(0x242933),
            term_fg: hex_to_rgb(0xeceff4),
            term_cursor: hex_to_rgb(0x88c0d0),
            term_selection: hex_to_rgb(0x434c5e),
            term_black: hex_to_rgb(0x2e3440),
            term_red: hex_to_rgb(0xbf616a),
            term_green: hex_to_rgb(0xa3be8c),
            term_yellow: hex_to_rgb(0xebcb8b),
            term_blue: hex_to_rgb(0x81a1c1),
            term_magenta: hex_to_rgb(0xb48ead),
            term_cyan: hex_to_rgb(0x88c0d0),
            term_white: hex_to_rgb(0xe5e9f0),
        },

        // 7. Dracula
        "dracula" => ColorPalette {
            is_dark: true,
            bg_primary: hex_to_rgb(0x1e1f29),
            bg_secondary: hex_to_rgb(0x282a36),
            bg_tertiary: hex_to_rgb(0x343746),
            bg_elevated: hex_to_rgb(0x44475a),
            bg_hover: hex_to_rgb(0x5a5d7a),
            bg_active: hex_to_rgb(0x6272a4),
            accent_primary: hex_to_rgb(0xff79c6),
            accent_secondary: hex_to_rgb(0xbd93f9),
            accent_success: hex_to_rgb(0x50fa7b),
            accent_warning: hex_to_rgb(0xf1fa8c),
            accent_danger: hex_to_rgb(0xff5555),
            text_primary: hex_to_rgb(0xf8f8f2),
            text_secondary: hex_to_rgb(0xbfbfbf),
            text_muted: hex_to_rgb(0x6272a4),
            text_on_accent: hex_to_rgb(0x282a36),
            border_primary: hex_to_rgb(0x44475a),
            border_secondary: hex_to_rgb(0x282a36),
            border_accent: (0xff, 0x79, 0xc6, 0x50),
            term_bg: hex_to_rgb(0x282a36),
            term_fg: hex_to_rgb(0xf8f8f2),
            term_cursor: hex_to_rgb(0xff79c6),
            term_selection: hex_to_rgb(0x44475a),
            term_black: hex_to_rgb(0x21222c),
            term_red: hex_to_rgb(0xff5555),
            term_green: hex_to_rgb(0x50fa7b),
            term_yellow: hex_to_rgb(0xf1fa8c),
            term_blue: hex_to_rgb(0xbd93f9),
            term_magenta: hex_to_rgb(0xff79c6),
            term_cyan: hex_to_rgb(0x8be9fd),
            term_white: hex_to_rgb(0xf8f8f2),
        },

        // 8. Monokai Pro
        "monokai pro" | "monokai" => ColorPalette {
            is_dark: true,
            bg_primary: hex_to_rgb(0x19181a),
            bg_secondary: hex_to_rgb(0x221f22),
            bg_tertiary: hex_to_rgb(0x2d2a2e),
            bg_elevated: hex_to_rgb(0x3a363b),
            bg_hover: hex_to_rgb(0x49444a),
            bg_active: hex_to_rgb(0x5b565d),
            accent_primary: hex_to_rgb(0xffd866),
            accent_secondary: hex_to_rgb(0xab9df2),
            accent_success: hex_to_rgb(0xa9dc76),
            accent_warning: hex_to_rgb(0xfc9867),
            accent_danger: hex_to_rgb(0xff6188),
            text_primary: hex_to_rgb(0xfcfcfa),
            text_secondary: hex_to_rgb(0xc1c0c0),
            text_muted: hex_to_rgb(0x727072),
            text_on_accent: hex_to_rgb(0x19181a),
            border_primary: hex_to_rgb(0x3a363b),
            border_secondary: hex_to_rgb(0x2d2a2e),
            border_accent: (0xff, 0xd8, 0x66, 0x50),
            term_bg: hex_to_rgb(0x221f22),
            term_fg: hex_to_rgb(0xfcfcfa),
            term_cursor: hex_to_rgb(0xffd866),
            term_selection: hex_to_rgb(0x403e41),
            term_black: hex_to_rgb(0x19181a),
            term_red: hex_to_rgb(0xff6188),
            term_green: hex_to_rgb(0xa9dc76),
            term_yellow: hex_to_rgb(0xffd866),
            term_blue: hex_to_rgb(0x78dce8),
            term_magenta: hex_to_rgb(0xab9df2),
            term_cyan: hex_to_rgb(0x78dce8),
            term_white: hex_to_rgb(0xfcfcfa),
        },

        // 9. Solarized Dark
        "solarized dark" => ColorPalette {
            is_dark: true,
            bg_primary: hex_to_rgb(0x00212b),
            bg_secondary: hex_to_rgb(0x002b36),
            bg_tertiary: hex_to_rgb(0x073642),
            bg_elevated: hex_to_rgb(0x0e4756),
            bg_hover: hex_to_rgb(0x15586b),
            bg_active: hex_to_rgb(0x1c6a80),
            accent_primary: hex_to_rgb(0x2aa198),
            accent_secondary: hex_to_rgb(0x268bd2),
            accent_success: hex_to_rgb(0x859900),
            accent_warning: hex_to_rgb(0xb58900),
            accent_danger: hex_to_rgb(0xdc322f),
            text_primary: hex_to_rgb(0x93a1a1),
            text_secondary: hex_to_rgb(0x839496),
            text_muted: hex_to_rgb(0x586e75),
            text_on_accent: hex_to_rgb(0x002b36),
            border_primary: hex_to_rgb(0x073642),
            border_secondary: hex_to_rgb(0x002b36),
            border_accent: (0x2a, 0xa1, 0x98, 0x50),
            term_bg: hex_to_rgb(0x002b36),
            term_fg: hex_to_rgb(0x839496),
            term_cursor: hex_to_rgb(0x2aa198),
            term_selection: hex_to_rgb(0x073642),
            term_black: hex_to_rgb(0x073642),
            term_red: hex_to_rgb(0xdc322f),
            term_green: hex_to_rgb(0x859900),
            term_yellow: hex_to_rgb(0xb58900),
            term_blue: hex_to_rgb(0x268bd2),
            term_magenta: hex_to_rgb(0xd33682),
            term_cyan: hex_to_rgb(0x2aa198),
            term_white: hex_to_rgb(0xeee8d5),
        },

        // ==========================================
        // 9 LIGHT THEMES
        // ==========================================

        // 1. Clean Light (Default Light)
        "clean light" | "light" => ColorPalette {
            is_dark: false,
            bg_primary: hex_to_rgb(0xf8fafc),
            bg_secondary: hex_to_rgb(0xffffff),
            bg_tertiary: hex_to_rgb(0xf1f5f9),
            bg_elevated: hex_to_rgb(0xe2e8f0),
            bg_hover: hex_to_rgb(0xe2e8f0),
            bg_active: hex_to_rgb(0xcbd5e1),
            accent_primary: hex_to_rgb(0x0284c7),
            accent_secondary: hex_to_rgb(0x7c3aed),
            accent_success: hex_to_rgb(0x16a34a),
            accent_warning: hex_to_rgb(0xd97706),
            accent_danger: hex_to_rgb(0xdc2626),
            text_primary: hex_to_rgb(0x0f172a),
            text_secondary: hex_to_rgb(0x475569),
            text_muted: hex_to_rgb(0x94a3b8),
            text_on_accent: hex_to_rgb(0xffffff),
            border_primary: hex_to_rgb(0xcbd5e1),
            border_secondary: hex_to_rgb(0xe2e8f0),
            border_accent: (0x02, 0x84, 0xc7, 0x60),
            term_bg: hex_to_rgb(0xffffff),
            term_fg: hex_to_rgb(0x1e293b),
            term_cursor: hex_to_rgb(0x0284c7),
            term_selection: hex_to_rgb(0xbae6fd),
            term_black: hex_to_rgb(0x0f172a),
            term_red: hex_to_rgb(0xdc2626),
            term_green: hex_to_rgb(0x16a34a),
            term_yellow: hex_to_rgb(0xca8a04),
            term_blue: hex_to_rgb(0x2563eb),
            term_magenta: hex_to_rgb(0x9333ea),
            term_cyan: hex_to_rgb(0x0891b2),
            term_white: hex_to_rgb(0xf8fafc),
        },

        // 2. Paper White
        "paper white" | "paper" => ColorPalette {
            is_dark: false,
            bg_primary: hex_to_rgb(0xf4f4f5),
            bg_secondary: hex_to_rgb(0xffffff),
            bg_tertiary: hex_to_rgb(0xe4e4e7),
            bg_elevated: hex_to_rgb(0xd4d4d8),
            bg_hover: hex_to_rgb(0xe4e4e7),
            bg_active: hex_to_rgb(0xd4d4d8),
            accent_primary: hex_to_rgb(0x2563eb),
            accent_secondary: hex_to_rgb(0x4f46e5),
            accent_success: hex_to_rgb(0x16a34a),
            accent_warning: hex_to_rgb(0xd97706),
            accent_danger: hex_to_rgb(0xdc2626),
            text_primary: hex_to_rgb(0x18181b),
            text_secondary: hex_to_rgb(0x52525b),
            text_muted: hex_to_rgb(0xa1a1aa),
            text_on_accent: hex_to_rgb(0xffffff),
            border_primary: hex_to_rgb(0xd4d4d8),
            border_secondary: hex_to_rgb(0xe4e4e7),
            border_accent: (0x25, 0x63, 0xeb, 0x60),
            term_bg: hex_to_rgb(0xfafafa),
            term_fg: hex_to_rgb(0x18181b),
            term_cursor: hex_to_rgb(0x2563eb),
            term_selection: hex_to_rgb(0xbfdbfe),
            term_black: hex_to_rgb(0x18181b),
            term_red: hex_to_rgb(0xdc2626),
            term_green: hex_to_rgb(0x16a34a),
            term_yellow: hex_to_rgb(0xca8a04),
            term_blue: hex_to_rgb(0x2563eb),
            term_magenta: hex_to_rgb(0x9333ea),
            term_cyan: hex_to_rgb(0x0891b2),
            term_white: hex_to_rgb(0xf4f4f5),
        },

        // 3. Solarized Light
        "solarized light" | "solarized" => ColorPalette {
            is_dark: false,
            bg_primary: hex_to_rgb(0xfdf6e3),
            bg_secondary: hex_to_rgb(0xeee8d5),
            bg_tertiary: hex_to_rgb(0xe0d9c4),
            bg_elevated: hex_to_rgb(0xd3ccb8),
            bg_hover: hex_to_rgb(0xe0d9c4),
            bg_active: hex_to_rgb(0xd3ccb8),
            accent_primary: hex_to_rgb(0x2aa198),
            accent_secondary: hex_to_rgb(0x268bd2),
            accent_success: hex_to_rgb(0x859900),
            accent_warning: hex_to_rgb(0xb58900),
            accent_danger: hex_to_rgb(0xdc322f),
            text_primary: hex_to_rgb(0x073642),
            text_secondary: hex_to_rgb(0x586e75),
            text_muted: hex_to_rgb(0x93a1a1),
            text_on_accent: hex_to_rgb(0xffffff),
            border_primary: hex_to_rgb(0xd3ccb8),
            border_secondary: hex_to_rgb(0xe0d9c4),
            border_accent: (0x2a, 0xa1, 0x98, 0x60),
            term_bg: hex_to_rgb(0xfdf6e3),
            term_fg: hex_to_rgb(0x073642),
            term_cursor: hex_to_rgb(0x2aa198),
            term_selection: hex_to_rgb(0xeee8d5),
            term_black: hex_to_rgb(0x073642),
            term_red: hex_to_rgb(0xdc322f),
            term_green: hex_to_rgb(0x859900),
            term_yellow: hex_to_rgb(0xb58900),
            term_blue: hex_to_rgb(0x268bd2),
            term_magenta: hex_to_rgb(0xd33682),
            term_cyan: hex_to_rgb(0x2aa198),
            term_white: hex_to_rgb(0xfdf6e3),
        },

        // 4. Nord Light
        "nord light" | "snow storm" => ColorPalette {
            is_dark: false,
            bg_primary: hex_to_rgb(0xeceff4),
            bg_secondary: hex_to_rgb(0xe5e9f0),
            bg_tertiary: hex_to_rgb(0xd8dee9),
            bg_elevated: hex_to_rgb(0xc2c9d6),
            bg_hover: hex_to_rgb(0xd8dee9),
            bg_active: hex_to_rgb(0xc2c9d6),
            accent_primary: hex_to_rgb(0x5e81ac),
            accent_secondary: hex_to_rgb(0x81a1c1),
            accent_success: hex_to_rgb(0xa3be8c),
            accent_warning: hex_to_rgb(0xebcb8b),
            accent_danger: hex_to_rgb(0xbf616a),
            text_primary: hex_to_rgb(0x2e3440),
            text_secondary: hex_to_rgb(0x434c5e),
            text_muted: hex_to_rgb(0x7b88a1),
            text_on_accent: hex_to_rgb(0xffffff),
            border_primary: hex_to_rgb(0xc2c9d6),
            border_secondary: hex_to_rgb(0xd8dee9),
            border_accent: (0x5e, 0x81, 0xac, 0x60),
            term_bg: hex_to_rgb(0xeceff4),
            term_fg: hex_to_rgb(0x2e3440),
            term_cursor: hex_to_rgb(0x5e81ac),
            term_selection: hex_to_rgb(0xd8dee9),
            term_black: hex_to_rgb(0x2e3440),
            term_red: hex_to_rgb(0xbf616a),
            term_green: hex_to_rgb(0xa3be8c),
            term_yellow: hex_to_rgb(0xebcb8b),
            term_blue: hex_to_rgb(0x5e81ac),
            term_magenta: hex_to_rgb(0xb48ead),
            term_cyan: hex_to_rgb(0x88c0d0),
            term_white: hex_to_rgb(0xe5e9f0),
        },

        // 5. GitHub Light
        "github light" => ColorPalette {
            is_dark: false,
            bg_primary: hex_to_rgb(0xf6f8fa),
            bg_secondary: hex_to_rgb(0xffffff),
            bg_tertiary: hex_to_rgb(0xeaeef2),
            bg_elevated: hex_to_rgb(0xd0d7de),
            bg_hover: hex_to_rgb(0xeaeef2),
            bg_active: hex_to_rgb(0xafb8c1),
            accent_primary: hex_to_rgb(0x0969da),
            accent_secondary: hex_to_rgb(0x8250df),
            accent_success: hex_to_rgb(0x1a7f37),
            accent_warning: hex_to_rgb(0x9a6700),
            accent_danger: hex_to_rgb(0xcf222e),
            text_primary: hex_to_rgb(0x1f2328),
            text_secondary: hex_to_rgb(0x57606a),
            text_muted: hex_to_rgb(0x8c959f),
            text_on_accent: hex_to_rgb(0xffffff),
            border_primary: hex_to_rgb(0xd0d7de),
            border_secondary: hex_to_rgb(0xeaeef2),
            border_accent: (0x09, 0x69, 0xda, 0x60),
            term_bg: hex_to_rgb(0xffffff),
            term_fg: hex_to_rgb(0x24292f),
            term_cursor: hex_to_rgb(0x0969da),
            term_selection: hex_to_rgb(0xb6e3ff),
            term_black: hex_to_rgb(0x24292f),
            term_red: hex_to_rgb(0xcf222e),
            term_green: hex_to_rgb(0x116329),
            term_yellow: hex_to_rgb(0x4d2d00),
            term_blue: hex_to_rgb(0x0969da),
            term_magenta: hex_to_rgb(0x8250df),
            term_cyan: hex_to_rgb(0x1b7c83),
            term_white: hex_to_rgb(0x6e7781),
        },

        // 6. Catppuccin Latte
        "catppuccin latte" | "catppuccin" => ColorPalette {
            is_dark: false,
            bg_primary: hex_to_rgb(0xeff1f5),
            bg_secondary: hex_to_rgb(0xe6e9ef),
            bg_tertiary: hex_to_rgb(0xdce0e8),
            bg_elevated: hex_to_rgb(0xccd0da),
            bg_hover: hex_to_rgb(0xdce0e8),
            bg_active: hex_to_rgb(0xbcc0cc),
            accent_primary: hex_to_rgb(0x7287fd),
            accent_secondary: hex_to_rgb(0x8839ef),
            accent_success: hex_to_rgb(0x40a02b),
            accent_warning: hex_to_rgb(0xdf8e1d),
            accent_danger: hex_to_rgb(0xd20f39),
            text_primary: hex_to_rgb(0x4c4f69),
            text_secondary: hex_to_rgb(0x5c5f77),
            text_muted: hex_to_rgb(0x9ca0b0),
            text_on_accent: hex_to_rgb(0xffffff),
            border_primary: hex_to_rgb(0xccd0da),
            border_secondary: hex_to_rgb(0xdce0e8),
            border_accent: (0x72, 0x87, 0xfd, 0x60),
            term_bg: hex_to_rgb(0xeff1f5),
            term_fg: hex_to_rgb(0x4c4f69),
            term_cursor: hex_to_rgb(0xdc8a78),
            term_selection: hex_to_rgb(0xccd0da),
            term_black: hex_to_rgb(0x5c5f77),
            term_red: hex_to_rgb(0xd20f39),
            term_green: hex_to_rgb(0x40a02b),
            term_yellow: hex_to_rgb(0xdf8e1d),
            term_blue: hex_to_rgb(0x1e66f5),
            term_magenta: hex_to_rgb(0xea76cb),
            term_cyan: hex_to_rgb(0x179299),
            term_white: hex_to_rgb(0xacb0be),
        },

        // 7. Sepia Warm
        "sepia warm" | "sepia" => ColorPalette {
            is_dark: false,
            bg_primary: hex_to_rgb(0xfbf0d9),
            bg_secondary: hex_to_rgb(0xf6e6c6),
            bg_tertiary: hex_to_rgb(0xedd5ac),
            bg_elevated: hex_to_rgb(0xdfc293),
            bg_hover: hex_to_rgb(0xedd5ac),
            bg_active: hex_to_rgb(0xd2b17d),
            accent_primary: hex_to_rgb(0xb45309),
            accent_secondary: hex_to_rgb(0x7c2d12),
            accent_success: hex_to_rgb(0x15803d),
            accent_warning: hex_to_rgb(0xd97706),
            accent_danger: hex_to_rgb(0xb91c1c),
            text_primary: hex_to_rgb(0x451a03),
            text_secondary: hex_to_rgb(0x78350f),
            text_muted: hex_to_rgb(0xa16207),
            text_on_accent: hex_to_rgb(0xffffff),
            border_primary: hex_to_rgb(0xdfc293),
            border_secondary: hex_to_rgb(0xedd5ac),
            border_accent: (0xb4, 0x53, 0x09, 0x60),
            term_bg: hex_to_rgb(0xfbf0d9),
            term_fg: hex_to_rgb(0x451a03),
            term_cursor: hex_to_rgb(0xb45309),
            term_selection: hex_to_rgb(0xdfc293),
            term_black: hex_to_rgb(0x451a03),
            term_red: hex_to_rgb(0xb91c1c),
            term_green: hex_to_rgb(0x15803d),
            term_yellow: hex_to_rgb(0xb45309),
            term_blue: hex_to_rgb(0x1d4ed8),
            term_magenta: hex_to_rgb(0x7e22ce),
            term_cyan: hex_to_rgb(0x0f766e),
            term_white: hex_to_rgb(0xfbf0d9),
        },

        // 8. Rose Pine Dawn
        "rose pine dawn" | "rose pine" => ColorPalette {
            is_dark: false,
            bg_primary: hex_to_rgb(0xfaf4ed),
            bg_secondary: hex_to_rgb(0xfffaf3),
            bg_tertiary: hex_to_rgb(0xf2e9de),
            bg_elevated: hex_to_rgb(0xe4d8c8),
            bg_hover: hex_to_rgb(0xf2e9de),
            bg_active: hex_to_rgb(0xd6c5b2),
            accent_primary: hex_to_rgb(0x286983),
            accent_secondary: hex_to_rgb(0x907aa9),
            accent_success: hex_to_rgb(0x56949f),
            accent_warning: hex_to_rgb(0xea9d34),
            accent_danger: hex_to_rgb(0xb4637a),
            text_primary: hex_to_rgb(0x575279),
            text_secondary: hex_to_rgb(0x797593),
            text_muted: hex_to_rgb(0x9893a5),
            text_on_accent: hex_to_rgb(0xffffff),
            border_primary: hex_to_rgb(0xe4d8c8),
            border_secondary: hex_to_rgb(0xf2e9de),
            border_accent: (0x28, 0x69, 0x83, 0x60),
            term_bg: hex_to_rgb(0xfaf4ed),
            term_fg: hex_to_rgb(0x575279),
            term_cursor: hex_to_rgb(0x286983),
            term_selection: hex_to_rgb(0xe4d8c8),
            term_black: hex_to_rgb(0x575279),
            term_red: hex_to_rgb(0xb4637a),
            term_green: hex_to_rgb(0x56949f),
            term_yellow: hex_to_rgb(0xea9d34),
            term_blue: hex_to_rgb(0x286983),
            term_magenta: hex_to_rgb(0x907aa9),
            term_cyan: hex_to_rgb(0xd7827e),
            term_white: hex_to_rgb(0xfaf4ed),
        },

        // 9. High Contrast Light
        "high contrast light" | "high contrast" => ColorPalette {
            is_dark: false,
            bg_primary: hex_to_rgb(0xf5f5f5),
            bg_secondary: hex_to_rgb(0xffffff),
            bg_tertiary: hex_to_rgb(0xe5e5e5),
            bg_elevated: hex_to_rgb(0xcccccc),
            bg_hover: hex_to_rgb(0xe5e5e5),
            bg_active: hex_to_rgb(0xb3b3b3),
            accent_primary: hex_to_rgb(0x0055ff),
            accent_secondary: hex_to_rgb(0x7700ee),
            accent_success: hex_to_rgb(0x008800),
            accent_warning: hex_to_rgb(0xcc6600),
            accent_danger: hex_to_rgb(0xdd0000),
            text_primary: hex_to_rgb(0x000000),
            text_secondary: hex_to_rgb(0x333333),
            text_muted: hex_to_rgb(0x666666),
            text_on_accent: hex_to_rgb(0xffffff),
            border_primary: hex_to_rgb(0x999999),
            border_secondary: hex_to_rgb(0xcccccc),
            border_accent: (0x00, 0x55, 0xff, 0x80),
            term_bg: hex_to_rgb(0xffffff),
            term_fg: hex_to_rgb(0x000000),
            term_cursor: hex_to_rgb(0x0055ff),
            term_selection: hex_to_rgb(0x99ccff),
            term_black: hex_to_rgb(0x000000),
            term_red: hex_to_rgb(0xdd0000),
            term_green: hex_to_rgb(0x008800),
            term_yellow: hex_to_rgb(0xcc6600),
            term_blue: hex_to_rgb(0x0055ff),
            term_magenta: hex_to_rgb(0x7700ee),
            term_cyan: hex_to_rgb(0x0088aa),
            term_white: hex_to_rgb(0xffffff),
        },

        // Fallback based on is_dark flag
        _ => {
            if is_dark {
                get_palette_by_name("github dark", true)
            } else {
                get_palette_by_name("clean light", false)
            }
        }
    }
}

pub fn get_accent_rgb(accent: &str, is_dark: bool) -> (u8, u8, u8) {
    match accent.to_lowercase().as_str() {
        "cyan" => {
            if is_dark {
                (0x00, 0xd4, 0xff)
            } else {
                (0x02, 0x84, 0xc7)
            }
        }
        "sky" | "blue" => {
            if is_dark {
                (0x38, 0xbd, 0xf8)
            } else {
                (0x25, 0x63, 0xeb)
            }
        }
        "purple" | "violet" => {
            if is_dark {
                (0xa8, 0x55, 0xf7)
            } else {
                (0x7c, 0x3a, 0xed)
            }
        }
        "emerald" | "green" => {
            if is_dark {
                (0x10, 0xb9, 0x81)
            } else {
                (0x16, 0xa3, 0x4a)
            }
        }
        "orange" => {
            if is_dark {
                (0xf9, 0x73, 0x16)
            } else {
                (0xea, 0x58, 0x0c)
            }
        }
        "rose" => {
            if is_dark {
                (0xf4, 0x3f, 0x5e)
            } else {
                (0xe1, 0x1d, 0x48)
            }
        }
        "red" | "bright red" | "crimson" => {
            if is_dark {
                (0xff, 0x00, 0x00) // Pure vivid RGB (255, 0, 0)
            } else {
                (0xef, 0x00, 0x00)
            }
        }
        "amber" | "yellow" => {
            if is_dark {
                (0xf5, 0x9e, 0x0b)
            } else {
                (0xd9, 0x77, 0x06)
            }
        }
        "slate" | "gray" => {
            if is_dark {
                (0x94, 0xa3, 0xb8)
            } else {
                (0x47, 0x55, 0x69)
            }
        }
        "pink" | "magenta" => {
            if is_dark {
                (0xec, 0x48, 0x99)
            } else {
                (0xdb, 0x27, 0x77)
            }
        }
        "lime" | "mint" => {
            if is_dark {
                (0x84, 0xcc, 0x16)
            } else {
                (0x65, 0xa3, 0x0d)
            }
        }
        "indigo" | "royal" => {
            if is_dark {
                (0x63, 0x66, 0xf1)
            } else {
                (0x4f, 0x46, 0xe5)
            }
        }
        "teal" | "aqua" => {
            if is_dark {
                (0x14, 0xb8, 0xa6)
            } else {
                (0x0d, 0x94, 0x88)
            }
        }
        _ => {
            if is_dark {
                (0x00, 0xd4, 0xff)
            } else {
                (0x02, 0x84, 0xc7)
            }
        }
    }
}

fn to_slint_color(rgb: (u8, u8, u8)) -> Color {
    Color::from_argb_u8(255, rgb.0, rgb.1, rgb.2)
}

fn to_slint_color_alpha(rgba: (u8, u8, u8, u8)) -> Color {
    Color::from_argb_u8(rgba.3, rgba.0, rgba.1, rgba.2)
}

/// Applies the theme configuration to the Slint Theme global instance.
/// Returns the effective color scheme name and whether dark mode is active.
pub fn apply_theme(
    theme_global: &crate::Theme,
    mode_str: &str,
    scheme_str: &str,
    accent_str: &str,
) -> (String, bool) {
    let mode = ThemeMode::from_str(mode_str);
    let is_dark = match mode {
        ThemeMode::Dark => true,
        ThemeMode::Light => false,
        ThemeMode::System => detect_system_is_dark(),
    };

    let effective_scheme = get_effective_scheme(scheme_str, is_dark);
    let mut palette = get_palette_by_name(&effective_scheme, is_dark);

    // Apply accent override if specified
    if !accent_str.trim().is_empty() && accent_str.to_lowercase() != "default" {
        palette.accent_primary = get_accent_rgb(accent_str, is_dark);
        palette.border_accent = (
            palette.accent_primary.0,
            palette.accent_primary.1,
            palette.accent_primary.2,
            if is_dark { 0x4d } else { 0x60 },
        );
    }

    // Set Theme global properties in Slint
    theme_global.set_is_dark(is_dark);
    theme_global.set_bg_primary(to_slint_color(palette.bg_primary));
    theme_global.set_bg_secondary(to_slint_color(palette.bg_secondary));
    theme_global.set_bg_tertiary(to_slint_color(palette.bg_tertiary));
    theme_global.set_bg_elevated(to_slint_color(palette.bg_elevated));
    theme_global.set_bg_hover(to_slint_color(palette.bg_hover));
    theme_global.set_bg_active(to_slint_color(palette.bg_active));

    theme_global.set_accent_primary(to_slint_color(palette.accent_primary));
    theme_global.set_accent_secondary(to_slint_color(palette.accent_secondary));
    theme_global.set_accent_success(to_slint_color(palette.accent_success));
    theme_global.set_accent_warning(to_slint_color(palette.accent_warning));
    theme_global.set_accent_danger(to_slint_color(palette.accent_danger));

    theme_global.set_text_primary(to_slint_color(palette.text_primary));
    theme_global.set_text_secondary(to_slint_color(palette.text_secondary));
    theme_global.set_text_muted(to_slint_color(palette.text_muted));
    theme_global.set_text_accent(to_slint_color(palette.accent_primary));
    theme_global.set_text_on_accent(to_slint_color(palette.text_on_accent));

    theme_global.set_border_primary(to_slint_color(palette.border_primary));
    theme_global.set_border_secondary(to_slint_color(palette.border_secondary));
    theme_global.set_border_accent(to_slint_color_alpha(palette.border_accent));

    theme_global.set_term_bg(to_slint_color(palette.term_bg));
    theme_global.set_term_fg(to_slint_color(palette.term_fg));
    theme_global.set_term_cursor(to_slint_color(palette.term_cursor));
    theme_global.set_term_selection(to_slint_color(palette.term_selection));
    theme_global.set_term_black(to_slint_color(palette.term_black));
    theme_global.set_term_red(to_slint_color(palette.term_red));
    theme_global.set_term_green(to_slint_color(palette.term_green));
    theme_global.set_term_yellow(to_slint_color(palette.term_yellow));
    theme_global.set_term_blue(to_slint_color(palette.term_blue));
    theme_global.set_term_magenta(to_slint_color(palette.term_magenta));
    theme_global.set_term_cyan(to_slint_color(palette.term_cyan));
    theme_global.set_term_white(to_slint_color(palette.term_white));

    (effective_scheme, is_dark)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_mode_parsing() {
        assert_eq!(ThemeMode::from_str("dark"), ThemeMode::Dark);
        assert_eq!(ThemeMode::from_str("Dark"), ThemeMode::Dark);
        assert_eq!(ThemeMode::from_str("light"), ThemeMode::Light);
        assert_eq!(ThemeMode::from_str("Light"), ThemeMode::Light);
        assert_eq!(ThemeMode::from_str("system"), ThemeMode::System);
        assert_eq!(ThemeMode::from_str("System"), ThemeMode::System);
        assert_eq!(ThemeMode::from_str("unknown"), ThemeMode::Dark);
    }

    #[test]
    fn test_all_9_dark_and_9_light_palettes() {
        let dark_themes = [
            "GitHub Dark",
            "Midnight Navy",
            "Cyberpunk",
            "Emerald Matrix",
            "Tokyo Night",
            "Nord Slate",
            "Dracula",
            "Monokai Pro",
            "Solarized Dark",
        ];

        for name in &dark_themes {
            let p = get_palette_by_name(name, true);
            assert!(p.is_dark, "Theme {} should be dark", name);
            assert!(is_preset_dark(name));
        }

        let light_themes = [
            "Clean Light",
            "Paper White",
            "Solarized Light",
            "Nord Light",
            "GitHub Light",
            "Catppuccin Latte",
            "Sepia Warm",
            "Rose Pine Dawn",
            "High Contrast Light",
        ];

        for name in &light_themes {
            let p = get_palette_by_name(name, false);
            assert!(!p.is_dark, "Theme {} should be light", name);
            assert!(!is_preset_dark(name));
        }
    }

    #[test]
    fn test_effective_scheme_switch() {
        assert_eq!(get_effective_scheme("GitHub Dark", false), "Clean Light");
        assert_eq!(get_effective_scheme("Clean Light", true), "GitHub Dark");
        assert_eq!(get_effective_scheme("Midnight Navy", true), "Midnight Navy");
        assert_eq!(get_effective_scheme("Paper White", false), "Paper White");
    }

    #[test]
    fn test_13_accent_colors() {
        let accents = [
            "cyan", "sky", "purple", "emerald", "orange", "rose", "red", "amber", "slate", "pink", "lime",
            "indigo", "teal",
        ];
        for acc in &accents {
            let dark_rgb = get_accent_rgb(acc, true);
            let light_rgb = get_accent_rgb(acc, false);
            assert_ne!(dark_rgb, (0, 0, 0));
            assert_ne!(light_rgb, (0, 0, 0));
        }
        // Verify red is pure bright red
        assert_eq!(get_accent_rgb("red", true), (255, 0, 0));
    }
}
