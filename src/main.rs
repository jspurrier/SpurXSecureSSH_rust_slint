#![windows_subsystem = "windows"]

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

use slint::{ComponentHandle, Model, ModelRc, VecModel};
use spurx_secure_ssh::terminal::TerminalBuffer;
use spurx_secure_ssh::*;

#[derive(Default, Debug)]
struct FolderExpansionState {
    expanded: HashSet<String>,
    search_collapsed: HashSet<String>,
}

#[derive(Default, Debug)]
struct SessionFolderNode {
    full_path: String,
    subfolders: BTreeMap<String, SessionFolderNode>,
    sessions: Vec<SavedSession>,
}

impl SessionFolderNode {
    fn insert_session(&mut self, folder_parts: &[&str], full_path: &str, session: SavedSession) {
        if folder_parts.is_empty() {
            self.sessions.push(session);
        } else {
            let part = folder_parts[0];
            let next_full_path = if full_path.is_empty() {
                part.to_string()
            } else {
                format!("{}/{}", full_path, part)
            };
            let child =
                self.subfolders
                    .entry(part.to_string())
                    .or_insert_with(|| SessionFolderNode {
                        full_path: next_full_path.clone(),
                        subfolders: BTreeMap::new(),
                        sessions: Vec::new(),
                    });
            child.insert_session(&folder_parts[1..], &next_full_path, session);
        }
    }

    fn total_items(&self) -> usize {
        let sub_items: usize = self.subfolders.values().map(|c| c.total_items()).sum();
        self.sessions.len() + sub_items
    }

    fn flatten_to_models(
        &self,
        expanded_set: &HashSet<String>,
        search_collapsed: Option<&HashSet<String>>,
        depth: usize,
        out: &mut Vec<SessionItemModel>,
    ) {
        for (name, folder_node) in &self.subfolders {
            let is_expanded = match search_collapsed {
                Some(collapsed) => !collapsed.contains(&folder_node.full_path),
                None => expanded_set.contains(&folder_node.full_path),
            };
            let count = folder_node.total_items();

            out.push(SessionItemModel {
                id: format!("folder:{}", folder_node.full_path).into(),
                name: name.clone().into(),
                host: "".into(),
                port: 0,
                folder: folder_node.full_path.clone().into(),
                username: "".into(),
                is_folder: true,
                expanded: is_expanded,
                count: count as i32,
                depth: depth as i32,
            });

            if is_expanded {
                folder_node.flatten_to_models(expanded_set, search_collapsed, depth + 1, out);
            }
        }

        for s in &self.sessions {
            out.push(SessionItemModel {
                id: s.id.clone().into(),
                name: s.name.clone().into(),
                host: s.host.clone().into(),
                port: s.port as i32,
                folder: self.full_path.clone().into(),
                username: s.username.clone().unwrap_or_default().into(),
                is_folder: false,
                expanded: false,
                count: 0,
                depth: depth as i32,
            });
        }
    }
}

#[derive(Default, Debug)]
struct CommandFolderNode {
    full_path: String,
    subfolders: BTreeMap<String, CommandFolderNode>,
    commands: Vec<QuickCommand>,
}

impl CommandFolderNode {
    fn insert_command(&mut self, folder_parts: &[&str], full_path: &str, command: QuickCommand) {
        if folder_parts.is_empty() {
            self.commands.push(command);
        } else {
            let part = folder_parts[0];
            let next_full_path = if full_path.is_empty() {
                part.to_string()
            } else {
                format!("{}/{}", full_path, part)
            };
            let child =
                self.subfolders
                    .entry(part.to_string())
                    .or_insert_with(|| CommandFolderNode {
                        full_path: next_full_path.clone(),
                        subfolders: BTreeMap::new(),
                        commands: Vec::new(),
                    });
            child.insert_command(&folder_parts[1..], &next_full_path, command);
        }
    }

    fn total_items(&self) -> usize {
        let sub_items: usize = self.subfolders.values().map(|c| c.total_items()).sum();
        self.commands.len() + sub_items
    }

    fn flatten_to_models(
        &self,
        expanded_set: &HashSet<String>,
        search_collapsed: Option<&HashSet<String>>,
        depth: usize,
        out: &mut Vec<CommandItemModel>,
    ) {
        for (name, folder_node) in &self.subfolders {
            let is_expanded = match search_collapsed {
                Some(collapsed) => !collapsed.contains(&folder_node.full_path),
                None => expanded_set.contains(&folder_node.full_path),
            };
            let count = folder_node.total_items();

            out.push(CommandItemModel {
                id: format!("cmd_folder:{}", folder_node.full_path).into(),
                name: name.clone().into(),
                command: "".into(),
                folder: folder_node.full_path.clone().into(),
                color: "cyan".into(),
                description: "".into(),
                is_folder: true,
                expanded: is_expanded,
                count: count as i32,
                depth: depth as i32,
            });

            if is_expanded {
                folder_node.flatten_to_models(expanded_set, search_collapsed, depth + 1, out);
            }
        }

        for c in &self.commands {
            out.push(CommandItemModel {
                id: c.id.clone().into(),
                name: c.name.clone().into(),
                command: c.command.clone().into(),
                folder: self.full_path.clone().into(),
                color: c.icon.clone().unwrap_or_else(|| "cyan".to_string()).into(),
                description: c.description.clone().unwrap_or_default().into(),
                is_folder: false,
                expanded: false,
                count: 0,
                depth: depth as i32,
            });
        }
    }
}

const TAB_GROUP_COLORS: [&str; 6] = [
    "#00d4ff", // Cyan
    "#10b981", // Emerald
    "#f59e0b", // Amber
    "#f97316", // Orange
    "#a855f7", // Violet
    "#ec4899", // Pink
];

fn parse_hex_color(hex: &str) -> slint::Color {
    let hex = hex.trim_start_matches('#');
    if hex.len() == 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
        slint::Color::from_rgb_u8(r, g, b)
    } else {
        slint::Color::from_rgb_u8(0, 212, 255)
    }
}

fn sync_tab_groups_ui(app: &AppWindow, state: &Arc<AppState>) {
    let groups_map = state.tab_groups.read();
    let mut ui_groups: Vec<TabGroupModel> = Vec::new();

    for g in groups_map.values() {
        let color_hex = TAB_GROUP_COLORS[g.color_index % TAB_GROUP_COLORS.len()];
        let col = parse_hex_color(color_hex);
        ui_groups.push(TabGroupModel {
            id: g.id.clone().into(),
            name: g.name.clone().into(),
            color: col,
            color_index: g.color_index as i32,
            collapsed: g.collapsed,
            tab_count: g.tab_ids.len() as i32,
        });
    }
    ui_groups.sort_by(|a, b| a.name.to_string().cmp(&b.name.to_string()));
    app.set_tab_groups(ModelRc::new(VecModel::from(ui_groups)));

    // Update existing tabs with their group metadata
    let tabs = app.get_tabs();
    let mut updated_tabs: Vec<TabModel> = Vec::new();
    for i in 0..tabs.row_count() {
        if let Some(mut t) = tabs.row_data(i) {
            let tab_id = t.id.to_string();
            let mut found_group = None;
            for g in groups_map.values() {
                if g.tab_ids.contains(&tab_id) {
                    found_group = Some(g.clone());
                    break;
                }
            }
            if let Some(g) = found_group {
                let color_hex = TAB_GROUP_COLORS[g.color_index % TAB_GROUP_COLORS.len()];
                t.group_id = g.id.into();
                t.group_name = g.name.into();
                t.group_color = parse_hex_color(color_hex);
                t.is_grouped = true;
            } else {
                t.group_id = "".into();
                t.group_name = "".into();
                t.group_color = slint::Color::from_rgb_u8(0, 0, 0);
                t.is_grouped = false;
            }
            updated_tabs.push(t);
        }
    }
    app.set_tabs(ModelRc::new(VecModel::from(updated_tabs)));
}

fn update_terminal_geometry(
    app: &AppWindow,
    app_state: &Arc<AppState>,
    term_h_px: f32,
    term_w_px: f32,
) {
    let font_sz = app_state
        .current_terminal_font_size
        .load(std::sync::atomic::Ordering::Relaxed)
        .max(8) as f32;
    // Font-aware monospace line height in Slint TextInput.
    // Measured font metrics (UPEM, ascent, descent, win metrics):
    // JetBrains Mono: ~1.34-1.35x
    // Cascadia Code / Fira Code: ~1.34-1.36x
    // Source Code Pro / Ubuntu Mono: ~1.40-1.42x
    let font_fam_lower = app_state.current_terminal_font_family.read().to_lowercase();
    let line_height_factor = if font_fam_lower.contains("source code") || font_fam_lower.contains("ubuntu") {
        1.42
    } else if font_fam_lower.contains("fira") || font_fam_lower.contains("cascadia") || font_fam_lower.contains("inconsolata") {
        1.36
    } else {
        1.34
    };
    let row_height_px = (font_sz * line_height_factor + 0.5).max(11.0);
    let char_width_px = (font_sz * 0.60).max(5.0);

    let calculated_cols = if term_w_px > 40.0 {
        ((term_w_px - 24.0) / char_width_px).floor().max(20.0) as usize
    } else {
        80
    };
    let calculated_rows = if term_h_px > 30.0 {
        // Reserve 14px padding clearance and subtract 1 row safety buffer so prompt line is always fully visible
        let usable_h = (term_h_px - 14.0).max(row_height_px);
        ((usable_h / row_height_px).floor().max(4.0) as usize).saturating_sub(1).max(4)
    } else {
        24
    };

    app_state
        .current_terminal_cols
        .store(calculated_cols, std::sync::atomic::Ordering::Relaxed);
    app_state
        .current_terminal_rows
        .store(calculated_rows, std::sync::atomic::Ordering::Relaxed);

    app.set_status_dimensions(format!("{}x{}", calculated_cols, calculated_rows).into());

    // Update ALL buffers so switching tabs post-resize is correct
    {
        let buffers = app_state.terminal_buffers.read();
        for buf in buffers.values() {
            buf.lock().set_size(calculated_cols, calculated_rows);
        }
    }

    // Send dynamic SSH NAWS window resize to all active sessions
    {
        let senders = app_state.input_senders.read();
        for sender in senders.values() {
            let _ = sender.send(ssh::SshInput::Resize {
                rows: calculated_rows as u16,
                cols: calculated_cols as u16,
            });
        }
    }

    // Re-render the active tab immediately
    let active_idx = app.get_active_tab_index() as usize;
    let tabs = app.get_tabs();
    if active_idx < tabs.row_count() {
        if let Some(tab) = tabs.row_data(active_idx) {
            let sid = tab.id.to_string();
            let buffers = app_state.terminal_buffers.read();
            if let Some(buf) = buffers.get(&sid) {
                let (text, offset, total, scroll_off, vis_rows) =
                    buf.lock().get_visible_text();
                app.set_terminal_scroll_total(total as i32);
                app.set_terminal_scroll_offset(scroll_off as i32);
                app.set_terminal_scroll_visible(vis_rows as i32);
                app.set_terminal_full_text(text.into());
                app.invoke_set_terminal_cursor_pos(offset as i32);
            }
        }
    }
}

fn make_tab_model(
    id: &str,
    title: &str,
    connected: bool,
    active: bool,
    host: &str,
    state: Option<&Arc<AppState>>,
) -> TabModel {
    let mut group_id = String::new();
    let mut group_name = String::new();
    let mut group_color = slint::Color::from_rgb_u8(0, 0, 0);
    let mut is_grouped = false;

    if let Some(state) = state {
        let groups = state.tab_groups.read();
        for g in groups.values() {
            if g.tab_ids.iter().any(|tid| tid == id) {
                group_id = g.id.clone();
                group_name = g.name.clone();
                let color_hex = TAB_GROUP_COLORS[g.color_index % TAB_GROUP_COLORS.len()];
                group_color = parse_hex_color(color_hex);
                is_grouped = true;
                break;
            }
        }
    }

    TabModel {
        id: id.into(),
        title: title.into(),
        connected,
        active,
        host: host.into(),
        group_id: group_id.into(),
        group_name: group_name.into(),
        group_color,
        is_grouped,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Prioritize Skia GPU acceleration (Metal on macOS, D3D12 on Windows, Vulkan/OpenGL on Linux)
    if std::env::var("SLINT_BACKEND").is_err() {
        std::env::set_var("SLINT_BACKEND", "winit-skia");
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let app = AppWindow::new()?;
    app.set_about_app_version(format!("Version {} (Slint Native Edition)", env!("CARGO_PKG_VERSION")).into());
    let app_state = Arc::new(AppState::new());

    let app_cfg = settings::load_settings();

    let expanded_session_folders = Arc::new(parking_lot::Mutex::new(FolderExpansionState::default()));
    let expanded_command_folders = Arc::new(parking_lot::Mutex::new(FolderExpansionState::default()));
    let session_cache = Arc::new(parking_lot::RwLock::new(
        session::load_sessions().unwrap_or_default(),
    ));
    let command_cache = Arc::new(parking_lot::RwLock::new(
        command_storage::load_commands().unwrap_or_default(),
    ));

    // If remember_expanded_folders is enabled, restore the saved expanded folder paths
    if app_cfg.remember_expanded_folders {
        let mut exp = expanded_session_folders.lock();
        for f in &app_cfg.expanded_folders {
            exp.expanded.insert(f.clone());
        }
    }

    let sidebar_filter = Arc::new(parking_lot::Mutex::new(String::new()));

    // Populate initial settings in UI
    if let Some(ref gu) = app_cfg.global_username {
        app.set_settings_global_username(gu.clone().into());
    }
    if let Ok(Some(gp)) = session::get_password("__global__") {
        app.set_settings_global_password(gp.into());
    }
    app.set_settings_default_port(app_cfg.default_port.to_string().into());
    app.set_settings_timeout(app_cfg.connection_timeout.to_string().into());
    app.set_settings_keepalive(app_cfg.keepalive_interval.to_string().into());
    let cur_config_dir = spurx_secure_ssh::get_app_config_dir()
        .to_string_lossy()
        .to_string();
    app.set_settings_config_dir(cur_config_dir.into());
    app.set_settings_logging_enabled(app_cfg.logging_enabled);
    let log_d = app_cfg.log_directory.clone().unwrap_or_else(|| {
        settings::get_default_log_dir()
            .to_string_lossy()
            .to_string()
    });
    app.set_settings_log_dir(log_d.into());
    app.set_settings_log_format(app_cfg.log_format.clone().into());
    app.set_settings_font_family(app_cfg.font_family.clone().into());
    app.set_settings_font_size(app_cfg.font_size.to_string().into());
    app_state
        .current_terminal_font_size
        .store(app_cfg.font_size, std::sync::atomic::Ordering::Relaxed);
    *app_state.current_terminal_font_family.write() = app_cfg.font_family.clone();

    // Populate available system fonts for font picker
    let system_fonts = settings::get_available_system_fonts();
    let font_models: Vec<slint::SharedString> =
        system_fonts.iter().map(|s| s.clone().into()).collect();
    app.set_available_system_fonts(slint::ModelRc::new(slint::VecModel::from(font_models)));

    app.set_settings_scrollback(app_cfg.scrollback_lines.to_string().into());
    app.set_settings_cursor_blink(app_cfg.cursor_blink);
    app.set_settings_theme_mode(app_cfg.theme.clone().into());
    app.set_settings_color_scheme(app_cfg.color_scheme.clone().into());
    app.set_settings_accent_color(app_cfg.accent_color.clone().into());
    app.set_settings_confirm_close(app_cfg.confirm_on_close);
    app.set_settings_restore_sessions(app_cfg.restore_sessions);
    app.set_settings_remember_expanded_folders(app_cfg.remember_expanded_folders);
    app.set_settings_single_click_launch(app_cfg.single_click_launch);

    // Apply initial theme
    let theme_global = app.global::<Theme>();
    theme_global.set_is_macos(cfg!(target_os = "macos"));
    let (effective_scheme, _) = theme::apply_theme(
        &theme_global,
        &app_cfg.theme,
        &app_cfg.color_scheme,
        &app_cfg.accent_color,
    );
    app.set_settings_color_scheme(effective_scheme.into());

    // Populate initial AI settings
    let ai_cfg = ai::load_config();
    app.set_ai_settings_ollama_host(ai_cfg.ollama_host.clone().into());
    app.set_ai_settings_ollama_model(ai_cfg.ollama_model.clone().into());
    app.set_ai_settings_lmstudio_host(ai_cfg.lmstudio_host.clone().into());
    app.set_ai_settings_lmstudio_model(ai_cfg.lmstudio_model.clone().into());
    app.set_ai_settings_lmstudio_key(ai_cfg.lmstudio_api_key.clone().unwrap_or_default().into());
    app.set_ai_settings_gemini_key(ai_cfg.gemini_api_key.clone().unwrap_or_default().into());
    app.set_ai_settings_gemini_model(
        ai_cfg
            .gemini_model
            .clone()
            .unwrap_or_else(|| "gemini-3.6-flash".to_string())
            .into(),
    );
    app.set_ai_settings_grok_key(ai_cfg.grok_api_key.clone().unwrap_or_default().into());
    app.set_ai_settings_grok_model(
        ai_cfg
            .grok_model
            .clone()
            .unwrap_or_else(|| "grok-4.5".to_string())
            .into(),
    );

    let prov = ai_cfg
        .provider
        .clone()
        .unwrap_or_else(|| "ollama".to_string());
    let init_mod = match prov.as_str() {
        "gemini" => ai_cfg
            .gemini_model
            .clone()
            .unwrap_or_else(|| "gemini-3.6-flash".to_string()),
        "grok" => ai_cfg
            .grok_model
            .clone()
            .unwrap_or_else(|| "grok-4.5".to_string()),
        "lmstudio" => {
            if ai_cfg.lmstudio_model.is_empty() {
                "local-model".to_string()
            } else {
                ai_cfg.lmstudio_model.clone()
            }
        }
        _ => {
            if ai_cfg.ollama_model.is_empty() {
                "llama3.2".to_string()
            } else {
                ai_cfg.ollama_model.clone()
            }
        }
    };
    app.set_ai_selected_provider(prov.into());
    app.set_ai_selected_model(init_mod.into());

    // Load initial data
    refresh_sessions_ui(&app, &session_cache, &expanded_session_folders, "");
    refresh_commands_ui(&app, &command_cache, &expanded_command_folders, "");
    refresh_keys_ui(&app);
    refresh_known_hosts_ui(&app);
    refresh_tunnels_ui(&app, &app_state);
    refresh_local_sftp_ui(&app, "/home");

    // Startup Auto-Update Check
    let app_weak_update = app.as_weak();
    let rt_handle_update = rt.handle().clone();
    rt_handle_update.spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
        let client = reqwest::Client::builder()
            .user_agent("SpurX-Secure-SSH-UpdateChecker")
            .timeout(tokio::time::Duration::from_secs(10))
            .build();
        if let Ok(client) = client {
            let url = "https://api.github.com/repos/jspurrier/SpurXSecureSSH_rust_slint/releases/latest";
            if let Ok(resp) = client.get(url).send().await {
                if let Ok(val) = resp.json::<serde_json::Value>().await {
                    if let Some(tag) = val.get("tag_name").and_then(|t| t.as_str()) {
                        let parse_parts = |s: &str| -> Vec<u64> {
                            s.trim_start_matches('v')
                                .split('.')
                                .map(|p| {
                                    p.chars()
                                        .take_while(|c| c.is_ascii_digit())
                                        .collect::<String>()
                                        .parse::<u64>()
                                        .unwrap_or(0)
                                })
                                .collect()
                        };
                        let curr_parts = parse_parts(env!("CARGO_PKG_VERSION"));
                        let latest_parts = parse_parts(tag);
                        if latest_parts > curr_parts {
                            let html_url = val
                                .get("html_url")
                                .and_then(|u| u.as_str())
                                .unwrap_or("https://github.com/jspurrier/SpurXSecureSSH_rust_slint/releases/latest")
                                .to_string();
                            let tag_str = tag.to_string();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(app) = app_weak_update.upgrade() {
                                    app.set_update_toast_version(tag_str.into());
                                    app.set_update_toast_url(html_url.into());
                                    app.set_update_toast_visible(true);
                                }
                            });
                        }
                    }
                }
            }
        }
    });

    // Setup initial default tab
    setup_initial_tab(&app);

    // Setup Sidebar Filter (Instant Real-Time Asynchronous Search)
    let (filter_tx, mut filter_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let filter_clone = sidebar_filter.clone();
    app.on_request_sidebar_filter(move |query| {
        let q_str = query.to_string();
        *filter_clone.lock() = q_str.clone();
        let _ = filter_tx.send(q_str);
    });

    let app_weak_filter = app.as_weak();
    let sess_cache_filt = session_cache.clone();
    let exp_sess_filt = expanded_session_folders.clone();
    let cmd_cache_filt = command_cache.clone();
    let exp_cmd_filt = expanded_command_folders.clone();
    rt.spawn(async move {
        let mut pending_query: Option<String> = None;
        loop {
            tokio::select! {
                recv_res = filter_rx.recv() => {
                    match recv_res {
                        Some(q) => {
                            if q.is_empty() {
                                // Instant reset when clearing filter
                                pending_query = None;
                                exp_sess_filt.lock().search_collapsed.clear();
                                exp_cmd_filt.lock().search_collapsed.clear();
                                let app_handle = app_weak_filter.clone();
                                let sess_cache = sess_cache_filt.clone();
                                let exp_sess = exp_sess_filt.clone();
                                let cmd_cache = cmd_cache_filt.clone();
                                let exp_cmd = exp_cmd_filt.clone();
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(app) = app_handle.upgrade() {
                                        refresh_sessions_ui(&app, &sess_cache, &exp_sess, "");
                                        refresh_commands_ui(&app, &cmd_cache, &exp_cmd, "");
                                    }
                                });
                            } else {
                                // Reset the debounce timer on every new keystroke
                                pending_query = Some(q);
                            }
                        }
                        None => break,
                    }
                }
                _ = tokio::time::sleep(std::time::Duration::from_millis(280)), if pending_query.is_some() => {
                    if let Some(q) = pending_query.take() {
                        exp_sess_filt.lock().search_collapsed.clear();
                        exp_cmd_filt.lock().search_collapsed.clear();
                        let app_handle = app_weak_filter.clone();
                        let sess_cache = sess_cache_filt.clone();
                        let exp_sess = exp_sess_filt.clone();
                        let cmd_cache = cmd_cache_filt.clone();
                        let exp_cmd = exp_cmd_filt.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = app_handle.upgrade() {
                                refresh_sessions_ui(&app, &sess_cache, &exp_sess, &q);
                                refresh_commands_ui(&app, &cmd_cache, &exp_cmd, &q);
                            }
                        });
                    }
                }
            }
        }
    });

    // Setup Toggle Session Folder callback (Instant in-memory toggle, async background persistence)
    let app_weak = app.as_weak();
    let exp_sess = expanded_session_folders.clone();
    let filter_for_sess = sidebar_filter.clone();
    let sess_cache_toggle = session_cache.clone();
    app.on_request_toggle_session_folder(move |folder| {
        let f_str = folder.to_string();
        let is_searching = !filter_for_sess.lock().trim().is_empty();
        let mut exp_vec = Vec::new();
        {
            let mut exp = exp_sess.lock();
            if is_searching {
                if exp.search_collapsed.contains(&f_str) {
                    exp.search_collapsed.remove(&f_str);
                } else {
                    exp.search_collapsed.insert(f_str);
                }
            } else {
                if exp.expanded.contains(&f_str) {
                    exp.expanded.remove(&f_str);
                } else {
                    exp.expanded.insert(f_str);
                }
                exp_vec = exp.expanded.iter().cloned().collect();
            }
        }
        if !is_searching {
            std::thread::spawn(move || {
                let mut app_cfg = settings::load_settings();
                if app_cfg.remember_expanded_folders {
                    app_cfg.expanded_folders = exp_vec;
                    let _ = settings::save_settings(&app_cfg);
                }
            });
        }
        if let Some(app) = app_weak.upgrade() {
            refresh_sessions_ui(&app, &sess_cache_toggle, &exp_sess, &filter_for_sess.lock());
        }
    });

    // Setup Toggle Command Folder callback (Instant in-memory toggle)
    let app_weak = app.as_weak();
    let exp_cmd = expanded_command_folders.clone();
    let filter_for_cmd = sidebar_filter.clone();
    let cmd_cache_toggle = command_cache.clone();
    app.on_request_toggle_command_folder(move |folder| {
        let f_str = folder.to_string();
        let is_searching = !filter_for_cmd.lock().trim().is_empty();
        {
            let mut exp = exp_cmd.lock();
            if is_searching {
                if exp.search_collapsed.contains(&f_str) {
                    exp.search_collapsed.remove(&f_str);
                } else {
                    exp.search_collapsed.insert(f_str);
                }
            } else {
                if exp.expanded.contains(&f_str) {
                    exp.expanded.remove(&f_str);
                } else {
                    exp.expanded.insert(f_str);
                }
            }
        }
        if let Some(app) = app_weak.upgrade() {
            refresh_commands_ui(&app, &cmd_cache_toggle, &exp_cmd, &filter_for_cmd.lock());
        }
    });

    // Setup Quick Connect callback
    let app_weak = app.as_weak();
    let state_clone = app_state.clone();
    let rt_handle = rt.handle().clone();
    app.on_request_quick_connect(move |host, port, user, pwd, auth, save| {
        let app_weak = app_weak.clone();
        let state = state_clone.clone();
        let host_str = host.to_string();
        let port_u16 = port.parse::<u16>().unwrap_or(22);
        let user_str = if user.is_empty() {
            settings::load_settings()
                .global_username
                .unwrap_or_else(|| "root".to_string())
        } else {
            user.to_string()
        };
        let pwd_opt = if pwd.is_empty() {
            session::get_password("__global__").ok().flatten()
        } else {
            Some(pwd.to_string())
        };
        let auth_str = auth.to_string();

        if save {
            let mut new_sess = SavedSession::new(
                host_str.clone(),
                host_str.clone(),
                port_u16,
                Some("Default".to_string()),
                Some(user_str.clone()),
                None,
            );
            new_sess.auth_method = auth_str.clone();
            let sess_id = new_sess.id.clone();
            let _ = session::save_session(new_sess);
            if let Some(ref p) = pwd_opt {
                let _ = session::save_password(&sess_id, p);
            }
        }

        rt_handle.spawn(async move {
            connect_ssh_session(
                app_weak, state, host_str, port_u16, user_str, pwd_opt, auth_str, None, None, None,
            )
            .await;
        });
    });

    // Setup Connect Address Bar callback
    let app_weak = app.as_weak();
    let state_clone = app_state.clone();
    let rt_handle = rt.handle().clone();
    app.on_request_connect_address(move |addr| {
        let app_weak = app_weak.clone();
        let state = state_clone.clone();
        let addr_str = addr.to_string();
        let parts: Vec<&str> = addr_str.split(':').collect();
        let host = parts[0].to_string();
        let port = if parts.len() > 1 {
            parts[1].parse::<u16>().unwrap_or(22)
        } else {
            22
        };
        let app_cfg = settings::load_settings();
        let user = app_cfg
            .global_username
            .unwrap_or_else(|| "root".to_string());
        let pwd = session::get_password("__global__").ok().flatten();

        rt_handle.spawn(async move {
            connect_ssh_session(
                app_weak,
                state,
                host,
                port,
                user,
                pwd,
                "password".to_string(),
                None,
                None,
                None,
            )
            .await;
        });
    });

    // Setup Connect Session from Sidebar (1-Click Connect)
    let app_weak = app.as_weak();
    let state_clone = app_state.clone();
    let rt_handle = rt.handle().clone();
    app.on_request_connect_session(move |sess| {
        let app_weak = app_weak.clone();
        let state = state_clone.clone();

        let host = sess.host.to_string();
        let port = if sess.port > 0 { sess.port as u16 } else { 22 };
        let sess_id = sess.id.to_string();
        let sess_user = sess.username.to_string();

        rt_handle.spawn(async move {
            let app_cfg = settings::load_settings();
            let mut final_user = if !sess_user.is_empty() {
                sess_user
            } else {
                app_cfg
                    .global_username
                    .clone()
                    .unwrap_or_else(|| "root".to_string())
            };
            let mut final_pwd = session::get_password(&sess_id).ok().flatten();
            let mut auth_method = "password".to_string();
            let mut priv_key = None;
            let mut sess_log_dir = None;
            let mut tunnels_to_start = Vec::new();

            // Enrich from saved sessions if available
            if let Ok(sessions) = session::load_sessions() {
                if let Some(s) = sessions
                    .into_iter()
                    .find(|item| item.id == sess_id || item.host == host)
                {
                    if s.use_global_credentials {
                        final_user = app_cfg
                            .global_username
                            .clone()
                            .filter(|u| !u.trim().is_empty())
                            .unwrap_or_else(|| "root".to_string());
                        final_pwd = session::get_password("__global__").ok().flatten();
                        auth_method = "password".to_string();
                    } else {
                        if let Some(ref u) = s.username {
                            if !u.trim().is_empty() {
                                final_user = u.clone();
                            }
                        }
                        if final_pwd.is_none() {
                            final_pwd = session::get_password(&s.id).ok().flatten();
                        }
                        auth_method = s.auth_method;
                        priv_key = s.private_key_name;
                    }
                    sess_log_dir = s.log_directory;

                    if s.auto_start_tunnels {
                        tunnels_to_start = s.tunnels;
                    } else {
                        tunnels_to_start = s.tunnels.into_iter().filter(|t| t.auto_start).collect();
                    }
                }
            }

            if final_pwd.is_none() {
                final_pwd = session::get_password("__global__").ok().flatten();
            }

            // Auto-start any configured tunnels for this session
            for tun_cfg in tunnels_to_start {
                let req = ConnectRequest {
                    host: host.clone(),
                    port,
                    username: final_user.clone(),
                    password: final_pwd.clone(),
                    log_directory: sess_log_dir.clone(),
                    auth_method: auth_method.clone(),
                    private_key_name: priv_key.clone(),
                    private_key_passphrase: None,
                };
                let _ = tunnels::TUNNEL_MANAGER.start_tunnel(tun_cfg, req).await;
            }

            connect_ssh_session(
                app_weak,
                state,
                host,
                port,
                final_user,
                final_pwd,
                auth_method,
                priv_key,
                None,
                sess_log_dir,
            )
            .await;
        });
    });

    // Setup Save Settings callback
    let app_weak = app.as_weak();
    let exp_sess_save = expanded_session_folders.clone();
    let state_save = app_state.clone();
    app.on_request_save_settings(
        move |u,
              pwd,
              port_str,
              timeout_str,
              keepalive_str,
              log_en,
              log_d,
              log_fmt,
              log_ts,
              font_fam,
              font_sz,
              scroll_l,
              cur_bl,
              conf_cl,
              rest_sess,
              rem_f,
              single_cl,
              th_m,
              col_sch,
              acc_col| {
            let mut app_cfg = settings::load_settings();
            app_cfg.global_username = if u.trim().is_empty() {
                None
            } else {
                Some(u.to_string())
            };
            if let Ok(p) = port_str.parse::<u16>() {
                app_cfg.default_port = p;
            }
            if let Ok(t) = timeout_str.parse::<u32>() {
                app_cfg.connection_timeout = t;
            }
            if let Ok(k) = keepalive_str.parse::<u32>() {
                app_cfg.keepalive_interval = k;
            }
            app_cfg.logging_enabled = log_en;
            app_cfg.log_directory = if log_d.trim().is_empty() {
                None
            } else {
                Some(log_d.to_string())
            };
            app_cfg.log_format = log_fmt.to_string();
            app_cfg.log_timestamps = log_ts;
            let clean_font_fam = font_fam.split(',').next().unwrap_or("JetBrains Mono").trim().to_string();
            let clean_font_fam = if clean_font_fam.is_empty() { "JetBrains Mono".to_string() } else { clean_font_fam };
            app_cfg.font_family = clean_font_fam.clone();
            let fs_val = font_sz.parse::<u32>().unwrap_or(14);
            app_cfg.font_size = fs_val;
            state_save
                .current_terminal_font_size
                .store(fs_val, std::sync::atomic::Ordering::Relaxed);
            *state_save.current_terminal_font_family.write() = clean_font_fam.clone();

            if let Ok(sl) = scroll_l.parse::<u32>() {
                app_cfg.scrollback_lines = sl;
            }
            app_cfg.cursor_blink = cur_bl;
            app_cfg.confirm_on_close = conf_cl;
            app_cfg.restore_sessions = rest_sess;
            app_cfg.remember_expanded_folders = rem_f;
            app_cfg.single_click_launch = single_cl;
            app_cfg.theme = th_m.to_string();
            app_cfg.color_scheme = col_sch.to_string();
            app_cfg.accent_color = acc_col.to_string();
            if rem_f {
                let exp = exp_sess_save.lock();
                app_cfg.expanded_folders = exp.expanded.iter().cloned().collect();
            } else {
                app_cfg.expanded_folders.clear();
            }

            let is_dark_mode = theme::ThemeMode::from_str(&th_m) == theme::ThemeMode::Dark
                || (theme::ThemeMode::from_str(&th_m) == theme::ThemeMode::System
                    && theme::detect_system_is_dark());
            let effective_scheme = theme::get_effective_scheme(&col_sch, is_dark_mode);
            app_cfg.color_scheme = effective_scheme.clone();

            let _ = settings::save_settings(&app_cfg);
            let pwd_trimmed = pwd.trim();
            if !pwd_trimmed.is_empty() {
                let _ = session::save_password("__global__", pwd_trimmed);
            } else {
                let _ = session::delete_password("__global__");
            }
            if let Some(app) = app_weak.upgrade() {
                let theme_global = app.global::<Theme>();
                let (eff, _) = theme::apply_theme(&theme_global, &th_m, &col_sch, &acc_col);
                app.set_settings_font_family(clean_font_fam.into());
                app.set_settings_font_size(font_sz.clone());
                app.set_settings_theme_mode(th_m.clone());
                app.set_settings_color_scheme(eff.into());
                app.set_settings_accent_color(acc_col.clone());
                app.set_settings_single_click_launch(single_cl);
                app.set_settings_global_password(pwd_trimmed.into());
                let h = app.invoke_get_terminal_viewport_height();
                let w = app.invoke_get_terminal_viewport_width();
                update_terminal_geometry(&app, &state_save, h, w);
                app.set_status_text("Settings saved successfully".into());
            }
        },
    );

    // Live Theme Mode change preview callback
    let app_weak = app.as_weak();
    app.on_request_theme_mode_changed(move |mode| {
        if let Some(app) = app_weak.upgrade() {
            let scheme = app.get_settings_color_scheme().to_string();
            let accent = app.get_settings_accent_color().to_string();
            let theme_global = app.global::<Theme>();
            let (effective_scheme, _is_dark) =
                theme::apply_theme(&theme_global, &mode, &scheme, &accent);
            app.set_settings_color_scheme(effective_scheme.into());
        }
    });

    // Live Color Scheme change preview callback
    let app_weak = app.as_weak();
    app.on_request_color_scheme_changed(move |scheme| {
        if let Some(app) = app_weak.upgrade() {
            let mode = app.get_settings_theme_mode().to_string();
            let accent = app.get_settings_accent_color().to_string();
            let theme_global = app.global::<Theme>();
            let (effective_scheme, _is_dark) =
                theme::apply_theme(&theme_global, &mode, &scheme, &accent);
            app.set_settings_color_scheme(effective_scheme.into());
        }
    });

    // Live Accent Color change preview callback
    let app_weak = app.as_weak();
    app.on_request_accent_color_changed(move |accent| {
        if let Some(app) = app_weak.upgrade() {
            let mode = app.get_settings_theme_mode().to_string();
            let scheme = app.get_settings_color_scheme().to_string();
            let theme_global = app.global::<Theme>();
            let (effective_scheme, _is_dark) =
                theme::apply_theme(&theme_global, &mode, &scheme, &accent);
            app.set_settings_color_scheme(effective_scheme.into());
        }
    });

    // Quick Theme Toggle callback from Menubar Options
    let app_weak = app.as_weak();
    app.on_request_toggle_theme(move || {
        if let Some(app) = app_weak.upgrade() {
            let mut app_cfg = settings::load_settings();
            let current_is_dark = app.global::<Theme>().get_is_dark();
            let new_mode = if current_is_dark { "light" } else { "dark" };
            let new_scheme = if current_is_dark {
                "Clean Light"
            } else {
                "GitHub Dark"
            };
            app_cfg.theme = new_mode.to_string();
            app_cfg.color_scheme = new_scheme.to_string();
            let _ = settings::save_settings(&app_cfg);

            let theme_global = app.global::<Theme>();
            let (effective_scheme, is_dark) =
                theme::apply_theme(&theme_global, new_mode, new_scheme, &app_cfg.accent_color);
            app.set_settings_theme_mode(new_mode.into());
            app.set_settings_color_scheme(effective_scheme.into());
            app.set_status_text(
                format!(
                    "Switched to {} theme",
                    if is_dark { "Dark" } else { "Light" }
                )
                .into(),
            );
        }
    });

    // Setup Browse Log Directory callback
    let app_weak = app.as_weak();
    app.on_request_browse_log_dir(move || {
        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
            if let Some(app) = app_weak.upgrade() {
                app.set_settings_log_dir(folder.to_string_lossy().to_string().into());
            }
        }
    });

    // Setup Open Log Directory callback
    let app_weak = app.as_weak();
    app.on_request_open_log_dir(move || {
        if let Some(app) = app_weak.upgrade() {
            let dir_str = app.get_settings_log_dir().to_string();
            let p =
                if !dir_str.trim().is_empty() && settings::is_valid_path_for_current_os(&dir_str) {
                    std::path::PathBuf::from(dir_str)
                } else {
                    settings::get_default_log_dir()
                };
            let _ = std::fs::create_dir_all(&p);
            let _ = open::that(&p);
        }
    });

    // Setup Copy Selection callback
    let app_weak = app.as_weak();
    app.on_request_copy_selection(move |sel| {
        let text = sel.to_string();
        if !text.is_empty() {
            let formatted = clipboard::format_text_for_system_clipboard(&text);
            let _ = clipboard::set_text(&formatted);
            if let Some(app) = app_weak.upgrade() {
                app.set_status_text("Copied selection to clipboard".into());
            }
        }
    });

    // Setup Copy Selected Range callback
    let state_clone = app_state.clone();
    let app_weak = app.as_weak();
    app.on_request_copy_selected_range(move |start, end| {
        if let Some(app) = app_weak.upgrade() {
            let active_idx = app.get_active_tab_index() as usize;
            let tabs = app.get_tabs();
            if active_idx < tabs.row_count() {
                if let Some(tab) = tabs.row_data(active_idx) {
                    let sid = tab.id.to_string();
                    let buffers = state_clone.terminal_buffers.read();
                    if let Some(buf) = buffers.get(&sid) {
                        let b = buf.lock();
                        let (text, cur_off, _, _, _) = b.get_visible_text();
                        drop(b);

                        let start_idx = (start as usize).min(text.len());
                        let end_idx = (end as usize).min(text.len());
                        if start_idx < end_idx {
                            let slice = safe_byte_slice(&text, start_idx, end_idx);
                            if !slice.is_empty() {
                                let formatted = clipboard::format_text_for_system_clipboard(slice);
                                if let Ok(()) = clipboard::set_text(&formatted) {
                                    app.set_status_text("Copied selection to clipboard".into());
                                }
                            }
                        }
                        app.invoke_set_terminal_cursor_pos(cur_off as i32);
                    }
                }
            }
        }
    });

    // Setup Clipboard Copy callback
    let state_clone = app_state.clone();
    let app_weak = app.as_weak();
    app.on_request_clipboard_copy(move || {
        if let Some(app) = app_weak.upgrade() {
            let active_idx = app.get_active_tab_index() as usize;
            let tabs = app.get_tabs();
            if active_idx < tabs.row_count() {
                if let Some(tab) = tabs.row_data(active_idx) {
                    let sid = tab.id.to_string();
                    let buffers = state_clone.terminal_buffers.read();
                    if let Some(buf) = buffers.get(&sid) {
                        let b = buf.lock();
                        let text = b.get_lines().join("\n");
                        let (_, cur_off, _, _, _) = b.get_visible_text();
                        drop(b);
                        let formatted = clipboard::format_text_for_system_clipboard(&text);
                        if let Ok(()) = clipboard::set_text(&formatted) {
                            app.set_status_text("Copied terminal lines to clipboard".into());
                        }
                        app.invoke_set_terminal_cursor_pos(cur_off as i32);
                    }
                }
            }
        }
    });

    // Setup Clipboard Paste callback
    let state_clone = app_state.clone();
    let app_weak = app.as_weak();
    let rt_handle_paste = rt.handle().clone();
    app.on_request_clipboard_paste(move || {
        if let Ok(raw_text) = clipboard::get_text() {
            if raw_text.is_empty() {
                return;
            }
            if let Some(app) = app_weak.upgrade() {
                let active_idx = app.get_active_tab_index() as usize;
                let tabs = app.get_tabs();
                if active_idx < tabs.row_count() {
                    if let Some(tab) = tabs.row_data(active_idx) {
                        let sid = tab.id.to_string();
                        let senders = state_clone.input_senders.read();
                        if let Some(tx) = senders.get(&sid).cloned() {
                            let commands = clipboard::prepare_paste_commands(&raw_text);
                            let count = commands.len();
                            if count > 0 {
                                rt_handle_paste.spawn(async move {
                                    for (i, cmd) in commands.into_iter().enumerate() {
                                        if i > 0 {
                                            // 25ms delay between consecutive lines to prevent buffer overflow on Cisco / Adtran CLI
                                            tokio::time::sleep(tokio::time::Duration::from_millis(25)).await;
                                        }
                                        let mut bytes = cmd.text.into_bytes();
                                        if cmd.submit {
                                            bytes.push(b'\r');
                                        }
                                        if tx.send(ssh::SshInput::Data(bytes)).is_err() {
                                            break;
                                        }
                                    }
                                });

                                if count > 1 {
                                    app.set_status_text(format!("Pasted {} lines into terminal", count).into());
                                } else {
                                    app.set_status_text("Pasted clipboard text into terminal".into());
                                }
                            }
                        }
                        let buffers = state_clone.terminal_buffers.read();
                        if let Some(buf) = buffers.get(&sid) {
                            let b = buf.lock();
                            let (_, cur_off, _, _, _) = b.get_visible_text();
                            drop(b);
                            app.invoke_set_terminal_cursor_pos(cur_off as i32);
                        }
                    }
                }
            }
        }
    });

    // Setup Clear Terminal callback
    let state_clone = app_state.clone();
    let app_weak = app.as_weak();
    app.on_request_clear_terminal(move || {
        if let Some(app) = app_weak.upgrade() {
            let active_idx = app.get_active_tab_index() as usize;
            let tabs = app.get_tabs();
            if active_idx < tabs.row_count() {
                if let Some(tab) = tabs.row_data(active_idx) {
                    let sid = tab.id.to_string();
                    let buffers = state_clone.terminal_buffers.read();
                    if let Some(buf) = buffers.get(&sid) {
                        buf.lock().clear();
                        let empty_lines = encounter_empty();
                        let empty_text = empty_lines
                            .iter()
                            .map(|s| s.as_str())
                            .collect::<Vec<_>>()
                            .join("\n");
                        let len = empty_text.len() as i32;
                        app.set_terminal_lines(ModelRc::new(VecModel::from(empty_lines)));
                        app.set_terminal_full_text(empty_text.into());
                        app.invoke_set_terminal_cursor_pos(len);
                    }
                }
            }
        }
    });

    // Setup Exit callback
    app.on_request_exit(move || {
        std::process::exit(0);
    });

    // Setup Terminal Key Event forwarding
    let state_clone = app_state.clone();
    let app_weak = app.as_weak();
    app.on_request_terminal_key(move |evt| {
        let bytes = translate_slint_key(&evt.text, evt.modifiers.control, evt.modifiers.alt);
        if bytes.is_empty() {
            return;
        }
        if let Some(app) = app_weak.upgrade() {
            let active_idx = app.get_active_tab_index() as usize;
            let tabs = app.get_tabs();
            if active_idx < tabs.row_count() {
                if let Some(tab) = tabs.row_data(active_idx) {
                    let sid = tab.id.to_string();
                    let senders = state_clone.input_senders.read();
                    if let Some(tx) = senders.get(&sid) {
                        let _ = tx.send(ssh::SshInput::Data(bytes));
                    }
                    if let Some(buf) = state_clone.terminal_buffers.read().get(&sid) {
                        let mut b = buf.lock();
                        b.scroll_to_bottom();
                        let (text, cur_off, total, scroll_off, vis_rows) = b.get_visible_text();
                        drop(b);
                        app.set_terminal_scroll_total(total as i32);
                        app.set_terminal_scroll_offset(scroll_off as i32);
                        app.set_terminal_scroll_visible(vis_rows as i32);
                        app.set_terminal_full_text(text.into());
                        app.invoke_set_terminal_cursor_pos(cur_off as i32);
                    }
                }
            }
        }
    });

    // Setup Cursor Clicked callback for single mouse clicks
    let state_clone = app_state.clone();
    let app_weak = app.as_weak();
    app.on_request_cursor_clicked(move |offset| {
        if let Some(app) = app_weak.upgrade() {
            let active_idx = app.get_active_tab_index() as usize;
            let tabs = app.get_tabs();
            if active_idx < tabs.row_count() {
                if let Some(tab) = tabs.row_data(active_idx) {
                    let sid = tab.id.to_string();
                    let senders = state_clone.input_senders.read();
                    let buffers = state_clone.terminal_buffers.read();
                    if let (Some(tx), Some(buf)) = (senders.get(&sid), buffers.get(&sid)) {
                        sync_remote_cursor(tx, buf, offset as usize);
                        let b = buf.lock();
                        let (text, cur_off, total, scroll_off, vis_rows) = b.get_visible_text();
                        drop(b);
                        app.set_terminal_scroll_total(total as i32);
                        app.set_terminal_scroll_offset(scroll_off as i32);
                        app.set_terminal_scroll_visible(vis_rows as i32);
                        app.set_terminal_full_text(text.into());
                        app.invoke_set_terminal_cursor_pos(cur_off as i32);
                    }
                }
            }
        }
    });

    // Setup Terminal Scroll Event (Mouse Wheel)
    let state_clone = app_state.clone();
    let app_weak = app.as_weak();
    app.on_request_terminal_scrolled(move |delta_y| {
        if let Some(app) = app_weak.upgrade() {
            let active_idx = app.get_active_tab_index() as usize;
            let tabs = app.get_tabs();
            if active_idx < tabs.row_count() {
                if let Some(tab) = tabs.row_data(active_idx) {
                    let sid = tab.id.to_string();
                    let buffers = state_clone.terminal_buffers.read();
                    if let Some(buf) = buffers.get(&sid) {
                        let mut b = buf.lock();
                        let lines_delta = if delta_y > 0.0 { 3 } else { -3 };
                        b.scroll_by(lines_delta);
                        let (text, offset, total, scroll_off, vis_rows) = b.get_visible_text();
                        drop(b);
                        app.set_terminal_scroll_total(total as i32);
                        app.set_terminal_scroll_offset(scroll_off as i32);
                        app.set_terminal_scroll_visible(vis_rows as i32);
                        app.set_terminal_full_text(text.into());
                        app.invoke_set_terminal_cursor_pos(offset as i32);
                    }
                }
            }
        }
    });

    // Setup Terminal Scrollbar Drag Event
    let state_clone = app_state.clone();
    let app_weak = app.as_weak();
    app.on_request_scrollbar_dragged(move |ratio| {
        if let Some(app) = app_weak.upgrade() {
            let active_idx = app.get_active_tab_index() as usize;
            let tabs = app.get_tabs();
            if active_idx < tabs.row_count() {
                if let Some(tab) = tabs.row_data(active_idx) {
                    let sid = tab.id.to_string();
                    let buffers = state_clone.terminal_buffers.read();
                    if let Some(buf) = buffers.get(&sid) {
                        let mut b = buf.lock();
                        b.scroll_to_ratio(ratio);
                        let (text, offset, total, scroll_off, vis_rows) = b.get_visible_text();
                        drop(b);
                        app.set_terminal_scroll_total(total as i32);
                        app.set_terminal_scroll_offset(scroll_off as i32);
                        app.set_terminal_scroll_visible(vis_rows as i32);
                        app.set_terminal_full_text(text.into());
                        app.invoke_set_terminal_cursor_pos(offset as i32);
                    }
                }
            }
        }
    });

    // Setup Terminal Viewport Resize Handler (fires from Slint changed-height and changed-width on viewport container)
    // Uses a last-dimension debounce so text-content updates don't cause a re-render feedback loop.
    let state_clone = app_state.clone();
    let app_weak = app.as_weak();
    let last_dims: Arc<std::sync::Mutex<(f32, f32)>> = Arc::new(std::sync::Mutex::new((0.0, 0.0)));
    app.on_request_terminal_resized(move |term_h_px, term_w_px| {
        // Only act if dimensions actually changed by more than 2px (real window resize)
        {
            let mut last = last_dims.lock().unwrap();
            if (term_h_px - last.0).abs() < 2.0 && (term_w_px - last.1).abs() < 2.0 {
                return;
            }
            *last = (term_h_px, term_w_px);
        }

        if let Some(app) = app_weak.upgrade() {
            update_terminal_geometry(&app, &state_clone, term_h_px, term_w_px);
        }
    });

    // Setup Terminal Click Cursor Syncing
    let state_clone = app_state.clone();
    let app_weak = app.as_weak();
    app.on_request_cursor_clicked(move |target_offset| {
        if let Some(app) = app_weak.upgrade() {
            let active_idx = app.get_active_tab_index() as usize;
            let tabs = app.get_tabs();
            if active_idx < tabs.row_count() {
                if let Some(tab) = tabs.row_data(active_idx) {
                    let sid = tab.id.to_string();
                    let senders = state_clone.input_senders.read();
                    let buffers = state_clone.terminal_buffers.read();
                    if let (Some(tx), Some(buf)) = (senders.get(&sid), buffers.get(&sid)) {
                        sync_remote_cursor(tx, buf, target_offset as usize);
                    }
                }
            }
        }
    });

    // Setup Tab Add callback
    let app_weak = app.as_weak();
    let state_clone = app_state.clone();
    app.on_request_tab_added(move || {
        if let Some(app) = app_weak.upgrade() {
            let tabs = app.get_tabs();
            let mut new_tabs: Vec<TabModel> = Vec::new();
            for i in 0..tabs.row_count() {
                if let Some(t) = tabs.row_data(i) {
                    new_tabs.push(t);
                }
            }
            let new_id = format!("tab-{}", Uuid::new_v4());
            new_tabs.push(make_tab_model(
                &new_id,
                &format!("Terminal {}", new_tabs.len() + 1),
                false,
                true,
                "",
                Some(&state_clone),
            ));
            let count = new_tabs.len();
            app.set_tabs(ModelRc::new(VecModel::from(new_tabs)));
            app.set_active_tab_index((count - 1) as i32);
            let empty_lines = encounter_empty();
            let empty_text = empty_lines
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            let len = empty_text.len() as i32;
            app.set_terminal_lines(ModelRc::new(VecModel::from(empty_lines)));
            app.set_terminal_full_text(empty_text.into());
            app.invoke_set_terminal_cursor_pos(len);
            app.invoke_focus_terminal();
            sync_tab_groups_ui(&app, &state_clone);
            app.set_status_text("New Terminal Tab Ready".into());
        }
    });

    // Setup Tab Close callback
    let app_weak = app.as_weak();
    let state_clone = app_state.clone();
    app.on_request_tab_closed(move |idx| {
        if let Some(app) = app_weak.upgrade() {
            let tabs = app.get_tabs();
            let idx = idx as usize;
            if idx < tabs.row_count() {
                if let Some(tab) = tabs.row_data(idx) {
                    let sid = tab.id.to_string();
                    state_clone.connections.write().remove(&sid);
                    state_clone.input_senders.write().remove(&sid);
                    state_clone.terminal_buffers.write().remove(&sid);
                    tunnels::TUNNEL_MANAGER.stop_session_tunnels(&sid);
                    // Remove from any tab group
                    let mut groups = state_clone.tab_groups.write();
                    for g in groups.values_mut() {
                        g.tab_ids.retain(|id| id != &sid);
                    }
                    groups.retain(|_, g| !g.tab_ids.is_empty());
                }
                let mut new_tabs: Vec<TabModel> = Vec::new();
                for i in 0..tabs.row_count() {
                    if i != idx {
                        if let Some(t) = tabs.row_data(i) {
                            new_tabs.push(t);
                        }
                    }
                }
                if new_tabs.is_empty() {
                    new_tabs.push(make_tab_model(
                        &format!("tab-{}", Uuid::new_v4()),
                        "Terminal 1",
                        false,
                        true,
                        "",
                        Some(&state_clone),
                    ));
                }
                let new_active = if idx >= new_tabs.len() {
                    (new_tabs.len() - 1) as i32
                } else {
                    idx as i32
                };
                app.set_tabs(ModelRc::new(VecModel::from(new_tabs)));
                app.set_active_tab_index(new_active);
                sync_tab_groups_ui(&app, &state_clone);

                let tabs_after = app.get_tabs();
                if let Some(t) = tabs_after.row_data(new_active as usize) {
                    let sid = t.id.to_string();
                    let buffers = state_clone.terminal_buffers.read();
                    if let Some(buf) = buffers.get(&sid) {
                        let (full_text, offset, total, scroll_off, vis_rows) =
                            buf.lock().get_visible_text();
                        app.set_terminal_scroll_total(total as i32);
                        app.set_terminal_scroll_offset(scroll_off as i32);
                        app.set_terminal_scroll_visible(vis_rows as i32);
                        app.set_terminal_full_text(full_text.into());
                        app.invoke_set_terminal_cursor_pos(offset as i32);
                    } else {
                        let empty_lines = encounter_empty();
                        let empty_text = empty_lines
                            .iter()
                            .map(|s| s.as_str())
                            .collect::<Vec<_>>()
                            .join("\n");
                        let len = empty_text.len() as i32;
                        app.set_terminal_lines(ModelRc::new(VecModel::from(empty_lines)));
                        app.set_terminal_full_text(empty_text.into());
                        app.invoke_set_terminal_cursor_pos(len);
                    }
                }
            }
        }
    });

    // Setup Tab Selected callback
    let app_weak = app.as_weak();
    let state_clone = app_state.clone();
    app.on_request_tab_selected(move |idx| {
        if let Some(app) = app_weak.upgrade() {
            app.set_active_tab_index(idx);
            app.invoke_focus_terminal();
            let tabs = app.get_tabs();
            let idx = idx as usize;
            if idx < tabs.row_count() {
                if let Some(tab) = tabs.row_data(idx) {
                    let sid = tab.id.to_string();
                    let buffers = state_clone.terminal_buffers.read();
                    if let Some(buf_arc) = buffers.get(&sid) {
                        let (full_text, offset, total, scroll_off, vis_rows) =
                            buf_arc.lock().get_visible_text();
                        app.set_terminal_scroll_total(total as i32);
                        app.set_terminal_scroll_offset(scroll_off as i32);
                        app.set_terminal_scroll_visible(vis_rows as i32);
                        app.set_terminal_full_text(full_text.into());
                        app.invoke_set_terminal_cursor_pos(offset as i32);
                        app.invoke_focus_terminal();
                    } else {
                        let empty_lines = encounter_empty();
                        let empty_text = empty_lines
                            .iter()
                            .map(|s| s.as_str())
                            .collect::<Vec<_>>()
                            .join("\n");
                        let len = empty_text.len() as i32;
                        app.set_terminal_lines(ModelRc::new(VecModel::from(empty_lines)));
                        app.set_terminal_full_text(empty_text.into());
                        app.invoke_set_terminal_cursor_pos(len);
                    }
                }
            }
        }
    });

    // Setup Tab Rename callback
    let app_weak = app.as_weak();
    app.on_request_tab_rename(move |idx, title| {
        if let Some(app) = app_weak.upgrade() {
            let tabs = app.get_tabs();
            let idx = idx as usize;
            if idx < tabs.row_count() {
                let mut new_tabs: Vec<TabModel> = Vec::new();
                for i in 0..tabs.row_count() {
                    if let Some(mut t) = tabs.row_data(i) {
                        if i == idx {
                            t.title = title.clone();
                        }
                        new_tabs.push(t);
                    }
                }
                app.set_tabs(ModelRc::new(VecModel::from(new_tabs)));
            }
        }
    });

    // Setup Tab Reset Name callback
    let app_weak = app.as_weak();
    app.on_request_tab_reset_name(move |idx| {
        if let Some(app) = app_weak.upgrade() {
            let tabs = app.get_tabs();
            let idx = idx as usize;
            if idx < tabs.row_count() {
                let mut new_tabs: Vec<TabModel> = Vec::new();
                for i in 0..tabs.row_count() {
                    if let Some(mut t) = tabs.row_data(i) {
                        if i == idx {
                            t.title = if !t.host.is_empty() {
                                t.host.clone()
                            } else {
                                format!("Terminal {}", i + 1).into()
                            };
                        }
                        new_tabs.push(t);
                    }
                }
                app.set_tabs(ModelRc::new(VecModel::from(new_tabs)));
            }
        }
    });

    // Setup Tab Reconnect callback
    let app_weak = app.as_weak();
    let state_clone = app_state.clone();
    let rt_handle = rt.handle().clone();
    app.on_request_tab_reconnect(move |idx| {
        let app_weak = app_weak.clone();
        let state = state_clone.clone();
        if let Some(app) = app_weak.upgrade() {
            let tabs = app.get_tabs();
            let idx = idx as usize;
            if idx < tabs.row_count() {
                if let Some(tab) = tabs.row_data(idx) {
                    let host = tab.host.to_string();
                    if !host.is_empty() {
                        let sid = tab.id.to_string();
                        state.connections.write().remove(&sid);
                        state.input_senders.write().remove(&sid);
                        let app_cfg = settings::load_settings();
                        let user = app_cfg
                            .global_username
                            .unwrap_or_else(|| "root".to_string());
                        let pwd = session::get_password("__global__").ok().flatten();
                        rt_handle.spawn(async move {
                            connect_ssh_session(
                                app_weak,
                                state,
                                host,
                                22,
                                user,
                                pwd,
                                "password".to_string(),
                                None,
                                None,
                                None,
                            )
                            .await;
                        });
                    }
                }
            }
        }
    });

    // Setup Tab Disconnect callback
    let app_weak = app.as_weak();
    let state_clone = app_state.clone();
    app.on_request_tab_disconnect(move |idx| {
        let app_weak = app_weak.clone();
        let state = state_clone.clone();
        if let Some(app) = app_weak.upgrade() {
            let tabs = app.get_tabs();
            let idx = idx as usize;
            if idx < tabs.row_count() {
                if let Some(mut tab) = tabs.row_data(idx) {
                    let sid = tab.id.to_string();
                    state.connections.write().remove(&sid);
                    state.input_senders.write().remove(&sid);
                    tunnels::TUNNEL_MANAGER.stop_session_tunnels(&sid);

                    let buffers = state.terminal_buffers.read();
                    if let Some(buf) = buffers.get(&sid) {
                        buf.lock()
                            .process_bytes(b"\r\n\x1b[33m=== Session Disconnected ===\x1b[0m\r\n");
                    }
                    drop(buffers);

                    tab.connected = false;
                    let mut new_tabs: Vec<TabModel> = Vec::new();
                    for i in 0..tabs.row_count() {
                        if i == idx {
                            new_tabs.push(tab.clone());
                        } else if let Some(t) = tabs.row_data(i) {
                            new_tabs.push(t);
                        }
                    }
                    app.set_tabs(ModelRc::new(VecModel::from(new_tabs)));

                    let active_idx = app.get_active_tab_index() as usize;
                    if active_idx == idx {
                        let buffers = state.terminal_buffers.read();
                        if let Some(buf) = buffers.get(&sid) {
                            let (full_text, offset, total, scroll_off, vis_rows) =
                                buf.lock().get_visible_text();
                            app.set_terminal_scroll_total(total as i32);
                            app.set_terminal_scroll_offset(scroll_off as i32);
                            app.set_terminal_scroll_visible(vis_rows as i32);
                            app.set_terminal_full_text(full_text.into());
                            app.invoke_set_terminal_cursor_pos(offset as i32);
                        }
                        app.set_status_text("Session Disconnected".into());
                    }
                }
            }
        }
    });

    // Setup Close Disconnected Tabs callback
    let app_weak = app.as_weak();
    let state_clone = app_state.clone();
    app.on_request_tab_close_disconnected(move || {
        if let Some(app) = app_weak.upgrade() {
            let tabs = app.get_tabs();
            let mut new_tabs: Vec<TabModel> = Vec::new();
            for i in 0..tabs.row_count() {
                if let Some(t) = tabs.row_data(i) {
                    if t.connected {
                        new_tabs.push(t);
                    } else {
                        let sid = t.id.to_string();
                        state_clone.connections.write().remove(&sid);
                        state_clone.input_senders.write().remove(&sid);
                        state_clone.terminal_buffers.write().remove(&sid);
                        let mut groups = state_clone.tab_groups.write();
                        for g in groups.values_mut() {
                            g.tab_ids.retain(|id| id != &sid);
                        }
                    }
                }
            }
            if new_tabs.is_empty() {
                new_tabs.push(make_tab_model(
                    &format!("tab-{}", Uuid::new_v4()),
                    "Terminal 1",
                    false,
                    true,
                    "",
                    Some(&state_clone),
                ));
            }
            app.set_tabs(ModelRc::new(VecModel::from(new_tabs)));
            app.set_active_tab_index(0);
            sync_tab_groups_ui(&app, &state_clone);
        }
    });

    // Setup Close Other Tabs callback
    let app_weak = app.as_weak();
    let state_clone = app_state.clone();
    app.on_request_tab_close_others(move |keep_idx| {
        if let Some(app) = app_weak.upgrade() {
            let tabs = app.get_tabs();
            let keep_idx = keep_idx as usize;
            if keep_idx < tabs.row_count() {
                let mut new_tabs: Vec<TabModel> = Vec::new();
                for i in 0..tabs.row_count() {
                    if let Some(t) = tabs.row_data(i) {
                        if i == keep_idx {
                            new_tabs.push(t);
                        } else {
                            let sid = t.id.to_string();
                            state_clone.connections.write().remove(&sid);
                            state_clone.input_senders.write().remove(&sid);
                            state_clone.terminal_buffers.write().remove(&sid);
                            let mut groups = state_clone.tab_groups.write();
                            for g in groups.values_mut() {
                                g.tab_ids.retain(|id| id != &sid);
                            }
                        }
                    }
                }
                app.set_tabs(ModelRc::new(VecModel::from(new_tabs)));
                app.set_active_tab_index(0);
                sync_tab_groups_ui(&app, &state_clone);
            }
        }
    });

    // Setup Close Tabs to the Right callback
    let app_weak = app.as_weak();
    let state_clone = app_state.clone();
    app.on_request_tab_close_to_right(move |from_idx| {
        if let Some(app) = app_weak.upgrade() {
            let tabs = app.get_tabs();
            let from_idx = from_idx as usize;
            let mut new_tabs: Vec<TabModel> = Vec::new();
            for i in 0..tabs.row_count() {
                if let Some(t) = tabs.row_data(i) {
                    if i <= from_idx {
                        new_tabs.push(t);
                    } else {
                        let sid = t.id.to_string();
                        state_clone.connections.write().remove(&sid);
                        state_clone.input_senders.write().remove(&sid);
                        state_clone.terminal_buffers.write().remove(&sid);
                        let mut groups = state_clone.tab_groups.write();
                        for g in groups.values_mut() {
                            g.tab_ids.retain(|id| id != &sid);
                        }
                    }
                }
            }
            let active = app.get_active_tab_index() as usize;
            let new_active = if active > from_idx {
                from_idx as i32
            } else {
                active as i32
            };
            app.set_tabs(ModelRc::new(VecModel::from(new_tabs)));
            app.set_active_tab_index(new_active);
            sync_tab_groups_ui(&app, &state_clone);
        }
    });

    // Setup Close All Tabs callback
    let app_weak = app.as_weak();
    let state_clone = app_state.clone();
    app.on_request_tab_close_all(move || {
        if let Some(app) = app_weak.upgrade() {
            let tabs = app.get_tabs();
            for i in 0..tabs.row_count() {
                if let Some(t) = tabs.row_data(i) {
                    let sid = t.id.to_string();
                    state_clone.connections.write().remove(&sid);
                    state_clone.input_senders.write().remove(&sid);
                    state_clone.terminal_buffers.write().remove(&sid);
                }
            }
            state_clone.tab_groups.write().clear();
            let new_tabs = vec![make_tab_model(
                &format!("tab-{}", Uuid::new_v4()),
                "Terminal 1",
                false,
                true,
                "",
                Some(&state_clone),
            )];
            app.set_tabs(ModelRc::new(VecModel::from(new_tabs)));
            app.set_active_tab_index(0);
            sync_tab_groups_ui(&app, &state_clone);
            let empty_lines = encounter_empty();
            let empty_text = empty_lines
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            let len = empty_text.len() as i32;
            app.set_terminal_lines(ModelRc::new(VecModel::from(empty_lines)));
            app.set_terminal_full_text(empty_text.into());
            app.invoke_set_terminal_cursor_pos(len);
        }
    });

    // Setup Tab Group Callbacks
    let app_weak = app.as_weak();
    let state_clone = app_state.clone();
    app.on_request_create_tab_group(move |tab_idx, group_name| {
        let name = group_name.trim().to_string();
        if name.is_empty() {
            return;
        }
        if let Some(app) = app_weak.upgrade() {
            let tabs = app.get_tabs();
            let tab_idx = tab_idx as usize;
            let mut tab_id = String::new();
            if tab_idx < tabs.row_count() {
                if let Some(t) = tabs.row_data(tab_idx) {
                    tab_id = t.id.to_string();
                }
            }

            {
                let mut groups = state_clone.tab_groups.write();
                let next_idx = groups.len();
                let mut new_grp = app_state::TabGroup::new(name.clone(), next_idx);
                if !tab_id.is_empty() {
                    for g in groups.values_mut() {
                        g.tab_ids.retain(|id| id != &tab_id);
                    }
                    new_grp.tab_ids.push(tab_id);
                }
                groups.insert(new_grp.id.clone(), new_grp);
            }

            sync_tab_groups_ui(&app, &state_clone);
            app.set_status_text(format!("Created tab group \"{}\"", name).into());
        }
    });

    let app_weak = app.as_weak();
    let state_clone = app_state.clone();
    app.on_request_add_tab_to_group(move |tab_idx, group_id| {
        if let Some(app) = app_weak.upgrade() {
            let tabs = app.get_tabs();
            let tab_idx = tab_idx as usize;
            if tab_idx < tabs.row_count() {
                if let Some(t) = tabs.row_data(tab_idx) {
                    let tab_id = t.id.to_string();
                    let gid = group_id.to_string();
                    let mut groups = state_clone.tab_groups.write();
                    for g in groups.values_mut() {
                        g.tab_ids.retain(|id| id != &tab_id);
                    }
                    if let Some(target_g) = groups.get_mut(&gid) {
                        target_g.tab_ids.push(tab_id);
                    }
                }
            }
            sync_tab_groups_ui(&app, &state_clone);
        }
    });

    let app_weak = app.as_weak();
    let state_clone = app_state.clone();
    app.on_request_remove_tab_from_group(move |tab_idx| {
        if let Some(app) = app_weak.upgrade() {
            let tabs = app.get_tabs();
            let tab_idx = tab_idx as usize;
            if tab_idx < tabs.row_count() {
                if let Some(t) = tabs.row_data(tab_idx) {
                    let tab_id = t.id.to_string();
                    let mut groups = state_clone.tab_groups.write();
                    for g in groups.values_mut() {
                        g.tab_ids.retain(|id| id != &tab_id);
                    }
                    groups.retain(|_, g| !g.tab_ids.is_empty());
                }
            }
            sync_tab_groups_ui(&app, &state_clone);
            app.set_status_text("Removed tab from group".into());
        }
    });

    let app_weak = app.as_weak();
    let state_clone = app_state.clone();
    app.on_request_rename_tab_group(move |group_id, new_name| {
        let name = new_name.trim().to_string();
        if name.is_empty() {
            return;
        }
        if let Some(app) = app_weak.upgrade() {
            {
                let mut groups = state_clone.tab_groups.write();
                if let Some(g) = groups.get_mut(group_id.as_str()) {
                    g.name = name.clone();
                }
            }
            sync_tab_groups_ui(&app, &state_clone);
            app.set_status_text(format!("Renamed group to \"{}\"", name).into());
        }
    });

    let app_weak = app.as_weak();
    let state_clone = app_state.clone();
    app.on_request_close_tab_group(move |group_id| {
        if let Some(app) = app_weak.upgrade() {
            let gid = group_id.to_string();
            let removed_tab_ids = {
                let mut groups = state_clone.tab_groups.write();
                if let Some(g) = groups.remove(&gid) {
                    g.tab_ids
                } else {
                    Vec::new()
                }
            };

            for tid in &removed_tab_ids {
                state_clone.connections.write().remove(tid);
                state_clone.input_senders.write().remove(tid);
                state_clone.terminal_buffers.write().remove(tid);
            }

            let tabs = app.get_tabs();
            let mut new_tabs: Vec<TabModel> = Vec::new();
            for i in 0..tabs.row_count() {
                if let Some(t) = tabs.row_data(i) {
                    if !removed_tab_ids.contains(&t.id.to_string()) {
                        new_tabs.push(t);
                    }
                }
            }
            if new_tabs.is_empty() {
                new_tabs.push(make_tab_model(
                    &format!("tab-{}", Uuid::new_v4()),
                    "Terminal 1",
                    false,
                    true,
                    "",
                    Some(&state_clone),
                ));
            }
            app.set_tabs(ModelRc::new(VecModel::from(new_tabs)));
            app.set_active_tab_index(0);
            sync_tab_groups_ui(&app, &state_clone);
            app.set_status_text("Closed tab group".into());
        }
    });

    let app_weak = app.as_weak();
    let state_clone = app_state.clone();
    app.on_request_broadcast_to_group(move |group_id| {
        if let Some(app) = app_weak.upgrade() {
            let gid = group_id.to_string();
            app.set_broadcast_selected_scope(gid.clone().into());
            let groups = state_clone.tab_groups.read();
            if let Some(g) = groups.get(&gid) {
                app.set_broadcast_selected_group_name(g.name.clone().into());
                let senders = state_clone.input_senders.read();
                let active_count = g
                    .tab_ids
                    .iter()
                    .filter(|id| senders.contains_key(*id))
                    .count();
                app.set_broadcast_target_count(active_count as i32);
            }
        }
    });

    let app_weak = app.as_weak();
    let state_clone = app_state.clone();
    app.on_request_broadcast_scope_changed(move |scope| {
        if let Some(app) = app_weak.upgrade() {
            let sc = scope.to_string();
            if sc == "all" {
                let senders = state_clone.input_senders.read();
                app.set_broadcast_target_count(senders.len() as i32);
                app.set_broadcast_selected_group_name("".into());
            } else {
                let groups = state_clone.tab_groups.read();
                if let Some(g) = groups.get(&sc) {
                    app.set_broadcast_selected_group_name(g.name.clone().into());
                    let senders = state_clone.input_senders.read();
                    let active_count = g
                        .tab_ids
                        .iter()
                        .filter(|id| senders.contains_key(*id))
                        .count();
                    app.set_broadcast_target_count(active_count as i32);
                }
            }
        }
    });

    // Setup Clone Tab Session callback
    let app_weak = app.as_weak();
    let state_clone = app_state.clone();
    let rt_handle = rt.handle().clone();
    app.on_request_tab_clone(move |idx| {
        let app_weak = app_weak.clone();
        let state = state_clone.clone();
        if let Some(app) = app_weak.upgrade() {
            let tabs = app.get_tabs();
            let idx = idx as usize;
            if idx < tabs.row_count() {
                if let Some(tab) = tabs.row_data(idx) {
                    let host = tab.host.to_string();
                    if !host.is_empty() {
                        let app_cfg = settings::load_settings();
                        let user = app_cfg
                            .global_username
                            .unwrap_or_else(|| "root".to_string());
                        let pwd = session::get_password("__global__").ok().flatten();
                        rt_handle.spawn(async move {
                            connect_ssh_session(
                                app_weak,
                                state,
                                host,
                                22,
                                user,
                                pwd,
                                "password".to_string(),
                                None,
                                None,
                                None,
                            )
                            .await;
                        });
                    }
                }
            }
        }
    });

    // Setup Save Tab Session callback
    let app_weak = app.as_weak();
    app.on_request_tab_save_session(move |idx| {
        if let Some(app) = app_weak.upgrade() {
            let tabs = app.get_tabs();
            let idx = idx as usize;
            if idx < tabs.row_count() {
                if let Some(tab) = tabs.row_data(idx) {
                    let host = tab.host.to_string();
                    let title = tab.title.to_string();
                    app.set_edit_session_is_new(true);
                    app.set_edit_session_id(Uuid::new_v4().to_string().into());
                    app.set_edit_session_name(title.into());
                    app.set_edit_session_host(host.into());
                    app.set_edit_session_port("22".into());
                    app.set_edit_session_folder("".into());
                    app.set_edit_session_username("".into());
                    app.set_edit_session_password("".into());
                    app.set_edit_session_auth_method("password".into());
                    let mut ui_st = app.get_ui_state();
                    ui_st.active_modal = "session_edit".into();
                    app.set_ui_state(ui_st);
                }
            }
        }
    });

    // Setup AI Model Selected from Dropdown callback
    let app_weak = app.as_weak();
    app.on_request_ai_provider_selected(move |provider| {
        let prov_str = provider.to_string();
        let mut ai_cfg = ai::load_config();
        ai_cfg.provider = Some(prov_str.clone());
        let _ = ai::save_config(&ai_cfg);
        if let Some(app) = app_weak.upgrade() {
            app.set_ai_selected_provider(prov_str.clone().into());
            let mod_name = match prov_str.as_str() {
                "gemini" => ai_cfg
                    .gemini_model
                    .clone()
                    .unwrap_or_else(|| "gemini-3.6-flash".to_string()),
                "grok" => ai_cfg
                    .grok_model
                    .clone()
                    .unwrap_or_else(|| "grok-4.5".to_string()),
                "lmstudio" => {
                    if ai_cfg.lmstudio_model.is_empty() {
                        "local-model".to_string()
                    } else {
                        ai_cfg.lmstudio_model.clone()
                    }
                }
                _ => {
                    if ai_cfg.ollama_model.is_empty() {
                        "llama3.2".to_string()
                    } else {
                        ai_cfg.ollama_model.clone()
                    }
                }
            };
            app.set_ai_selected_model(mod_name.into());
        }
    });

    // Setup AI Message callback
    let app_weak = app.as_weak();
    let rt_handle = rt.handle().clone();
    app.on_request_ai_message(move |prompt| {
        let app_weak = app_weak.clone();
        let prompt_str = prompt.to_string();

        if let Some(app) = app_weak.upgrade() {
            let provider = app.get_ai_selected_provider().to_string();
            let model = app.get_ai_selected_model().to_string();
            let existing_messages = app.get_ai_messages();
            let mut msgs: Vec<AiChatMessageModel> = Vec::new();
            for i in 0..existing_messages.row_count() {
                if let Some(m) = existing_messages.row_data(i) {
                    msgs.push(m);
                }
            }
            msgs.push(AiChatMessageModel {
                role: "user".into(),
                content: prompt_str.clone().into(),
                timestamp: chrono::Local::now().format("%H:%M").to_string().into(),
            });
            app.set_ai_messages(ModelRc::new(VecModel::from(msgs)));
            app.set_ai_is_loading(true);

            rt_handle.spawn(async move {
                let ai_msgs: Vec<ai::AiMessage> = vec![ai::AiMessage {
                    role: "user".to_string(),
                    content: prompt_str,
                }];
                let reply = ai::chat_with_model(&provider, &model, ai_msgs)
                    .await
                    .unwrap_or_else(|e| format!("AI Error: {}", e));

                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = app_weak.upgrade() {
                        let existing = app.get_ai_messages();
                        let mut msgs: Vec<AiChatMessageModel> = Vec::new();
                        for i in 0..existing.row_count() {
                            if let Some(m) = existing.row_data(i) {
                                msgs.push(m);
                            }
                        }
                        msgs.push(AiChatMessageModel {
                            role: "model".into(),
                            content: reply.into(),
                            timestamp: chrono::Local::now().format("%H:%M").to_string().into(),
                        });
                        app.set_ai_messages(ModelRc::new(VecModel::from(msgs)));
                        app.set_ai_is_loading(false);
                    }
                });
            });
        }
    });

    // Setup AI Analyze Terminal callback
    let app_weak = app.as_weak();
    let state_clone = app_state.clone();
    let rt_handle = rt.handle().clone();
    app.on_request_ai_analyze(move || {
        let app_weak = app_weak.clone();
        let state = state_clone.clone();
        if let Some(app) = app_weak.upgrade() {
            let active_idx = app.get_active_tab_index() as usize;
            let tabs = app.get_tabs();
            if active_idx < tabs.row_count() {
                if let Some(tab) = tabs.row_data(active_idx) {
                    let sid = tab.id.to_string();
                    let terminal_text = if let Some(buf) = state.terminal_buffers.read().get(&sid) {
                        buf.lock().get_lines().join("\n")
                    } else {
                        "".to_string()
                    };

                    let prompt = format!("Please analyze this network terminal output and explain any errors, issues, or key findings:\n\n```\n{}\n```", terminal_text);
                    let provider = app.get_ai_selected_provider().to_string();
                    let model = app.get_ai_selected_model().to_string();
                    app.set_ai_is_loading(true);

                    rt_handle.spawn(async move {
                        let ai_msgs = vec![ai::AiMessage { role: "user".to_string(), content: prompt }];
                        let reply = ai::chat_with_model(&provider, &model, ai_msgs).await.unwrap_or_else(|e| format!("AI Error: {}", e));

                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = app_weak.upgrade() {
                                let existing = app.get_ai_messages();
                                let mut msgs: Vec<AiChatMessageModel> = Vec::new();
                                for i in 0..existing.row_count() {
                                    if let Some(m) = existing.row_data(i) {
                                        msgs.push(m);
                                    }
                                }
                                msgs.push(AiChatMessageModel {
                                    role: "model".into(),
                                    content: reply.into(),
                                    timestamp: chrono::Local::now().format("%H:%M").to_string().into(),
                                });
                                app.set_ai_messages(ModelRc::new(VecModel::from(msgs)));
                                app.set_ai_is_loading(false);
                            }
                        });
                    });
                }
            }
        }
    });

    // Setup AI Command Help callback
    let app_weak = app.as_weak();
    let rt_handle = rt.handle().clone();
    app.on_request_ai_help(move || {
        let app_weak = app_weak.clone();
        if let Some(app) = app_weak.upgrade() {
            let provider = app.get_ai_selected_provider().to_string();
            let model = app.get_ai_selected_model().to_string();
            app.set_ai_is_loading(true);

            rt_handle.spawn(async move {
                let ai_msgs = vec![ai::AiMessage {
                    role: "user".to_string(),
                    content: "What are the most essential troubleshooting and diagnostic commands for Cisco IOS, JunOS, and Linux systems?".to_string(),
                }];
                let reply = ai::chat_with_model(&provider, &model, ai_msgs).await.unwrap_or_else(|e| format!("AI Error: {}", e));

                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = app_weak.upgrade() {
                        let existing = app.get_ai_messages();
                        let mut msgs: Vec<AiChatMessageModel> = Vec::new();
                        for i in 0..existing.row_count() {
                            if let Some(m) = existing.row_data(i) {
                                msgs.push(m);
                            }
                        }
                        msgs.push(AiChatMessageModel {
                            role: "model".into(),
                            content: reply.into(),
                            timestamp: chrono::Local::now().format("%H:%M").to_string().into(),
                        });
                        app.set_ai_messages(ModelRc::new(VecModel::from(msgs)));
                        app.set_ai_is_loading(false);
                    }
                });
            });
        }
    });

    // Setup Open AI Settings callback
    let app_weak = app.as_weak();
    app.on_request_open_ai_settings(move || {
        if let Some(app) = app_weak.upgrade() {
            let ai_cfg = ai::load_config();
            app.set_ai_settings_ollama_host(ai_cfg.ollama_host.into());
            app.set_ai_settings_ollama_model(ai_cfg.ollama_model.into());
            app.set_ai_settings_lmstudio_host(ai_cfg.lmstudio_host.into());
            app.set_ai_settings_lmstudio_model(ai_cfg.lmstudio_model.into());
            app.set_ai_settings_lmstudio_key(ai_cfg.lmstudio_api_key.unwrap_or_default().into());
            app.set_ai_settings_gemini_key(ai_cfg.gemini_api_key.unwrap_or_default().into());
            app.set_ai_settings_gemini_model(
                ai_cfg
                    .gemini_model
                    .unwrap_or_else(|| "gemini-3.6-flash".to_string())
                    .into(),
            );
            app.set_ai_settings_grok_key(ai_cfg.grok_api_key.unwrap_or_default().into());
            app.set_ai_settings_grok_model(
                ai_cfg
                    .grok_model
                    .unwrap_or_else(|| "grok-4.5".to_string())
                    .into(),
            );
            let mut ui_st = app.get_ui_state();
            ui_st.active_modal = "ai_settings".into();
            app.set_ui_state(ui_st);
        }
    });

    // Setup Save AI Settings callback
    let app_weak = app.as_weak();
    app.on_request_save_ai_settings(
        move |provider,
              host,
              ollama_mod,
              lm_host,
              lm_mod,
              lm_key,
              gemini_key,
              gemini_mod,
              grok_key,
              grok_mod| {
            let mut ai_cfg = ai::load_config();
            let prov_str = provider.to_string();
            ai_cfg.provider = Some(prov_str.clone());
            ai_cfg.ollama_host = host.to_string();
            ai_cfg.ollama_model = ollama_mod.to_string();
            ai_cfg.lmstudio_host = lm_host.to_string();
            ai_cfg.lmstudio_model = lm_mod.to_string();
            ai_cfg.lmstudio_api_key = if lm_key.trim().is_empty() {
                None
            } else {
                Some(lm_key.trim().to_string())
            };
            ai_cfg.gemini_api_key = if gemini_key.trim().is_empty() {
                None
            } else {
                Some(gemini_key.trim().to_string())
            };
            ai_cfg.gemini_model = if gemini_mod.trim().is_empty() {
                Some("gemini-3.6-flash".to_string())
            } else {
                Some(gemini_mod.trim().to_string())
            };
            ai_cfg.grok_api_key = if grok_key.trim().is_empty() {
                None
            } else {
                Some(grok_key.trim().to_string())
            };
            ai_cfg.grok_model = if grok_mod.trim().is_empty() {
                Some("grok-4.5".to_string())
            } else {
                Some(grok_mod.trim().to_string())
            };
            let _ = ai::save_config(&ai_cfg);

            if let Some(app) = app_weak.upgrade() {
                app.set_ai_selected_provider(provider);
                let current_model = match prov_str.as_str() {
                    "gemini" => ai_cfg
                        .gemini_model
                        .unwrap_or_else(|| "gemini-3.6-flash".to_string()),
                    "grok" => ai_cfg.grok_model.unwrap_or_else(|| "grok-4.5".to_string()),
                    "lmstudio" => {
                        if ai_cfg.lmstudio_model.is_empty() {
                            "local-model".to_string()
                        } else {
                            ai_cfg.lmstudio_model
                        }
                    }
                    _ => ai_cfg.ollama_model,
                };
                app.set_ai_selected_model(current_model.into());
                app.set_status_text("AI settings saved successfully".into());
            }
        },
    );

    // Setup Open URL callback (for releases and documentation)
    app.on_request_open_url(move |url| {
        let u_str = url.to_string();
        let _ = open::that(&u_str);
    });

    // Setup Check for Updates callback
    let app_weak = app.as_weak();
    let rt_handle = rt.handle().clone();
    app.on_request_check_for_updates(move || {
        let app_weak = app_weak.clone();
        if let Some(app) = app_weak.upgrade() {
            app.set_about_is_checking_update(true);
            app.set_about_update_status("Checking GitHub for releases...".into());
            app.set_about_update_available(false);

            rt_handle.spawn(async move {
                let current_version = env!("CARGO_PKG_VERSION");
                let client = reqwest::Client::builder()
                    .user_agent("SpurX-Secure-SSH")
                    .timeout(std::time::Duration::from_secs(8))
                    .build();

                let mut latest_tag: Option<String> = None;
                let mut release_url = "https://github.com/jspurrier/SpurXSecureSSH_rust_slint/releases/latest".to_string();

                if let Ok(client) = client {
                    let url = "https://api.github.com/repos/jspurrier/SpurXSecureSSH_rust_slint/releases/latest";
                    if let Ok(resp) = client.get(url).send().await {
                        if resp.status().is_success() {
                            if let Ok(json) = resp.json::<serde_json::Value>().await {
                                if let Some(tag) = json.get("tag_name").and_then(|t| t.as_str()) {
                                    latest_tag = Some(tag.to_string());
                                    if let Some(html_url) = json.get("html_url").and_then(|u| u.as_str()) {
                                        release_url = html_url.to_string();
                                    }
                                }
                            }
                        }
                    }
                }

                let parse_parts = |s: &str| -> Vec<u64> {
                    s.trim_start_matches('v')
                        .split('.')
                        .map(|p| p.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse::<u64>().unwrap_or(0))
                        .collect()
                };

                let (status_text, is_available) = match latest_tag {
                    Some(tag) => {
                        let curr_parts = parse_parts(current_version);
                        let latest_parts = parse_parts(&tag);
                        if latest_parts > curr_parts {
                            (format!("New update available: {} (Current: v{})", tag, current_version), true)
                        } else {
                            (format!("You are running the latest version (v{})!", current_version), false)
                        }
                    }
                    None => {
                        (format!("Up to date! Running v{} (no newer GitHub release found).", current_version), false)
                    }
                };

                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = app_weak.upgrade() {
                        app.set_about_is_checking_update(false);
                        app.set_about_update_status(status_text.into());
                        app.set_about_update_available(is_available);
                        app.set_about_release_url(release_url.into());
                    }
                });
            });
        }
    });

    // Helper to find current active session id
    fn get_active_session_id(app: &AppWindow) -> Option<String> {
        let tabs = app.get_tabs();
        for i in 0..tabs.row_count() {
            if let Some(t) = tabs.row_data(i) {
                if t.active && t.connected && !t.id.is_empty() {
                    return Some(t.id.to_string());
                }
            }
        }
        None
    }

    // Setup YModem Send callback
    let app_weak = app.as_weak();
    let state_ymodem = app_state.clone();
    let rt_handle = rt.handle().clone();
    app.on_request_ymodem_send(move || {
        let app_weak = app_weak.clone();
        if let Some(app) = app_weak.upgrade() {
            if let Some(sid) = get_active_session_id(&app) {
                if let Some(file_path) = rfd::FileDialog::new().pick_file() {
                    let file_str = file_path.to_string_lossy().to_string();
                    let fname = file_path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    let fsize = std::fs::metadata(&file_path).map(|m| m.len()).unwrap_or(0);

                    app.set_ymodem(YModemInfo {
                        active: true,
                        filename: fname.clone().into(),
                        state: "Starting".into(),
                        bytes_sent: 0.0,
                        total_bytes: fsize as f32,
                        percentage: 0.0,
                        speed: "0 KB/s".into(),
                        elapsed: "0s".into(),
                    });

                    let senders = state_ymodem.input_senders.read();
                    if let Some(tx) = senders.get(&sid) {
                        let (prog_tx, mut prog_rx) =
                            tokio::sync::mpsc::unbounded_channel::<ymodem::YmodemProgress>();
                        let app_weak_prog = app_weak.clone();

                        rt_handle.spawn(async move {
                            while let Some(prog) = prog_rx.recv().await {
                                let app_weak_inner = app_weak_prog.clone();
                                let pct = if prog.total_bytes > 0 {
                                    (prog.bytes_sent as f32 / prog.total_bytes as f32) * 100.0
                                } else {
                                    0.0
                                };
                                let speed_str = format!("{:.1} KB/s", prog.speed / 1024.0);

                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(app) = app_weak_inner.upgrade() {
                                        app.set_ymodem(YModemInfo {
                                            active: prog.state != "Finished"
                                                && !prog.state.contains("Failed"),
                                            filename: prog.filename.into(),
                                            state: prog.state.into(),
                                            bytes_sent: prog.bytes_sent as f32,
                                            total_bytes: prog.total_bytes as f32,
                                            percentage: pct,
                                            speed: speed_str.into(),
                                            elapsed: "0s".into(),
                                        });
                                    }
                                });
                            }
                        });

                        match ymodem::YmodemSender::new(&file_str, sid.clone(), Some(prog_tx)) {
                            Ok(mut sender) => {
                                if let Some(header) = sender.handle_byte(ymodem::C_CHAR) {
                                    let _ = tx.send(ssh::SshInput::Data(header));
                                }
                            }
                            Err(e) => {
                                app.set_status_text(format!("YModem error: {}", e).into());
                            }
                        }
                    }
                }
            } else {
                app.set_status_text("YModem requires an active connected SSH session".into());
            }
        }
    });

    // Setup YModem Abort callback
    let app_weak = app.as_weak();
    let state_ymodem_abort = app_state.clone();
    app.on_request_ymodem_abort(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_ymodem(YModemInfo {
                active: false,
                filename: "".into(),
                state: "Cancelled".into(),
                bytes_sent: 0.0,
                total_bytes: 0.0,
                percentage: 0.0,
                speed: "".into(),
                elapsed: "".into(),
            });
            if let Some(sid) = get_active_session_id(&app) {
                let senders = state_ymodem_abort.input_senders.read();
                if let Some(tx) = senders.get(&sid) {
                    let _ = tx.send(ssh::SshInput::Data(vec![
                        ymodem::CAN,
                        ymodem::CAN,
                        ymodem::CAN,
                    ]));
                }
            }
            app.set_status_text("YModem transfer cancelled".into());
        }
    });

    // Setup SFTP Local Navigate callback
    let app_weak = app.as_weak();
    app.on_request_sftp_local_navigate(move |target| {
        if let Some(app) = app_weak.upgrade() {
            let current = app.get_sftp_local_path().to_string();
            let new_path = if target == ".." {
                Path::new(&current)
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| "/".to_string())
            } else if target.starts_with('/') {
                target.to_string()
            } else {
                Path::new(&current)
                    .join(target.as_str())
                    .to_string_lossy()
                    .to_string()
            };
            app.set_sftp_local_path(new_path.clone().into());
            refresh_local_sftp_ui(&app, &new_path);
        }
    });

    async fn ensure_sftp_connected(state: &Arc<AppState>, session_id: &str) -> Result<(), String> {
        if state.sftp_manager.is_connected(session_id).await {
            return Ok(());
        }
        let req = {
            let creds = state.session_credentials.read();
            creds.get(session_id).cloned()
        };
        if let Some(r) = req {
            state
                .sftp_manager
                .connect(
                    session_id,
                    &r.host,
                    r.port,
                    &r.username,
                    r.password.as_deref(),
                    &r.auth_method,
                    r.private_key_name.as_deref(),
                    r.private_key_passphrase.as_deref(),
                )
                .await
        } else {
            Err("No stored credentials found for session".to_string())
        }
    }

    // Setup SFTP Remote Navigate callback
    let app_weak = app.as_weak();
    let state_sftp = app_state.clone();
    let rt_handle = rt.handle().clone();
    app.on_request_sftp_remote_navigate(move |target| {
        let app_weak = app_weak.clone();
        let state_sftp = state_sftp.clone();
        if let Some(app) = app_weak.upgrade() {
            if let Some(sid) = get_active_session_id(&app) {
                let current = app.get_sftp_remote_path().to_string();
                let new_path = if target == ".." {
                    Path::new(&current)
                        .parent()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|| "/".to_string())
                } else if target.starts_with('/') {
                    target.to_string()
                } else {
                    format!(
                        "{}/{}",
                        current.trim_end_matches('/'),
                        target.as_str().trim_start_matches('/')
                    )
                };

                let path_to_set = new_path.clone();
                app.set_sftp_remote_path(path_to_set.clone().into());
                app.set_sftp_status("Connecting SFTP & loading remote directory...".into());

                rt_handle.spawn(async move {
                    if let Err(e) = ensure_sftp_connected(&state_sftp, &sid).await {
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = app_weak.upgrade() {
                                app.set_sftp_status(format!("SFTP connect error: {}", e).into());
                            }
                        });
                        return;
                    }
                    match state_sftp.sftp_manager.list_dir(&sid, &new_path).await {
                        Ok(entries) => {
                            let mut files: Vec<SftpFileItemModel> = Vec::new();
                            for e in entries {
                                let size_str = if e.is_dir {
                                    "-".to_string()
                                } else {
                                    format!("{} B", e.size)
                                };
                                files.push(SftpFileItemModel {
                                    name: e.name.into(),
                                    is_dir: e.is_dir,
                                    size: size_str.into(),
                                    modified: "-".into(),
                                });
                            }
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(app) = app_weak.upgrade() {
                                    app.set_sftp_remote_files(ModelRc::new(VecModel::from(files)));
                                    app.set_sftp_status("Ready".into());
                                }
                            });
                        }
                        Err(e) => {
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(app) = app_weak.upgrade() {
                                    app.set_sftp_status(format!("SFTP error: {}", e).into());
                                }
                            });
                        }
                    }
                });
            } else {
                app.set_sftp_status("Not connected to SSH session".into());
            }
        }
    });

    // Setup SFTP Upload callback
    let app_weak = app.as_weak();
    let state_sftp_up = app_state.clone();
    let rt_handle = rt.handle().clone();
    app.on_request_sftp_upload(move |filename, remote_dir| {
        let app_weak = app_weak.clone();
        let state_sftp = state_sftp_up.clone();
        if let Some(app) = app_weak.upgrade() {
            if let Some(sid) = get_active_session_id(&app) {
                let local_dir = app.get_sftp_local_path().to_string();
                let local_path = Path::new(&local_dir)
                    .join(filename.as_str())
                    .to_string_lossy()
                    .to_string();
                let remote_path = format!(
                    "{}/{}",
                    remote_dir.as_str().trim_end_matches('/'),
                    filename.as_str().trim_start_matches('/')
                );
                let r_dir = remote_dir.to_string();

                app.set_sftp_status(format!("Uploading {}...", filename).into());

                rt_handle.spawn(async move {
                    if let Err(e) = ensure_sftp_connected(&state_sftp, &sid).await {
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = app_weak.upgrade() {
                                app.set_sftp_status(format!("SFTP connect error: {}", e).into());
                            }
                        });
                        return;
                    }
                    match state_sftp
                        .sftp_manager
                        .upload(&sid, &local_path, &remote_path)
                        .await
                    {
                        Ok(_) => {
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(app) = app_weak.upgrade() {
                                    app.set_sftp_status("Upload complete".into());
                                    app.invoke_request_sftp_remote_navigate(r_dir.into());
                                }
                            });
                        }
                        Err(e) => {
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(app) = app_weak.upgrade() {
                                    app.set_sftp_status(format!("Upload error: {}", e).into());
                                }
                            });
                        }
                    }
                });
            }
        }
    });

    // Setup SFTP Download callback
    let app_weak = app.as_weak();
    let state_sftp_down = app_state.clone();
    let rt_handle = rt.handle().clone();
    app.on_request_sftp_download(move |filename, local_dir| {
        let app_weak = app_weak.clone();
        let state_sftp = state_sftp_down.clone();
        if let Some(app) = app_weak.upgrade() {
            if let Some(sid) = get_active_session_id(&app) {
                let remote_dir = app.get_sftp_remote_path().to_string();
                let remote_path = format!(
                    "{}/{}",
                    remote_dir.trim_end_matches('/'),
                    filename.as_str().trim_start_matches('/')
                );
                let local_path = Path::new(local_dir.as_str())
                    .join(filename.as_str())
                    .to_string_lossy()
                    .to_string();
                let l_dir = local_dir.to_string();

                app.set_sftp_status(format!("Downloading {}...", filename).into());

                rt_handle.spawn(async move {
                    if let Err(e) = ensure_sftp_connected(&state_sftp, &sid).await {
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = app_weak.upgrade() {
                                app.set_sftp_status(format!("SFTP connect error: {}", e).into());
                            }
                        });
                        return;
                    }
                    match state_sftp
                        .sftp_manager
                        .download(&sid, &remote_path, &local_path)
                        .await
                    {
                        Ok(_) => {
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(app) = app_weak.upgrade() {
                                    app.set_sftp_status("Download complete".into());
                                    refresh_local_sftp_ui(&app, &l_dir);
                                }
                            });
                        }
                        Err(e) => {
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(app) = app_weak.upgrade() {
                                    app.set_sftp_status(format!("Download error: {}", e).into());
                                }
                            });
                        }
                    }
                });
            }
        }
    });

    // Setup SFTP New Folder callback
    let app_weak = app.as_weak();
    let state_sftp_nf = app_state.clone();
    let rt_handle = rt.handle().clone();
    app.on_request_sftp_new_folder(move |folder_name| {
        let app_weak = app_weak.clone();
        let state_sftp = state_sftp_nf.clone();
        if let Some(app) = app_weak.upgrade() {
            if let Some(sid) = get_active_session_id(&app) {
                let remote_dir = app.get_sftp_remote_path().to_string();
                let new_dir_path = format!(
                    "{}/{}",
                    remote_dir.trim_end_matches('/'),
                    folder_name.as_str().trim_start_matches('/')
                );
                let r_dir = remote_dir.clone();

                rt_handle.spawn(async move {
                    if let Ok(_) = ensure_sftp_connected(&state_sftp, &sid).await {
                        let _ = state_sftp.sftp_manager.mkdir(&sid, &new_dir_path).await;
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = app_weak.upgrade() {
                                app.invoke_request_sftp_remote_navigate(r_dir.into());
                            }
                        });
                    }
                });
            }
        }
    });

    // Setup SFTP Delete callback
    let app_weak = app.as_weak();
    let state_sftp_del = app_state.clone();
    let rt_handle = rt.handle().clone();
    app.on_request_sftp_delete(move |path_name, is_dir| {
        let app_weak = app_weak.clone();
        let state_sftp = state_sftp_del.clone();
        if let Some(app) = app_weak.upgrade() {
            if let Some(sid) = get_active_session_id(&app) {
                let remote_dir = app.get_sftp_remote_path().to_string();
                let target_path = format!(
                    "{}/{}",
                    remote_dir.trim_end_matches('/'),
                    path_name.as_str().trim_start_matches('/')
                );
                let r_dir = remote_dir.clone();

                rt_handle.spawn(async move {
                    if let Ok(_) = ensure_sftp_connected(&state_sftp, &sid).await {
                        let _ = if is_dir {
                            state_sftp.sftp_manager.delete_dir(&sid, &target_path).await
                        } else {
                            state_sftp
                                .sftp_manager
                                .delete_file(&sid, &target_path)
                                .await
                        };
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = app_weak.upgrade() {
                                app.invoke_request_sftp_remote_navigate(r_dir.into());
                            }
                        });
                    }
                });
            }
        }
    });

    // Setup Broadcast Command callback
    let state_clone = app_state.clone();
    let app_weak = app.as_weak();
    app.on_request_broadcast(move |cmd, scope| {
        let cmd_str = cmd.to_string();
        if cmd_str.trim().is_empty() {
            return;
        }
        let cmd_bytes = format!("{}\n", cmd_str.trim_end()).into_bytes();
        let senders = state_clone.input_senders.read();
        let scope_str = scope.to_string();

        let mut sent_count = 0;
        if scope_str == "all" {
            for tx in senders.values() {
                let _ = tx.send(ssh::SshInput::Data(cmd_bytes.clone()));
                sent_count += 1;
            }
        } else {
            let groups = state_clone.tab_groups.read();
            if let Some(g) = groups.get(&scope_str) {
                for tab_id in &g.tab_ids {
                    if let Some(tx) = senders.get(tab_id) {
                        let _ = tx.send(ssh::SshInput::Data(cmd_bytes.clone()));
                        sent_count += 1;
                    }
                }
            }
        }
        if let Some(app) = app_weak.upgrade() {
            app.set_status_text(format!("Broadcasted command to {} session(s)", sent_count).into());
        }
    });

    // Setup SSH Keypair Generation callback
    let app_weak = app.as_weak();
    app.on_request_generate_key(move |name, algo, pass, storage_dir| {
        let pass_opt = if pass.is_empty() {
            None
        } else {
            Some(pass.to_string())
        };
        let dir_opt = if storage_dir.is_empty() {
            None
        } else {
            Some(storage_dir.as_str())
        };
        let res = keys::generate_key_pair(&name, &algo, pass_opt, dir_opt);
        if let Some(app) = app_weak.upgrade() {
            match res {
                Ok(k) => {
                    app.set_status_text(
                        format!("Generated SSH key {} at {}", k.name, k.path).into(),
                    );
                    refresh_keys_ui(&app);
                }
                Err(e) => {
                    app.set_status_text(format!("Key generation error: {}", e).into());
                }
            }
        }
    });

    // Setup Delete Key callback
    let app_weak = app.as_weak();
    app.on_request_delete_key(move |path_or_name| {
        let _ = keys::delete_key(&path_or_name);
        if let Some(app) = app_weak.upgrade() {
            app.set_status_text(format!("Deleted SSH key {}", path_or_name).into());
            refresh_keys_ui(&app);
        }
    });

    // Setup Browse Key Storage Directory callback
    let app_weak = app.as_weak();
    app.on_request_browse_key_storage(move || {
        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
            if let Some(app) = app_weak.upgrade() {
                app.set_default_ssh_dir(folder.to_string_lossy().to_string().into());
            }
        }
    });

    // Setup Open Keys Directory callback
    app.on_request_open_keys_dir(move || {
        let dir = keys::get_default_ssh_dir();
        let _ = open::that(dir);
    });

    // Setup Master Password submit callback
    let app_weak = app.as_weak();
    let exp_sess_pwd = expanded_session_folders.clone();
    let filter_for_pwd = sidebar_filter.clone();
    let sess_cache_pwd = session_cache.clone();
    app.on_request_master_password_submit(move |old_pwd, new_pwd| {
        let old_opt = if old_pwd.is_empty() {
            None
        } else {
            Some(old_pwd.as_str())
        };
        let new_opt = if new_pwd.is_empty() {
            None
        } else {
            Some(new_pwd.as_str())
        };
        match session::change_master_password(old_opt, new_opt) {
            Ok(_) => {
                *sess_cache_pwd.write() = session::load_sessions().unwrap_or_default();
                if let Some(app) = app_weak.upgrade() {
                    app.set_master_password_error("".into());
                    app.set_master_password_is_set(
                        session::is_master_password_set().unwrap_or(false),
                    );
                    refresh_sessions_ui(
                        &app,
                        &sess_cache_pwd,
                        &exp_sess_pwd,
                        &filter_for_pwd.lock(),
                    );
                }
            }
            Err(e) => {
                if let Some(app) = app_weak.upgrade() {
                    app.set_master_password_error(e.into());
                }
            }
        }
    });

    // Setup Delete Session callback
    let app_weak = app.as_weak();
    let exp_sess_del = expanded_session_folders.clone();
    let filter_for_del = sidebar_filter.clone();
    let sess_cache_del = session_cache.clone();
    app.on_request_delete_session(move |id| {
        let _ = session::delete_session(&id);
        *sess_cache_del.write() = session::load_sessions().unwrap_or_default();
        if let Some(app) = app_weak.upgrade() {
            refresh_sessions_ui(&app, &sess_cache_del, &exp_sess_del, &filter_for_del.lock());
        }
    });

    // Setup Delete Command callback
    let app_weak = app.as_weak();
    let exp_cmd_del = expanded_command_folders.clone();
    let filter_for_cmddel = sidebar_filter.clone();
    let cmd_cache_del = command_cache.clone();
    app.on_request_delete_command(move |id| {
        let _ = command_storage::delete_command(&id);
        *cmd_cache_del.write() = command_storage::load_commands().unwrap_or_default();
        if let Some(app) = app_weak.upgrade() {
            refresh_commands_ui(
                &app,
                &cmd_cache_del,
                &exp_cmd_del,
                &filter_for_cmddel.lock(),
            );
        }
    });

    // Setup Save Folder callback (New folder, Subfolder, Rename folder)
    let app_weak = app.as_weak();
    let exp_sess_fld = expanded_session_folders.clone();
    let exp_cmd_fld = expanded_command_folders.clone();
    let filter_for_fld = sidebar_filter.clone();
    let sess_cache_fld = session_cache.clone();
    let cmd_cache_fld = command_cache.clone();
    app.on_request_save_folder(move |mode, target_path, name| {
        let mode_str = mode.to_string();
        let target_str = target_path.to_string();
        let name_str = name.trim().to_string();
        if name_str.is_empty() {
            return;
        }

        match mode_str.as_str() {
            "rename" => {
                let parent_dir = if let Some(idx) = target_str.rfind('/') {
                    &target_str[..idx]
                } else {
                    ""
                };
                let new_path = if parent_dir.is_empty() {
                    name_str.clone()
                } else {
                    format!("{}/{}", parent_dir, name_str)
                };

                let _ = session::rename_folder(&target_str, &new_path);
                let _ = command_storage::rename_folder(&target_str, &new_path);

                {
                    let mut exp_s = exp_sess_fld.lock();
                    if exp_s.expanded.remove(&target_str) {
                        exp_s.expanded.insert(new_path.clone());
                    }
                    let mut exp_c = exp_cmd_fld.lock();
                    if exp_c.expanded.remove(&target_str) {
                        exp_c.expanded.insert(new_path.clone());
                    }
                }
            }
            "subfolder" => {
                let full_subpath = if target_str.is_empty() {
                    name_str.clone()
                } else {
                    format!("{}/{}", target_str, name_str)
                };
                exp_sess_fld.lock().expanded.insert(full_subpath.clone());
                exp_sess_fld.lock().expanded.insert(target_str);
                exp_cmd_fld.lock().expanded.insert(full_subpath);
            }
            _ => {
                exp_sess_fld.lock().expanded.insert(name_str.clone());
                exp_cmd_fld.lock().expanded.insert(name_str);
            }
        }

        *sess_cache_fld.write() = session::load_sessions().unwrap_or_default();
        *cmd_cache_fld.write() = command_storage::load_commands().unwrap_or_default();
        if let Some(app) = app_weak.upgrade() {
            let f = filter_for_fld.lock();
            refresh_sessions_ui(&app, &sess_cache_fld, &exp_sess_fld, &f);
            refresh_commands_ui(&app, &cmd_cache_fld, &exp_cmd_fld, &f);
        }
    });

    // Setup Delete Folder callback
    let app_weak = app.as_weak();
    let exp_sess_fld_del = expanded_session_folders.clone();
    let exp_cmd_fld_del = expanded_command_folders.clone();
    let filter_for_fld_del = sidebar_filter.clone();
    let sess_cache_fld_del = session_cache.clone();
    let cmd_cache_fld_del = command_cache.clone();
    app.on_request_delete_folder(move |folder_path| {
        let f_str = folder_path.to_string();
        let _ = session::delete_folder(&f_str);
        let _ = command_storage::delete_folder(&f_str);
        {
            let mut exp_s = exp_sess_fld_del.lock();
            exp_s.expanded.remove(&f_str);
            let mut exp_c = exp_cmd_fld_del.lock();
            exp_c.expanded.remove(&f_str);
        }
        *sess_cache_fld_del.write() = session::load_sessions().unwrap_or_default();
        *cmd_cache_fld_del.write() = command_storage::load_commands().unwrap_or_default();
        if let Some(app) = app_weak.upgrade() {
            let f = filter_for_fld_del.lock();
            refresh_sessions_ui(&app, &sess_cache_fld_del, &exp_sess_fld_del, &f);
            refresh_commands_ui(&app, &cmd_cache_fld_del, &exp_cmd_fld_del, &f);
        }
    });

    // Setup SFTP Local Navigate
    let app_weak = app.as_weak();
    app.on_request_sftp_local_navigate(move |path_str| {
        if let Some(app) = app_weak.upgrade() {
            refresh_local_sftp_ui(&app, &path_str);
        }
    });

    let editing_session_tunnels =
        Arc::new(parking_lot::Mutex::new(Vec::<tunnels::TunnelConfig>::new()));

    // Setup Edit Session (Load Data) callback
    let app_weak = app.as_weak();
    let edit_tun_load = editing_session_tunnels.clone();
    app.on_request_load_session_for_edit(move |id| {
        let id_str = id.to_string();
        if let Ok(sessions) = session::load_sessions() {
            if let Some(s) = sessions.into_iter().find(|item| item.id == id_str) {
                let pwd = session::get_password(&id_str)
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                *edit_tun_load.lock() = s.tunnels.clone();
                let rule_models: Vec<SessionTunnelRuleModel> = s
                    .tunnels
                    .iter()
                    .map(|t| SessionTunnelRuleModel {
                        name: t.name.clone().into(),
                        local_port: t.local_port as i32,
                        remote_host: t.remote_host.clone().into(),
                        remote_port: t.remote_port as i32,
                        auto_start: t.auto_start,
                    })
                    .collect();

                if let Some(app) = app_weak.upgrade() {
                    app.set_edit_session_is_new(false);
                    app.set_edit_session_id(s.id.into());
                    app.set_edit_session_name(s.name.into());
                    app.set_edit_session_host(s.host.into());
                    app.set_edit_session_port(s.port.to_string().into());
                    app.set_edit_session_folder(s.folder.unwrap_or_default().into());
                    app.set_edit_session_use_global(s.use_global_credentials);
                    app.set_edit_session_username(s.username.unwrap_or_default().into());
                    app.set_edit_session_password(pwd.into());
                    app.set_edit_session_auth_method(s.auth_method.into());
                    app.set_edit_session_log_dir(s.log_directory.unwrap_or_default().into());
                    app.set_edit_session_auto_start_tunnels(s.auto_start_tunnels);
                    app.set_edit_session_tunnel_rules(ModelRc::new(VecModel::from(rule_models)));
                    let mut ui = app.get_ui_state();
                    ui.active_modal = "session_edit".into();
                    app.set_ui_state(ui);
                }
            }
        }
    });

    // Setup New Session callback
    let app_weak = app.as_weak();
    let edit_tun_new = editing_session_tunnels.clone();
    app.on_request_new_session(move || {
        edit_tun_new.lock().clear();
        if let Some(app) = app_weak.upgrade() {
            app.set_edit_session_is_new(true);
            app.set_edit_session_id(Uuid::new_v4().to_string().into());
            app.set_edit_session_name("".into());
            app.set_edit_session_host("".into());
            app.set_edit_session_port("22".into());
            app.set_edit_session_folder("".into());
            app.set_edit_session_use_global(false);
            app.set_edit_session_username("".into());
            app.set_edit_session_password("".into());
            app.set_edit_session_auth_method("password".into());
            app.set_edit_session_log_dir("".into());
            app.set_edit_session_auto_start_tunnels(false);
            app.set_edit_session_tunnel_rules(ModelRc::new(VecModel::from(Vec::new())));
            let mut ui = app.get_ui_state();
            ui.active_modal = "session_edit".into();
            app.set_ui_state(ui);
        }
    });

    // Setup Session Add Tunnel Rule callback
    let app_weak = app.as_weak();
    let edit_tun_add = editing_session_tunnels.clone();
    app.on_request_session_add_tunnel(move |name, local_p, remote_h, remote_p, auto_start| {
        let lp = local_p.trim().parse::<u16>().unwrap_or(8080);
        let rp = remote_p.trim().parse::<u16>().unwrap_or(80);
        let rh = if remote_h.trim().is_empty() {
            "127.0.0.1".to_string()
        } else {
            remote_h.trim().to_string()
        };
        let nm = if name.trim().is_empty() {
            format!("{}:{}", rh, rp)
        } else {
            name.trim().to_string()
        };

        let tun = tunnels::TunnelConfig {
            id: Uuid::new_v4().to_string(),
            name: nm,
            session_id: None,
            tunnel_type: "local".to_string(),
            local_host: "127.0.0.1".to_string(),
            local_port: lp,
            remote_host: rh,
            remote_port: rp,
            auto_start,
        };

        let mut list = edit_tun_add.lock();
        list.push(tun);
        let rule_models: Vec<SessionTunnelRuleModel> = list
            .iter()
            .map(|t| SessionTunnelRuleModel {
                name: t.name.clone().into(),
                local_port: t.local_port as i32,
                remote_host: t.remote_host.clone().into(),
                remote_port: t.remote_port as i32,
                auto_start: t.auto_start,
            })
            .collect();

        if let Some(app) = app_weak.upgrade() {
            app.set_edit_session_tunnel_rules(ModelRc::new(VecModel::from(rule_models)));
        }
    });

    // Setup Session Remove Tunnel Rule callback
    let app_weak = app.as_weak();
    let edit_tun_rm = editing_session_tunnels.clone();
    app.on_request_session_remove_tunnel(move |idx| {
        let mut list = edit_tun_rm.lock();
        let u_idx = idx as usize;
        if u_idx < list.len() {
            list.remove(u_idx);
        }
        let rule_models: Vec<SessionTunnelRuleModel> = list
            .iter()
            .map(|t| SessionTunnelRuleModel {
                name: t.name.clone().into(),
                local_port: t.local_port as i32,
                remote_host: t.remote_host.clone().into(),
                remote_port: t.remote_port as i32,
                auto_start: t.auto_start,
            })
            .collect();

        if let Some(app) = app_weak.upgrade() {
            app.set_edit_session_tunnel_rules(ModelRc::new(VecModel::from(rule_models)));
        }
    });

    // Setup Save Session Data callback
    let app_weak = app.as_weak();
    let exp_sess_save = expanded_session_folders.clone();
    let filter_for_save = sidebar_filter.clone();
    let edit_tun_save = editing_session_tunnels.clone();
    let sess_cache_save = session_cache.clone();
    app.on_request_save_session_data(
        move |id, name, host, port, folder, use_global, user, pwd, auth, log_dir, auto_tunnels| {
            let id_str = id.to_string();
            let port_u16 = port.parse::<u16>().unwrap_or(22);
            let f_opt = if folder.is_empty() {
                None
            } else {
                Some(folder.to_string())
            };
            let u_opt = if user.is_empty() {
                None
            } else {
                Some(user.to_string())
            };
            let log_opt = if log_dir.is_empty() {
                None
            } else {
                Some(log_dir.to_string())
            };

            let mut sess = SavedSession::new(
                name.to_string(),
                host.to_string(),
                port_u16,
                f_opt,
                u_opt,
                None,
            );
            sess.id = id_str.clone();
            sess.use_global_credentials = use_global;
            sess.auth_method = auth.to_string();
            sess.log_directory = log_opt;
            sess.auto_start_tunnels = auto_tunnels;
            sess.tunnels = edit_tun_save.lock().clone();

            let _ = session::save_session(sess);
            if use_global {
                let _ = session::delete_password(&id_str);
            } else if !pwd.is_empty() {
                let _ = session::save_password(&id_str, &pwd);
            } else {
                let _ = session::delete_password(&id_str);
            }

            *sess_cache_save.write() = session::load_sessions().unwrap_or_default();
            if let Some(app) = app_weak.upgrade() {
                app.set_status_text(format!("Saved session {}", name).into());
                refresh_sessions_ui(
                    &app,
                    &sess_cache_save,
                    &exp_sess_save,
                    &filter_for_save.lock(),
                );
            }
        },
    );

    // Setup Edit Command (Load Data) callback
    let app_weak = app.as_weak();
    app.on_request_load_command_for_edit(move |id| {
        let id_str = id.to_string();
        if let Ok(commands) = command_storage::load_commands() {
            if let Some(c) = commands.into_iter().find(|item| item.id == id_str) {
                if let Some(app) = app_weak.upgrade() {
                    app.set_edit_command_is_new(false);
                    app.set_edit_command_id(c.id.into());
                    app.set_edit_command_name(c.name.into());
                    app.set_edit_command_text(c.command.into());
                    app.set_edit_command_folder(c.folder.into());
                    app.set_edit_command_description(c.description.unwrap_or_default().into());
                    let mut ui = app.get_ui_state();
                    ui.active_modal = "command_edit".into();
                    app.set_ui_state(ui);
                }
            }
        }
    });

    // Setup New Command callback
    let app_weak = app.as_weak();
    app.on_request_new_command(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_edit_command_is_new(true);
            app.set_edit_command_id(Uuid::new_v4().to_string().into());
            app.set_edit_command_name("".into());
            app.set_edit_command_text("".into());
            app.set_edit_command_folder("".into());
            app.set_edit_command_description("".into());
            let mut ui = app.get_ui_state();
            ui.active_modal = "command_edit".into();
            app.set_ui_state(ui);
        }
    });

    // Setup Save Command Data callback
    let app_weak = app.as_weak();
    let exp_cmd_save = expanded_command_folders.clone();
    let filter_for_cmd_save = sidebar_filter.clone();
    let cmd_cache_save = command_cache.clone();
    app.on_request_save_command_data(move |id, name, cmd_text, folder, desc| {
        let cmd = QuickCommand {
            id: id.to_string(),
            name: name.to_string(),
            command: cmd_text.to_string(),
            folder: folder.to_string(),
            description: if desc.is_empty() {
                None
            } else {
                Some(desc.to_string())
            },
            icon: Some("cyan".to_string()),
            example: None,
        };
        let _ = command_storage::save_command(cmd);
        *cmd_cache_save.write() = command_storage::load_commands().unwrap_or_default();
        if let Some(app) = app_weak.upgrade() {
            app.set_status_text(format!("Saved command {}", name).into());
            refresh_commands_ui(
                &app,
                &cmd_cache_save,
                &exp_cmd_save,
                &filter_for_cmd_save.lock(),
            );
        }
    });

    // Setup Storage Directory Browse and Open callbacks
    let app_weak = app.as_weak();
    let exp_sess_browse = expanded_session_folders.clone();
    let exp_cmd_browse = expanded_command_folders.clone();
    let filter_browse = sidebar_filter.clone();
    let state_browse = app_state.clone();
    let sess_cache_browse = session_cache.clone();
    let cmd_cache_browse = command_cache.clone();
    app.on_request_browse_storage_dir(move || {
        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
            let default_dir = spurx_secure_ssh::get_default_config_dir();
            let _ = std::fs::create_dir_all(&default_dir);
            let _ = std::fs::write(
                default_dir.join("storage_path.txt"),
                folder.to_string_lossy().to_string(),
            );
            *sess_cache_browse.write() = session::load_sessions().unwrap_or_default();
            *cmd_cache_browse.write() = command_storage::load_commands().unwrap_or_default();
            if let Some(app) = app_weak.upgrade() {
                let cur_dir = spurx_secure_ssh::get_app_config_dir()
                    .to_string_lossy()
                    .to_string();
                app.set_settings_config_dir(cur_dir.into());
                let f = filter_browse.lock();
                refresh_sessions_ui(&app, &sess_cache_browse, &exp_sess_browse, &f);
                refresh_commands_ui(&app, &cmd_cache_browse, &exp_cmd_browse, &f);
                refresh_keys_ui(&app);
                refresh_tunnels_ui(&app, &state_browse);
                app.set_status_text("Storage directory updated successfully".into());
            }
        }
    });

    app.on_request_open_storage_dir(move || {
        let dir = spurx_secure_ssh::get_app_config_dir();
        let _ = open::that(dir);
    });

    // Setup Save Tunnel Rule callback
    let app_weak = app.as_weak();
    let state_save_tun = app_state.clone();
    app.on_request_save_tunnel(
        move |name, local_p, remote_h, remote_p, ssh_h, auto_start| {
            let name_str = name.trim().to_string();
            let local_p_str = local_p.trim().to_string();
            let remote_h_str = remote_h.trim().to_string();
            let remote_p_str = remote_p.trim().to_string();
            let ssh_h_str = ssh_h.trim().to_string();

            let port_u16 = local_p_str.parse::<u16>().unwrap_or(8080);
            let r_port_u16 = remote_p_str.parse::<u16>().unwrap_or(80);
            let remote_host = if remote_h_str.is_empty() {
                "127.0.0.1".to_string()
            } else {
                remote_h_str
            };
            let ssh_target = if ssh_h_str.is_empty() {
                None
            } else {
                Some(ssh_h_str)
            };

            let rule_name = if name_str.is_empty() {
                format!(
                    "{}:{} -> {}:{}",
                    "127.0.0.1", port_u16, remote_host, r_port_u16
                )
            } else {
                name_str
            };

            let tun = tunnels::TunnelConfig {
                id: Uuid::new_v4().to_string(),
                name: rule_name,
                session_id: ssh_target,
                tunnel_type: "local".to_string(),
                local_host: "127.0.0.1".to_string(),
                local_port: port_u16,
                remote_host,
                remote_port: r_port_u16,
                auto_start,
            };
            let _ = tunnels::save_tunnel(tun);
            if let Some(app) = app_weak.upgrade() {
                refresh_tunnels_ui(&app, &state_save_tun);
                app.set_status_text("Saved port forwarding rule".into());
            }
        },
    );

    // Setup Delete Tunnel Rule callback
    let app_weak = app.as_weak();
    let state_del_tun = app_state.clone();
    app.on_request_delete_tunnel(move |id| {
        let id_str = id.to_string();
        let _ = tunnels::delete_tunnel(&id_str);
        tunnels::TUNNEL_MANAGER.stop_tunnel(&id_str);
        if let Some(app) = app_weak.upgrade() {
            refresh_tunnels_ui(&app, &state_del_tun);
        }
    });

    // Setup Start Tunnel callback
    let app_weak = app.as_weak();
    let state_start_tun = app_state.clone();
    let rt_handle = rt.handle().clone();
    app.on_request_start_tunnel(move |id| {
        let id_str = id.to_string();
        let app_weak = app_weak.clone();
        let state = state_start_tun.clone();
        rt_handle.spawn(async move {
            let saved = tunnels::load_tunnels();
            if let Some(cfg) = saved.into_iter().find(|t| t.id == id_str) {
                let req_opt = resolve_tunnel_ssh_request(&cfg, &state);
                let (status_msg, _success) = if let Some(req) = req_opt {
                    let res = tunnels::TUNNEL_MANAGER.start_tunnel(cfg, req).await;
                    match res {
                        Ok(_) => ("Tunnel started successfully".to_string(), true),
                        Err(e) => (format!("Failed to start tunnel: {}", e), false),
                    }
                } else {
                    (
                        "No SSH session found for tunnel. Connect to a session first.".to_string(),
                        false,
                    )
                };

                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = app_weak.upgrade() {
                        app.set_status_text(status_msg.into());
                        refresh_tunnels_ui(&app, &state);
                    }
                });
            }
        });
    });

    // Setup Stop Tunnel callback
    let app_weak = app.as_weak();
    let state_stop_tun = app_state.clone();
    app.on_request_stop_tunnel(move |id| {
        let id_str = id.to_string();
        tunnels::TUNNEL_MANAGER.stop_tunnel(&id_str);
        if let Some(app) = app_weak.upgrade() {
            app.set_status_text("Tunnel stopped".into());
            refresh_tunnels_ui(&app, &state_stop_tun);
        }
    });

    // Setup Start All Tunnels callback
    let app_weak = app.as_weak();
    let state_start_all = app_state.clone();
    let rt_handle = rt.handle().clone();
    app.on_request_start_all_tunnels(move || {
        let app_weak = app_weak.clone();
        let state = state_start_all.clone();
        rt_handle.spawn(async move {
            let saved = tunnels::load_tunnels();
            for cfg in saved {
                if let Some(req) = resolve_tunnel_ssh_request(&cfg, &state) {
                    let _ = tunnels::TUNNEL_MANAGER.start_tunnel(cfg, req).await;
                }
            }

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = app_weak.upgrade() {
                    app.set_status_text("Started all port forwarding tunnels".into());
                    refresh_tunnels_ui(&app, &state);
                }
            });
        });
    });

    // Setup Stop All Tunnels callback
    let app_weak = app.as_weak();
    let state_stop_all = app_state.clone();
    app.on_request_stop_all_tunnels(move || {
        tunnels::TUNNEL_MANAGER.stop_all_tunnels();
        if let Some(app) = app_weak.upgrade() {
            app.set_status_text("Stopped all port forwarding tunnels".into());
            refresh_tunnels_ui(&app, &state_stop_all);
        }
    });

    // Setup Move to Folder Modal callbacks
    let app_weak = app.as_weak();
    let sess_cache_move = session_cache.clone();
    let cmd_cache_move = command_cache.clone();
    app.on_request_open_move_modal(move |item_type, item_id, item_name, current_folder| {
        if let Some(app) = app_weak.upgrade() {
            let folders: Vec<slint::SharedString> = if item_type == "session" {
                let sessions = sess_cache_move.read();
                let mut set = BTreeSet::new();
                for s in sessions.iter() {
                    if let Some(ref f) = s.folder {
                        let trimmed = f.trim();
                        if !trimmed.is_empty() {
                            set.insert(trimmed.to_string());
                        }
                    }
                }
                set.into_iter().map(slint::SharedString::from).collect()
            } else {
                let cmds = cmd_cache_move.read();
                let mut set = BTreeSet::new();
                for c in cmds.iter() {
                    let trimmed = c.folder.trim();
                    if !trimmed.is_empty() {
                        set.insert(trimmed.to_string());
                    }
                }
                set.into_iter().map(slint::SharedString::from).collect()
            };

            app.set_move_modal_item_type(item_type);
            app.set_move_modal_item_id(item_id);
            app.set_move_modal_item_name(item_name);
            app.set_move_modal_current_folder(current_folder);
            app.set_move_modal_existing_folders(ModelRc::new(VecModel::from(folders)));
            let mut ui_st = app.get_ui_state();
            ui_st.active_modal = "move".into();
            app.set_ui_state(ui_st);
        }
    });

    let app_weak = app.as_weak();
    let sess_cache_domove = session_cache.clone();
    let cmd_cache_domove = command_cache.clone();
    let exp_sess_domove = expanded_session_folders.clone();
    let exp_cmd_domove = expanded_command_folders.clone();
    let filter_domove = sidebar_filter.clone();
    app.on_request_move_item(move |item_type, item_id, target_folder| {
        let target_str = target_folder.trim().to_string();
        let target_opt = if target_str.is_empty() {
            None
        } else {
            Some(target_str.clone())
        };

        if item_type == "session" {
            let mut sessions = sess_cache_domove.write();
            if let Some(sess) = sessions.iter_mut().find(|s| s.id == item_id.as_str()) {
                sess.folder = target_opt;
                let _ = session::save_session(sess.clone());
            }
            drop(sessions);
            *sess_cache_domove.write() = session::load_sessions().unwrap_or_default();
            if let Some(app) = app_weak.upgrade() {
                refresh_sessions_ui(
                    &app,
                    &sess_cache_domove,
                    &exp_sess_domove,
                    &filter_domove.lock(),
                );
            }
        } else {
            let mut cmds = cmd_cache_domove.write();
            if let Some(cmd) = cmds.iter_mut().find(|c| c.id == item_id.as_str()) {
                cmd.folder = target_str;
                let _ = command_storage::save_command(cmd.clone());
            }
            drop(cmds);
            *cmd_cache_domove.write() = command_storage::load_commands().unwrap_or_default();
            if let Some(app) = app_weak.upgrade() {
                refresh_commands_ui(
                    &app,
                    &cmd_cache_domove,
                    &exp_cmd_domove,
                    &filter_domove.lock(),
                );
            }
        }
    });

    // Setup Known Hosts Callbacks
    let app_weak = app.as_weak();
    app.on_request_delete_known_host(move |host_name| {
        if let Err(e) = known_hosts::delete_known_host(&host_name) {
            eprintln!("Error deleting known host: {}", e);
        }
        if let Some(app) = app_weak.upgrade() {
            refresh_known_hosts_ui(&app);
        }
    });

    let app_weak = app.as_weak();
    app.on_request_clear_known_hosts(move || {
        if let Err(e) = known_hosts::clear_all_known_hosts() {
            eprintln!("Error clearing known hosts: {}", e);
        }
        if let Some(app) = app_weak.upgrade() {
            refresh_known_hosts_ui(&app);
        }
    });

    let app_weak = app.as_weak();
    app.on_request_refresh_known_hosts(move || {
        if let Some(app) = app_weak.upgrade() {
            refresh_known_hosts_ui(&app);
        }
    });

    // Setup AI Inline Autocomplete Callbacks
    let app_weak = app.as_weak();
    let state_ai_auto = app_state.clone();
    let rt_handle = rt.handle().clone();
    app.on_request_ai_autocomplete(move |tab_idx| {
        let (banner, recent_lines) = {
            let tabs = app_weak.upgrade().map(|a| a.get_tabs()).unwrap_or_default();
            let mut sid = String::new();
            if (tab_idx as usize) < tabs.row_count() {
                if let Some(t) = tabs.row_data(tab_idx as usize) {
                    sid = t.id.to_string();
                }
            }
            let buffers = state_ai_auto.terminal_buffers.read();
            if let Some(buf) = buffers.get(&sid) {
                let lines = buf.lock().get_lines();
                let banner = lines
                    .iter()
                    .take(30)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n");
                let recent = lines
                    .iter()
                    .rev()
                    .take(30)
                    .cloned()
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>()
                    .join("\n");
                (banner, recent)
            } else {
                (String::new(), String::new())
            }
        };

        if let Some(app) = app_weak.upgrade() {
            // Instant 0ms heuristic detection & tailored platform suggestions
            let full_text = format!("{}\n{}", banner, recent_lines);
            let (instant_detected, typed_cmd, instant_suggestions) =
                ai::detect_platform_and_suggestions(&full_text);
            let initial_models: Vec<AiAutocompleteItemModel> = instant_suggestions
                .iter()
                .map(|s| AiAutocompleteItemModel {
                    cmd: s.cmd.clone().into(),
                    desc: s.desc.clone().into(),
                })
                .collect();

            let header_context = if !typed_cmd.is_empty() {
                format!("{} • Subcommands for \"{}\"", instant_detected, typed_cmd)
            } else {
                format!("{} • Available Commands", instant_detected)
            };

            app.set_ai_autocomplete_visible(true);
            app.set_ai_autocomplete_context(header_context.into());
            app.set_ai_autocomplete_suggestions(ModelRc::new(VecModel::from(initial_models)));
            app.set_ai_autocomplete_loading(true);

            let prov = app.get_ai_selected_provider().to_string();
            let app_weak_res = app_weak.clone();

            rt_handle.spawn(async move {
                let (detected, suggestions) =
                    ai::get_inline_autocomplete(&banner, &recent_lines, &prov)
                        .await
                        .unwrap_or_else(|_| (instant_detected, instant_suggestions));

                let header_final = if !typed_cmd.is_empty() {
                    format!("{} • Subcommands for \"{}\"", detected, typed_cmd)
                } else {
                    format!("{} • Available Commands", detected)
                };

                let models: Vec<AiAutocompleteItemModel> = suggestions
                    .into_iter()
                    .map(|s| AiAutocompleteItemModel {
                        cmd: s.cmd.into(),
                        desc: s.desc.into(),
                    })
                    .collect();

                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = app_weak_res.upgrade() {
                        app.set_ai_autocomplete_context(header_final.into());
                        app.set_ai_autocomplete_suggestions(ModelRc::new(VecModel::from(models)));
                        app.set_ai_autocomplete_loading(false);
                    }
                });
            });
        }
    });

    let app_weak = app.as_weak();
    let state_accept_ai = app_state.clone();
    app.on_request_accept_ai_suggestion(move |cmd_text| {
        let cmd_str = cmd_text.to_string();
        if let Some(app) = app_weak.upgrade() {
            app.set_ai_autocomplete_visible(false);
            let platform_ctx = app.get_ai_autocomplete_context().to_string();
            let platform = platform_ctx.split('•').next().unwrap_or("").trim();
            if !platform.is_empty() {
                ai::save_learned_command(platform, &cmd_str, "Frequently used command");
            }
            if let Some(sid) = get_active_session_id(&app) {
                let senders = state_accept_ai.input_senders.read();
                if let Some(tx) = senders.get(&sid) {
                    let _ = tx.send(ssh::SshInput::Data(format!("{}\n", cmd_str).into_bytes()));
                }
            }
        }
    });

    let app_weak = app.as_weak();
    app.on_request_dismiss_ai_autocomplete(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_ai_autocomplete_visible(false);
        }
    });

    // Run Slint Event Loop
    app.run()?;

    Ok(())
}

fn translate_slint_key(text_str: &str, is_ctrl: bool, is_alt: bool) -> Vec<u8> {
    use slint::platform::Key;

    if text_str.is_empty() {
        return Vec::new();
    }

    let first_ch = match text_str.chars().next() {
        Some(c) => c,
        None => return Vec::new(),
    };

    // 1. Ignore raw standalone modifier keys (Shift, Ctrl, Alt, Meta, CapsLock, etc.)
    if first_ch == char::from(Key::Shift)
        || first_ch == char::from(Key::ShiftR)
        || first_ch == char::from(Key::Control)
        || first_ch == char::from(Key::ControlR)
        || first_ch == char::from(Key::Alt)
        || first_ch == char::from(Key::AltGr)
        || first_ch == char::from(Key::Meta)
        || first_ch == char::from(Key::MetaR)
        || first_ch == char::from(Key::CapsLock)
        || first_ch == char::from(Key::ScrollLock)
    {
        return Vec::new();
    }

    // 2. Control Key Combinations (Ctrl+C => 0x03, Ctrl+D => 0x04, Ctrl+Z => 0x1a, etc.)
    if is_ctrl {
        let lower = first_ch.to_ascii_lowercase();
        if lower >= 'a' && lower <= 'z' {
            let code = (lower as u8) - b'a' + 1;
            return vec![code];
        } else if lower == '@' || lower == ' ' || lower == '2' {
            return vec![0x00]; // NUL
        } else if lower == '[' || lower == '3' {
            return vec![0x1b]; // ESC
        } else if lower == '\\' || lower == '4' {
            return vec![0x1c]; // FS
        } else if lower == ']' || lower == '5' {
            return vec![0x1d]; // GS
        } else if lower == '^' || lower == '6' {
            return vec![0x1e]; // RS
        } else if lower == '_' || lower == '7' || lower == '/' {
            return vec![0x1f]; // US
        } else if lower == '8' || lower == '?' {
            return vec![0x7f]; // DEL
        }
    }

    // 3. Special Keys & Navigation (VT100 / Xterm standard escape sequences)
    if first_ch == char::from(Key::UpArrow) {
        return b"\x1b[A".to_vec();
    }
    if first_ch == char::from(Key::DownArrow) {
        return b"\x1b[B".to_vec();
    }
    if first_ch == char::from(Key::RightArrow) {
        return b"\x1b[C".to_vec();
    }
    if first_ch == char::from(Key::LeftArrow) {
        return b"\x1b[D".to_vec();
    }
    if first_ch == char::from(Key::Home) {
        return b"\x1b[H".to_vec();
    }
    if first_ch == char::from(Key::End) {
        return b"\x1b[F".to_vec();
    }
    if first_ch == char::from(Key::PageUp) {
        return b"\x1b[5~".to_vec();
    }
    if first_ch == char::from(Key::PageDown) {
        return b"\x1b[6~".to_vec();
    }
    if first_ch == char::from(Key::Insert) {
        return b"\x1b[2~".to_vec();
    }
    if first_ch == char::from(Key::Delete) {
        return vec![0x04];
    }
    if first_ch == char::from(Key::Backspace) || first_ch == '\x08' || first_ch == '\x7f' {
        return vec![0x7f];
    }
    if first_ch == char::from(Key::Return) || first_ch == '\n' || first_ch == '\r' {
        return b"\r".to_vec();
    }
    if first_ch == char::from(Key::Tab) || first_ch == '\t' {
        return b"\t".to_vec();
    }
    if first_ch == char::from(Key::Escape) || first_ch == '\x1b' {
        return b"\x1b".to_vec();
    }
    if first_ch == char::from(Key::F1) {
        return b"\x1bOP".to_vec();
    }
    if first_ch == char::from(Key::F2) {
        return b"\x1bOQ".to_vec();
    }
    if first_ch == char::from(Key::F3) {
        return b"\x1bOR".to_vec();
    }
    if first_ch == char::from(Key::F4) {
        return b"\x1bOS".to_vec();
    }
    if first_ch == char::from(Key::F5) {
        return b"\x1b[15~".to_vec();
    }
    if first_ch == char::from(Key::F6) {
        return b"\x1b[17~".to_vec();
    }
    if first_ch == char::from(Key::F7) {
        return b"\x1b[18~".to_vec();
    }
    if first_ch == char::from(Key::F8) {
        return b"\x1b[19~".to_vec();
    }
    if first_ch == char::from(Key::F9) {
        return b"\x1b[20~".to_vec();
    }
    if first_ch == char::from(Key::F10) {
        return b"\x1b[21~".to_vec();
    }
    if first_ch == char::from(Key::F11) {
        return b"\x1b[23~".to_vec();
    }
    if first_ch == char::from(Key::F12) {
        return b"\x1b[24~".to_vec();
    }

    // 4. Alt Key prefix (ESC prefix for meta sequences)
    if is_alt {
        if first_ch.is_ascii_graphic() || first_ch == ' ' {
            let mut res = vec![0x1b];
            res.extend_from_slice(text_str.as_bytes());
            return res;
        }
        return Vec::new();
    }

    // 5. Standard Unicode text input (including shifted symbols like "@", "!", "$", uppercase letters, etc.)
    text_str.as_bytes().to_vec()
}

fn safe_byte_slice(s: &str, mut start: usize, mut end: usize) -> &str {
    if start > end {
        std::mem::swap(&mut start, &mut end);
    }
    start = start.min(s.len());
    end = end.min(s.len());
    while !s.is_char_boundary(start) && start > 0 {
        start -= 1;
    }
    while !s.is_char_boundary(end) && end < s.len() {
        end += 1;
    }
    if start <= end && end <= s.len() {
        &s[start..end]
    } else {
        ""
    }
}

fn sync_remote_cursor(
    tx: &tokio::sync::mpsc::UnboundedSender<ssh::SshInput>,
    buffer: &Arc<parking_lot::Mutex<TerminalBuffer>>,
    target_offset: usize,
) {
    let mut buf = buffer.lock();
    if buf.scroll_offset == 0 {
        let active_row = buf.cursor_row.min(buf.screen.len().saturating_sub(1));
        let mut line_start_in_visible = 0;
        for i in 0..active_row {
            line_start_in_visible += buf.screen[i].chars().count() + 1; // +1 for '\n'
        }

        let line_len = buf
            .screen
            .get(active_row)
            .map(|l| l.chars().count())
            .unwrap_or(0);
        if target_offset >= line_start_in_visible {
            let clicked_col = target_offset - line_start_in_visible;
            let line_str = buf.screen.get(active_row).map(|s| s.as_str()).unwrap_or("");
            let prompt_col = if let Some(pos) = line_str.rfind('#') {
                pos + 1
            } else if let Some(pos) = line_str.rfind('$') {
                pos + 1
            } else if let Some(pos) = line_str.rfind('>') {
                pos + 1
            } else {
                0
            };
            let target_col = clicked_col.min(line_len).max(prompt_col);
            let current_col = buf.cursor_col;

            if target_col < current_col {
                let count = current_col - target_col;
                let arrows = "\x1b[D".repeat(count);
                let _ = tx.send(ssh::SshInput::Data(arrows.into_bytes()));
                buf.cursor_col = target_col;
            } else if target_col > current_col {
                let count = target_col - current_col;
                let arrows = "\x1b[C".repeat(count);
                let _ = tx.send(ssh::SshInput::Data(arrows.into_bytes()));
                buf.cursor_col = target_col;
            }
        }
    }
}

fn encounter_empty() -> Vec<slint::SharedString> {
    vec![
        slint::SharedString::from("SpurX Secure SSH Terminal Ready."),
        slint::SharedString::from(
            "Quick Connect: click the lightning bolt ⚡ on the toolbar or enter host above.",
        ),
        slint::SharedString::from("Saved Sessions: click any session in the sidebar to connect."),
    ]
}

fn setup_initial_tab(app: &AppWindow) {
    let tab = make_tab_model(
        &format!("tab-{}", Uuid::new_v4()),
        "Terminal 1",
        false,
        true,
        "",
        None,
    );
    app.set_tabs(ModelRc::new(VecModel::from(vec![tab])));
    app.set_active_tab_index(0);
    let empty_lines = encounter_empty();
    let empty_text = empty_lines
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let len = empty_text.len() as i32;
    app.set_terminal_lines(ModelRc::new(VecModel::from(empty_lines)));
    app.set_terminal_full_text(empty_text.into());
    app.invoke_set_terminal_cursor_pos(len);
}

fn refresh_sessions_ui(
    app: &AppWindow,
    session_cache: &Arc<parking_lot::RwLock<Vec<SavedSession>>>,
    expanded_folders: &Arc<parking_lot::Mutex<FolderExpansionState>>,
    filter_query: &str,
) {
    let q = filter_query.trim().to_lowercase();
    let sess_guard = session_cache.read();
    let exp = expanded_folders.lock();

    if q.is_empty() {
        // Populate available session folders for folder pickers only when not filtering
        let mut folder_set = BTreeSet::new();
        for f in exp.expanded.iter() {
            let trimmed = f.trim();
            if !trimmed.is_empty() {
                folder_set.insert(trimmed.to_string());
            }
        }
        for s in sess_guard.iter() {
            if let Some(ref f) = s.folder {
                let trimmed = f.trim();
                if !trimmed.is_empty() {
                    let parts: Vec<&str> = trimmed.split('/').collect();
                    let mut cur = String::new();
                    for p in parts {
                        if !cur.is_empty() {
                            cur.push('/');
                        }
                        cur.push_str(p);
                        folder_set.insert(cur.clone());
                    }
                }
            }
        }
        let folder_models: Vec<slint::SharedString> =
            folder_set.into_iter().map(|f| f.into()).collect();
        app.set_available_session_folders(ModelRc::new(VecModel::from(folder_models)));

        let mut root_node = SessionFolderNode::default();
        for s in sess_guard.iter() {
            let folder_str = s.folder.clone().unwrap_or_default();
            let parts: Vec<&str> = folder_str
                .split('/')
                .map(|p| p.trim())
                .filter(|p| !p.is_empty())
                .collect();
            root_node.insert_session(&parts, "", s.clone());
        }

        let mut models: Vec<SessionItemModel> = Vec::new();
        root_node.flatten_to_models(&exp.expanded, None, 0, &mut models);
        app.set_sessions(ModelRc::new(VecModel::from(models)));
    } else {
        // Hierarchical search path - preserves folder structure, starts expanded, but allows collapsing
        let mut root_node = SessionFolderNode::default();
        for s in sess_guard.iter() {
            let name_match = s.name.to_lowercase().contains(&q);
            let host_match = s.host.to_lowercase().contains(&q);
            let user_match = s
                .username
                .as_deref()
                .unwrap_or("")
                .to_lowercase()
                .contains(&q);
            let folder_match = s
                .folder
                .as_deref()
                .unwrap_or("")
                .to_lowercase()
                .contains(&q);

            if name_match || host_match || user_match || folder_match {
                let folder_str = s.folder.clone().unwrap_or_default();
                let parts: Vec<&str> = folder_str
                    .split('/')
                    .map(|p| p.trim())
                    .filter(|p| !p.is_empty())
                    .collect();
                root_node.insert_session(&parts, "", s.clone());
            }
        }
        let mut models: Vec<SessionItemModel> = Vec::new();
        root_node.flatten_to_models(&exp.expanded, Some(&exp.search_collapsed), 0, &mut models);
        app.set_sessions(ModelRc::new(VecModel::from(models)));
    }
}

fn refresh_commands_ui(
    app: &AppWindow,
    command_cache: &Arc<parking_lot::RwLock<Vec<QuickCommand>>>,
    expanded_folders: &Arc<parking_lot::Mutex<FolderExpansionState>>,
    filter_query: &str,
) {
    let q = filter_query.trim().to_lowercase();
    let cmd_guard = command_cache.read();
    let exp = expanded_folders.lock();

    if q.is_empty() {
        // Populate available command folders for folder pickers only when not filtering
        let mut folder_set = BTreeSet::new();
        for f in exp.expanded.iter() {
            let trimmed = f.trim();
            if !trimmed.is_empty() {
                folder_set.insert(trimmed.to_string());
            }
        }
        for c in cmd_guard.iter() {
            let trimmed = c.folder.trim();
            if !trimmed.is_empty() {
                let parts: Vec<&str> = trimmed.split('/').collect();
                let mut cur = String::new();
                for p in parts {
                    if !cur.is_empty() {
                        cur.push('/');
                    }
                    cur.push_str(p);
                    folder_set.insert(cur.clone());
                }
            }
        }
        let folder_models: Vec<slint::SharedString> =
            folder_set.into_iter().map(|f| f.into()).collect();
        app.set_available_command_folders(ModelRc::new(VecModel::from(folder_models)));

        let mut root_node = CommandFolderNode::default();
        for c in cmd_guard.iter() {
            let folder_clone = c.folder.clone();
            let parts: Vec<&str> = folder_clone
                .split('/')
                .map(|p| p.trim())
                .filter(|p| !p.is_empty())
                .collect();
            root_node.insert_command(&parts, "", c.clone());
        }

        let mut models: Vec<CommandItemModel> = Vec::new();
        root_node.flatten_to_models(&exp.expanded, None, 0, &mut models);
        app.set_commands(ModelRc::new(VecModel::from(models)));
    } else {
        // Hierarchical search path - preserves folder structure, starts expanded, but allows collapsing
        let mut root_node = CommandFolderNode::default();
        for c in cmd_guard.iter() {
            let name_match = c.name.to_lowercase().contains(&q);
            let cmd_match = c.command.to_lowercase().contains(&q);
            let desc_match = c
                .description
                .as_deref()
                .unwrap_or("")
                .to_lowercase()
                .contains(&q);
            let folder_match = c.folder.to_lowercase().contains(&q);

            if name_match || cmd_match || desc_match || folder_match {
                let folder_clone = c.folder.clone();
                let parts: Vec<&str> = folder_clone
                    .split('/')
                    .map(|p| p.trim())
                    .filter(|p| !p.is_empty())
                    .collect();
                root_node.insert_command(&parts, "", c.clone());
            }
        }
        let mut models: Vec<CommandItemModel> = Vec::new();
        root_node.flatten_to_models(&exp.expanded, Some(&exp.search_collapsed), 0, &mut models);
        app.set_commands(ModelRc::new(VecModel::from(models)));
    }
}

fn refresh_keys_ui(app: &AppWindow) {
    app.set_default_ssh_dir(keys::get_default_os_ssh_path().into());
    if let Ok(keys_list) = keys::list_keys() {
        let models: Vec<SshKeyModel> = keys_list
            .into_iter()
            .map(|k| SshKeyModel {
                name: k.name.into(),
                key_type: k.algorithm.into(),
                fingerprint: k.fingerprint.into(),
                public_key: k.public_key.into(),
                path: k.path.into(),
            })
            .collect();
        app.set_ssh_keys(ModelRc::new(VecModel::from(models)));
    }
}

fn refresh_known_hosts_ui(app: &AppWindow) {
    if let Ok(entries) = known_hosts::get_known_hosts() {
        let models: Vec<KnownHostItemModel> = entries
            .into_iter()
            .map(|e| KnownHostItemModel {
                name: e.name.into(),
                key_type: e.key_type.into(),
                fingerprint: e.fingerprint.into(),
            })
            .collect();
        app.set_known_hosts_list(ModelRc::new(VecModel::from(models)));
    }
}

fn refresh_tunnels_ui(app: &AppWindow, _state: &Arc<AppState>) {
    let saved = tunnels::load_tunnels();
    let running = tunnels::TUNNEL_MANAGER.get_statuses();

    let mut models: Vec<TunnelItemModel> = Vec::new();
    for cfg in &saved {
        let status = running.iter().find(|r| r.id == cfg.id);
        let is_running = status.is_some();
        let active = status.map(|s| s.active_connections).unwrap_or(0);
        let rx = status.map(|s| s.bytes_rx).unwrap_or(0);
        let tx = status.map(|s| s.bytes_tx).unwrap_or(0);

        models.push(TunnelItemModel {
            id: cfg.id.clone().into(),
            name: cfg.name.clone().into(),
            tunnel_type: cfg.tunnel_type.clone().into(),
            local_addr: format!("{}:{}", cfg.local_host, cfg.local_port).into(),
            remote_target: format!("{}:{}", cfg.remote_host, cfg.remote_port).into(),
            running: is_running,
            active_conns: active as i32,
            bytes_rx: format!("{} B", rx).into(),
            bytes_tx: format!("{} B", tx).into(),
        });
    }
    app.set_tunnels_list(ModelRc::new(VecModel::from(models)));
}

fn resolve_tunnel_ssh_request(
    cfg: &tunnels::TunnelConfig,
    state: &Arc<AppState>,
) -> Option<ConnectRequest> {
    let app_cfg = settings::load_settings();
    let default_user = app_cfg
        .global_username
        .unwrap_or_else(|| "root".to_string());
    let default_pwd = session::get_password("__global__").ok().flatten();

    // 1. If cfg has a session_id specified, try to find matching saved session
    if let Some(ref sid) = cfg.session_id {
        if let Ok(sessions) = session::load_sessions() {
            if let Some(s) = sessions
                .into_iter()
                .find(|s| &s.id == sid || &s.name == sid || &s.host == sid)
            {
                let user = if !s.use_global_credentials {
                    s.username
                        .clone()
                        .filter(|u| !u.is_empty())
                        .unwrap_or_else(|| default_user.clone())
                } else {
                    default_user.clone()
                };
                let pwd = if !s.use_global_credentials {
                    session::get_password(&s.id).ok().flatten()
                } else {
                    default_pwd.clone()
                };
                return Some(ConnectRequest {
                    host: s.host,
                    port: s.port,
                    username: user,
                    password: pwd,
                    log_directory: None,
                    auth_method: s.auth_method,
                    private_key_name: s.private_key_name,
                    private_key_passphrase: None,
                });
            }
        }
    }

    // 2. If there are active connections in state.connections, use the active connection
    {
        let conns = state.connections.read();
        if let Some(conn) = conns.values().next() {
            let sid = &conn.session_id;
            let mut auth_method = "password".to_string();
            let mut priv_key = None;
            let mut pwd = session::get_password(sid)
                .ok()
                .flatten()
                .or_else(|| default_pwd.clone());

            if let Ok(sessions) = session::load_sessions() {
                if let Some(s) = sessions
                    .into_iter()
                    .find(|s| &s.id == sid || &s.host == &conn.host)
                {
                    auth_method = s.auth_method;
                    priv_key = s.private_key_name;
                    if !s.use_global_credentials {
                        pwd = session::get_password(&s.id).ok().flatten();
                    }
                }
            }

            return Some(ConnectRequest {
                host: conn.host.clone(),
                port: conn.port,
                username: conn.username.clone(),
                password: pwd,
                log_directory: None,
                auth_method,
                private_key_name: priv_key,
                private_key_passphrase: None,
            });
        }
    }

    // 3. Fallback to first saved session if available
    if let Ok(sessions) = session::load_sessions() {
        if let Some(s) = sessions.into_iter().next() {
            let user = if !s.use_global_credentials {
                s.username
                    .clone()
                    .filter(|u| !u.is_empty())
                    .unwrap_or_else(|| default_user.clone())
            } else {
                default_user.clone()
            };
            let pwd = if !s.use_global_credentials {
                session::get_password(&s.id).ok().flatten()
            } else {
                default_pwd.clone()
            };
            return Some(ConnectRequest {
                host: s.host,
                port: s.port,
                username: user,
                password: pwd,
                log_directory: None,
                auth_method: s.auth_method,
                private_key_name: s.private_key_name,
                private_key_passphrase: None,
            });
        }
    }

    None
}

fn refresh_local_sftp_ui(app: &AppWindow, dir_path: &str) {
    let path = Path::new(dir_path);
    if let Ok(entries) = std::fs::read_dir(path) {
        let mut files: Vec<SftpFileItemModel> = Vec::new();
        for entry in entries.flatten() {
            let metadata = entry.metadata().ok();
            let is_dir = metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false);
            let size_str = if is_dir {
                "-".to_string()
            } else {
                format!("{} B", metadata.as_ref().map(|m| m.len()).unwrap_or(0))
            };
            files.push(SftpFileItemModel {
                name: entry.file_name().to_string_lossy().to_string().into(),
                is_dir,
                size: size_str.into(),
                modified: "-".into(),
            });
        }
        app.set_sftp_local_files(ModelRc::new(VecModel::from(files)));
    }
}

async fn connect_ssh_session(
    app_weak: slint::Weak<AppWindow>,
    state: Arc<AppState>,
    host: String,
    port: u16,
    username: String,
    password: Option<String>,
    auth_method: String,
    private_key_name: Option<String>,
    private_key_passphrase: Option<String>,
    log_directory: Option<String>,
) {
    let session_id = Uuid::new_v4().to_string();
    let (output_tx, mut output_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let (input_tx, mut input_rx) = mpsc::unbounded_channel::<ssh::SshInput>();

    state
        .input_senders
        .write()
        .insert(session_id.clone(), input_tx);
    let conn = Arc::new(ssh::SshConnection::new(
        session_id.clone(),
        host.clone(),
        port,
        username.clone(),
    ));
    state.connections.write().insert(session_id.clone(), conn);

    let init_cols = state
        .current_terminal_cols
        .load(std::sync::atomic::Ordering::Relaxed);
    let init_rows = state
        .current_terminal_rows
        .load(std::sync::atomic::Ordering::Relaxed);
    let mut initial_buffer = TerminalBuffer::new(init_cols, init_rows, 10000);
    let init_msg = format!("Connecting to {}:{} as {}...\r\n", host, port, username);
    initial_buffer.process_bytes(init_msg.as_bytes());

    state.session_credentials.write().insert(
        session_id.clone(),
        ConnectRequest {
            host: host.clone(),
            port,
            username: username.clone(),
            password: password.clone(),
            log_directory: log_directory.clone(),
            auth_method: auth_method.clone(),
            private_key_name: private_key_name.clone(),
            private_key_passphrase: private_key_passphrase.clone(),
        },
    );

    let buffer = Arc::new(parking_lot::Mutex::new(initial_buffer));
    state
        .terminal_buffers
        .write()
        .insert(session_id.clone(), buffer.clone());

    // Update Tab in UI
    let app_weak_tab = app_weak.clone();
    let sid_tab = session_id.clone();
    let host_tab = host.clone();
    let (init_text, init_offset, total, scroll_off, vis_rows) = buffer.lock().get_visible_text();

    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = app_weak_tab.upgrade() {
            let tabs = app.get_tabs();
            let mut new_tabs: Vec<TabModel> = Vec::new();
            for i in 0..tabs.row_count() {
                if let Some(t) = tabs.row_data(i) {
                    new_tabs.push(t);
                }
            }

            // If only 1 initial disconnected placeholder tab exists, replace it
            let target_idx =
                if new_tabs.len() == 1 && !new_tabs[0].connected && new_tabs[0].host.is_empty() {
                    new_tabs[0] = make_tab_model(
                        &sid_tab,
                        &format!("{}:{}", host_tab, port),
                        true,
                        true,
                        &host_tab,
                        None,
                    );
                    0
                } else {
                    new_tabs.push(make_tab_model(
                        &sid_tab,
                        &format!("{}:{}", host_tab, port),
                        true,
                        true,
                        &host_tab,
                        None,
                    ));
                    (new_tabs.len() - 1) as i32
                };

            app.set_tabs(ModelRc::new(VecModel::from(new_tabs)));
            app.set_active_tab_index(target_idx);
            app.set_terminal_scroll_total(total as i32);
            app.set_terminal_scroll_offset(scroll_off as i32);
            app.set_terminal_scroll_visible(vis_rows as i32);
            app.set_terminal_full_text(init_text.into());
            app.invoke_set_terminal_cursor_pos(init_offset as i32);
            app.invoke_focus_terminal();
            app.set_status_text(format!("Connecting to {}:{}...", host_tab, port).into());
        }
    });

    // Spawn async output reader task
    let app_weak_out = app_weak.clone();
    let buf_for_out = buffer.clone();
    let sid_out = session_id.clone();
    tokio::spawn(async move {
        let mut buffer_dirty = false;
        let mut in_burst = false;
        let last_rendered = Arc::new(parking_lot::Mutex::new((
            String::new(),
            -1i32,
            -1i32,
            -1i32,
            -1i32,
            false,
        )));
        let last_rend_clone = last_rendered.clone();

        let render_term =
            move |app_weak: &slint::Weak<AppWindow>,
                  sid: &str,
                  buf: &Arc<parking_lot::Mutex<TerminalBuffer>>| {
                let app_weak = app_weak.clone();
                let sid_chk = sid.to_string();
                let buf_clone = buf.clone();
                let last_rend = last_rend_clone.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = app_weak.upgrade() {
                        let active_idx = app.get_active_tab_index() as usize;
                        let tabs = app.get_tabs();
                        if active_idx < tabs.row_count() {
                            if let Some(t) = tabs.row_data(active_idx) {
                                if t.id == sid_chk {
                                    let needs_init = {
                                        let mut state = last_rend.lock();
                                        if !state.5 {
                                            state.5 = true;
                                            true
                                        } else {
                                            false
                                        }
                                    };

                                    if needs_init {
                                        app.invoke_focus_terminal();
                                    }

                                    let (full_text, offset, total, scroll_off, vis_rows) = {
                                        let b = buf_clone.lock();
                                        b.get_visible_text()
                                    };

                                    let total_i32 = total as i32;
                                    let scroll_off_i32 = scroll_off as i32;
                                    let vis_rows_i32 = vis_rows as i32;
                                    let offset_i32 = offset as i32;

                                    let mut state = last_rend.lock();
                                    if total_i32 != state.1 {
                                        state.1 = total_i32;
                                        app.set_terminal_scroll_total(total_i32);
                                    }
                                    if scroll_off_i32 != state.2 {
                                        state.2 = scroll_off_i32;
                                        app.set_terminal_scroll_offset(scroll_off_i32);
                                    }
                                    if vis_rows_i32 != state.3 {
                                        state.3 = vis_rows_i32;
                                        app.set_terminal_scroll_visible(vis_rows_i32);
                                    }
                                    if full_text != state.0 {
                                        state.0 = full_text.clone();
                                        app.set_terminal_full_text(full_text.into());
                                    }
                                    if offset_i32 != state.4 {
                                        state.4 = offset_i32;
                                        app.invoke_set_terminal_cursor_pos(offset_i32);
                                    }
                                }
                            }
                        }
                    }
                });
            };

        let mut ticker = tokio::time::interval(tokio::time::Duration::from_millis(12));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                maybe_data = output_rx.recv() => {
                    match maybe_data {
                        Some(data) => {
                            let mut all_data = data;
                            // Batch up to 64KB per parse chunk to avoid lock monopolization
                            while all_data.len() < 65536 {
                                if let Ok(more) = output_rx.try_recv() {
                                    all_data.extend_from_slice(&more);
                                } else {
                                    break;
                                }
                            }
                            buf_for_out.lock().process_bytes(&all_data);

                            if !in_burst {
                                in_burst = true;
                                buffer_dirty = false;
                                render_term(&app_weak_out, &sid_out, &buf_for_out);
                            } else {
                                buffer_dirty = true;
                            }
                        }
                        None => break,
                    }
                }
                _ = ticker.tick() => {
                    if buffer_dirty {
                        buffer_dirty = false;
                        render_term(&app_weak_out, &sid_out, &buf_for_out);
                    } else {
                        in_burst = false;
                    }
                }
            }
        }
    });

    // Run SSH connection loop
    let conn_res = ssh::connect_ssh_async(
        &session_id,
        &host,
        port,
        &username,
        password.as_deref(),
        &auth_method,
        private_key_name.as_deref(),
        private_key_passphrase.as_deref(),
        log_directory,
        init_cols as u16,
        init_rows as u16,
        output_tx.clone(),
        &mut input_rx,
        false,
        None,
    )
    .await;

    if let Err(e) = conn_res {
        let app_weak_err = app_weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(app) = app_weak_err.upgrade() {
                app.set_status_text(format!("Disconnected: {}", e).into());
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_folder_collapse_and_expand() {
        let mut root = SessionFolderNode::default();
        let s1 = SavedSession::new(
            "austin-rtr".into(),
            "10.0.0.1".into(),
            22,
            Some("Texas/Austin".into()),
            None,
            None,
        );
        let s2 = SavedSession::new(
            "dallas-rtr".into(),
            "10.0.0.2".into(),
            22,
            Some("Texas/Dallas".into()),
            None,
            None,
        );
        let s3 = SavedSession::new(
            "houston-rtr".into(),
            "10.0.0.3".into(),
            22,
            Some("Texas/Houston".into()),
            None,
            None,
        );

        root.insert_session(&["Texas", "Austin"], "", s1);
        root.insert_session(&["Texas", "Dallas"], "", s2);
        root.insert_session(&["Texas", "Houston"], "", s3);

        let expanded_set = HashSet::new();
        let mut search_collapsed = HashSet::new();

        // 1. Initial search state: search_collapsed is empty -> all matching folders are expanded
        let mut models = Vec::new();
        root.flatten_to_models(&expanded_set, Some(&search_collapsed), 0, &mut models);

        // Models: folder:Texas, folder:Texas/Austin, austin-rtr, folder:Texas/Dallas, dallas-rtr, folder:Texas/Houston, houston-rtr
        assert_eq!(models.len(), 7);
        let texas_model = models.iter().find(|m| m.id == "folder:Texas").unwrap();
        assert!(texas_model.expanded);
        assert_eq!(texas_model.count, 3);

        // 2. User collapses "Texas/Dallas" to focus on other folders
        search_collapsed.insert("Texas/Dallas".into());
        let mut models2 = Vec::new();
        root.flatten_to_models(&expanded_set, Some(&search_collapsed), 0, &mut models2);
        assert_eq!(models2.len(), 6); // dallas-rtr is now hidden
        assert!(models2.iter().any(|m| m.name == "austin-rtr"));
        assert!(!models2.iter().any(|m| m.name == "dallas-rtr"));
        let dallas_folder = models2.iter().find(|m| m.id == "folder:Texas/Dallas").unwrap();
        assert!(!dallas_folder.expanded);

        // 3. User collapses top-level "Texas"
        search_collapsed.insert("Texas".into());
        let mut models3 = Vec::new();
        root.flatten_to_models(&expanded_set, Some(&search_collapsed), 0, &mut models3);
        assert_eq!(models3.len(), 1); // Only folder:Texas is visible
        assert!(!models3[0].expanded);

        // 4. User re-expands "Texas"
        search_collapsed.remove("Texas");
        let mut models4 = Vec::new();
        root.flatten_to_models(&expanded_set, Some(&search_collapsed), 0, &mut models4);
        assert_eq!(models4.len(), 6); // Austin and Houston are visible, Dallas remains collapsed
        assert!(models4.iter().any(|m| m.name == "austin-rtr"));
        assert!(!models4.iter().any(|m| m.name == "dallas-rtr"));

        // 5. Normal mode (search query cleared, search_collapsed = None)
        let mut normal_models = Vec::new();
        root.flatten_to_models(&expanded_set, None, 0, &mut normal_models);
        // With expanded_set empty, only top-level Texas folder is visible and collapsed
        assert_eq!(normal_models.len(), 1);
        assert!(!normal_models[0].expanded);
    }
}
