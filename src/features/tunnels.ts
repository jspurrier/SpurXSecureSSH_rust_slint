import { state } from '../state';
import { TunnelConfig, TunnelStatus, SavedSession, ConnectRequest } from '../types';

/**
 * Format bytes into human readable format (KB, MB, GB)
 */
function formatBytes(bytes: number): string {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
}

/**
 * Generate a new UUID v4
 */
function generateId(): string {
    return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, function (c) {
        const r = (Math.random() * 16) | 0;
        const v = c === 'x' ? r : (r & 0x3) | 0x8;
        return v.toString(16);
    });
}

/**
 * Fetch latest active tunnel statuses from backend
 */
export async function refreshActiveTunnels(): Promise<TunnelStatus[]> {
    try {
        const statuses: TunnelStatus[] = await window.__TAURI__.core.invoke('get_active_tunnels');
        state.activeTunnels = statuses;
        return statuses;
    } catch (e) {
        console.error('Failed to get active tunnels:', e);
        return [];
    }
}

/**
 * Open the SSH Tunnels Manager Modal
 */
export async function openTunnelsModal(filterSessionId?: string) {
    const modal = document.getElementById('modal-tunnels');
    if (!modal) return;

    modal.classList.add('active');

    // Populate session filter dropdown
    const sessionFilter = document.getElementById('tunnels-session-filter') as HTMLSelectElement;
    if (sessionFilter) {
        sessionFilter.innerHTML = '<option value="">All Sessions & Tunnels</option>';
        state.savedSessions.forEach((s) => {
            const opt = document.createElement('option');
            opt.value = s.id;
            opt.textContent = `${s.name} (${s.host}:${s.port})`;
            if (filterSessionId && filterSessionId === s.id) {
                opt.selected = true;
            }
            sessionFilter.appendChild(opt);
        });
    }

    await refreshActiveTunnels();
    renderTunnelsList(filterSessionId);

    // Start polling interval for live metrics
    if (state.tunnelPollInterval) {
        clearInterval(state.tunnelPollInterval);
    }
    state.tunnelPollInterval = setInterval(async () => {
        if (!modal.classList.contains('active')) {
            if (state.tunnelPollInterval) {
                clearInterval(state.tunnelPollInterval);
                state.tunnelPollInterval = null;
            }
            return;
        }
        await refreshActiveTunnels();
        const currentFilter = sessionFilter ? sessionFilter.value : undefined;
        renderTunnelsList(currentFilter || undefined);
    }, 1500);
}

/**
 * Close the SSH Tunnels Manager Modal
 */
export function closeTunnelsModal() {
    const modal = document.getElementById('modal-tunnels');
    if (modal) {
        modal.classList.remove('active');
    }
    if (state.tunnelPollInterval) {
        clearInterval(state.tunnelPollInterval);
        state.tunnelPollInterval = null;
    }
}

/**
 * Render the list of tunnels in the Tunnels Manager Modal
 */
export function renderTunnelsList(filterSessionId?: string) {
    const container = document.getElementById('tunnels-list');
    if (!container) return;

    // Collect all configured tunnels from saved sessions
    const allTunnels: Array<{ tunnel: TunnelConfig; session?: SavedSession }> = [];

    state.savedSessions.forEach((s) => {
        if (filterSessionId && s.id !== filterSessionId) return;
        if (s.tunnels && s.tunnels.length > 0) {
            s.tunnels.forEach((t) => {
                allTunnels.push({ tunnel: t, session: s });
            });
        }
    });

    if (allTunnels.length === 0) {
        container.innerHTML = `
            <div class="tunnels-empty-state">
                <div class="empty-icon">🔀</div>
                <h3>No SSH Tunnels Configured</h3>
                <p>Add port forwarding rules to connect to remote web interfaces (e.g. <code>localhost:8080</code> ➔ <code>192.168.1.50:80</code>) through your SSH servers.</p>
                <button class="btn btn-primary btn-sm" id="btn-empty-add-tunnel">Add New Tunnel</button>
            </div>
        `;
        const addBtn = document.getElementById('btn-empty-add-tunnel');
        if (addBtn) {
            addBtn.addEventListener('click', () => openEditTunnelModal(null, filterSessionId));
        }
        return;
    }

    let html = `
        <table class="tunnels-table">
            <thead>
                <tr>
                    <th>Status</th>
                    <th>Name / Session</th>
                    <th>Local Address</th>
                    <th>Direction</th>
                    <th>Remote Endpoint</th>
                    <th>Traffic</th>
                    <th style="text-align: right;">Actions</th>
                </tr>
            </thead>
            <tbody>
    `;

    allTunnels.forEach(({ tunnel, session }) => {
        const activeStatus = state.activeTunnels.find((st) => st.id === tunnel.id);
        const isRunning = !!activeStatus && activeStatus.running;
        const statusClass = isRunning ? 'status-active' : 'status-stopped';
        const statusText = isRunning ? 'Active' : 'Stopped';
        const sessionName = session ? session.name : 'Standalone';

        const rx = activeStatus ? formatBytes(activeStatus.bytes_rx) : '0 B';
        const tx = activeStatus ? formatBytes(activeStatus.bytes_tx) : '0 B';
        const conns = activeStatus ? activeStatus.active_connections : 0;

        html += `
            <tr data-tunnel-id="${tunnel.id}" data-session-id="${session ? session.id : ''}">
                <td>
                    <span class="tunnel-status-badge ${statusClass}">
                        <span class="status-dot"></span> ${statusText}
                    </span>
                    ${isRunning && conns > 0 ? `<span class="tunnel-conn-count" title="${conns} active client connections">${conns} conn</span>` : ''}
                </td>
                <td>
                    <div class="tunnel-name">${escapeHtml(tunnel.name || 'Unnamed Tunnel')}</div>
                    <div class="tunnel-session-sub">${escapeHtml(sessionName)}</div>
                </td>
                <td>
                    <span class="tunnel-port-pill local-port">
                        ${escapeHtml(tunnel.local_host || '127.0.0.1')}:${tunnel.local_port}
                    </span>
                </td>
                <td style="color: var(--accent-primary, #6366f1); font-weight: bold; text-align: center;">
                    ➔
                </td>
                <td>
                    <span class="tunnel-port-pill remote-port">
                        ${escapeHtml(tunnel.remote_host)}:${tunnel.remote_port}
                    </span>
                </td>
                <td>
                    <div class="tunnel-traffic-metrics">
                        <span title="Downloaded">↓ ${rx}</span>
                        <span title="Uploaded">↑ ${tx}</span>
                    </div>
                </td>
                <td style="text-align: right;">
                    <div class="tunnel-row-actions">
                        ${
                            isRunning
                                ? `
                                <button class="btn btn-sm btn-outline-primary btn-open-browser" data-port="${tunnel.local_port}" title="Open http://localhost:${tunnel.local_port} in browser">
                                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="13" height="13">
                                        <circle cx="12" cy="12" r="10"/>
                                        <line x1="2" y1="12" x2="22" y2="12"/>
                                        <path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"/>
                                    </svg>
                                    Browser
                                </button>
                                <button class="btn btn-sm btn-danger btn-stop-tunnel" data-id="${tunnel.id}" title="Stop Tunnel">
                                    Stop
                                </button>
                                `
                                : `
                                <button class="btn btn-sm btn-success btn-start-tunnel" data-id="${tunnel.id}" data-session-id="${session ? session.id : ''}" title="Start Tunnel">
                                    Start
                                </button>
                                `
                        }
                        <button class="btn btn-sm btn-ghost btn-edit-tunnel" data-id="${tunnel.id}" data-session-id="${session ? session.id : ''}" title="Edit Tunnel">
                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="13" height="13">
                                <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/>
                                <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"/>
                            </svg>
                        </button>
                        <button class="btn btn-sm btn-ghost btn-delete-tunnel" data-id="${tunnel.id}" data-session-id="${session ? session.id : ''}" title="Delete Tunnel">
                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="13" height="13">
                                <polyline points="3 6 5 6 21 6"/>
                                <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>
                            </svg>
                        </button>
                    </div>
                </td>
            </tr>
        `;
    });

    html += `
            </tbody>
        </table>
    `;

    container.innerHTML = html;

    // Attach row button event listeners
    container.querySelectorAll('.btn-open-browser').forEach((btn) => {
        btn.addEventListener('click', () => {
            const port = btn.getAttribute('data-port');
            if (port) {
                openTunnelInBrowser(parseInt(port, 10));
            }
        });
    });

    container.querySelectorAll('.btn-start-tunnel').forEach((btn) => {
        btn.addEventListener('click', async () => {
            const tid = btn.getAttribute('data-id');
            const sid = btn.getAttribute('data-session-id');
            if (tid && sid) {
                await startTunnelById(tid, sid);
            }
        });
    });

    container.querySelectorAll('.btn-stop-tunnel').forEach((btn) => {
        btn.addEventListener('click', async () => {
            const tid = btn.getAttribute('data-id');
            if (tid) {
                await stopTunnelById(tid);
            }
        });
    });

    container.querySelectorAll('.btn-edit-tunnel').forEach((btn) => {
        btn.addEventListener('click', () => {
            const tid = btn.getAttribute('data-id');
            const sid = btn.getAttribute('data-session-id');
            if (tid && sid) {
                const session = state.savedSessions.find((s) => s.id === sid);
                const tunnel = session?.tunnels?.find((t) => t.id === tid);
                if (tunnel) {
                    openEditTunnelModal(tunnel, sid);
                }
            }
        });
    });

    container.querySelectorAll('.btn-delete-tunnel').forEach((btn) => {
        btn.addEventListener('click', async () => {
            const tid = btn.getAttribute('data-id');
            const sid = btn.getAttribute('data-session-id');
            if (tid && sid) {
                await deleteTunnel(tid, sid);
            }
        });
    });
}

/**
 * Open a tunnel port in the default browser
 */
export async function openTunnelInBrowser(port: number, path: string = '') {
    const url = `http://localhost:${port}${path.startsWith('/') ? path : path ? '/' + path : ''}`;
    try {
        await window.__TAURI__.core.invoke('open_tunnel_url', { url });
    } catch (e) {
        console.error('Failed to open browser URL:', e);
        window.open(url, '_blank');
    }
}

/**
 * Start an individual tunnel by ID and Session ID
 */
export async function startTunnelById(tunnelId: string, sessionId: string) {
    const session = state.savedSessions.find((s) => s.id === sessionId);
    if (!session) {
        alert('Associated session not found.');
        return;
    }
    const tunnel = session.tunnels?.find((t) => t.id === tunnelId);
    if (!tunnel) {
        alert('Tunnel configuration not found.');
        return;
    }

    try {
        let password = '';
        if (session.use_global_credentials) {
            password = (await window.__TAURI__.core.invoke<string | null>('get_global_password')) || '';
        } else {
            password = (await window.__TAURI__.core.invoke<string | null>('get_session_password', { sessionId: session.id })) || '';
        }

        const connectReq: ConnectRequest = {
            host: session.host,
            port: session.port,
            username: session.username || '',
            password: password || undefined,
            auth_method: session.auth_method || 'password',
            private_key_name: session.private_key_name,
        };

        const tunnelConfig: TunnelConfig = {
            ...tunnel,
            session_id: sessionId,
        };

        await window.__TAURI__.core.invoke('start_tunnel', {
            tunnel: tunnelConfig,
            connection: connectReq,
        });

        await refreshActiveTunnels();
        renderTunnelsList();
    } catch (e: any) {
        alert(`Failed to start tunnel "${tunnel.name}": ${e}`);
        console.error(e);
    }
}

/**
 * Stop an individual tunnel by ID
 */
export async function stopTunnelById(tunnelId: string) {
    try {
        await window.__TAURI__.core.invoke('stop_tunnel', { tunnelId });
        await refreshActiveTunnels();
        renderTunnelsList();
    } catch (e: any) {
        console.error('Failed to stop tunnel:', e);
    }
}

/**
 * Start all tunnels for a given session or all sessions
 */
export async function startAllTunnels(sessionId?: string) {
    if (sessionId) {
        try {
            await window.__TAURI__.core.invoke('start_session_tunnels', {
                sessionId,
                password: state.masterPassword,
            });
        } catch (e: any) {
            alert(`Failed to start session tunnels: ${e}`);
        }
    } else {
        // Start all tunnels for all saved sessions
        for (const session of state.savedSessions) {
            if (session.tunnels && session.tunnels.length > 0) {
                try {
                    await window.__TAURI__.core.invoke('start_session_tunnels', {
                        sessionId: session.id,
                        password: state.masterPassword,
                    });
                } catch (e) {
                    console.error(`Failed to start tunnels for session ${session.name}:`, e);
                }
            }
        }
    }
    await refreshActiveTunnels();
    renderTunnelsList(sessionId);
}

/**
 * Stop all tunnels
 */
export async function stopAllTunnels(sessionId?: string) {
    try {
        if (sessionId) {
            await window.__TAURI__.core.invoke('stop_session_tunnels', { sessionId });
        } else {
            await window.__TAURI__.core.invoke('stop_all_tunnels');
        }
        await refreshActiveTunnels();
        renderTunnelsList(sessionId);
    } catch (e: any) {
        console.error('Failed to stop all tunnels:', e);
    }
}

/**
 * Delete a tunnel configuration from a saved session
 */
export async function deleteTunnel(tunnelId: string, sessionId: string) {
    if (!confirm('Are you sure you want to delete this SSH tunnel configuration?')) return;

    // Stop it if currently running
    await stopTunnelById(tunnelId);

    const session = state.savedSessions.find((s) => s.id === sessionId);
    if (session && session.tunnels) {
        session.tunnels = session.tunnels.filter((t) => t.id !== tunnelId);
        try {
            await window.__TAURI__.core.invoke('save_session', {
                session,
                password: state.masterPassword,
            });
            renderTunnelsList();
        } catch (e: any) {
            alert(`Failed to save updated session: ${e}`);
        }
    }
}

/**
 * Open Modal to Add/Edit a Tunnel Configuration
 */
export function openEditTunnelModal(tunnel: TunnelConfig | null, preselectedSessionId?: string) {
    const modal = document.getElementById('modal-edit-tunnel');
    if (!modal) return;

    modal.classList.add('active');

    const title = document.getElementById('edit-tunnel-title') as HTMLElement;
    const inputId = document.getElementById('tunnel-edit-id') as HTMLInputElement;
    const inputName = document.getElementById('tunnel-edit-name') as HTMLInputElement;
    const selectSession = document.getElementById('tunnel-edit-session') as HTMLSelectElement;
    const inputLocalHost = document.getElementById('tunnel-edit-localhost') as HTMLInputElement;
    const inputLocalPort = document.getElementById('tunnel-edit-localport') as HTMLInputElement;
    const inputRemoteHost = document.getElementById('tunnel-edit-remotehost') as HTMLInputElement;
    const inputRemotePort = document.getElementById('tunnel-edit-remoteport') as HTMLInputElement;
    const checkAutoStart = document.getElementById('tunnel-edit-autostart') as HTMLInputElement;

    // Populate session select
    if (selectSession) {
        selectSession.innerHTML = '';
        state.savedSessions.forEach((s) => {
            const opt = document.createElement('option');
            opt.value = s.id;
            opt.textContent = `${s.name} (${s.host}:${s.port})`;
            selectSession.appendChild(opt);
        });
    }

    if (tunnel) {
        if (title) title.textContent = 'Edit SSH Tunnel';
        if (inputId) inputId.value = tunnel.id;
        if (inputName) inputName.value = tunnel.name;
        if (selectSession) selectSession.value = tunnel.session_id || preselectedSessionId || '';
        if (inputLocalHost) inputLocalHost.value = tunnel.local_host || '127.0.0.1';
        if (inputLocalPort) inputLocalPort.value = tunnel.local_port.toString();
        if (inputRemoteHost) inputRemoteHost.value = tunnel.remote_host;
        if (inputRemotePort) inputRemotePort.value = tunnel.remote_port.toString();
        if (checkAutoStart) checkAutoStart.checked = !!tunnel.auto_start;
    } else {
        if (title) title.textContent = 'Add SSH Tunnel';
        if (inputId) inputId.value = generateId();
        if (inputName) inputName.value = '';
        if (selectSession && preselectedSessionId) selectSession.value = preselectedSessionId;
        if (inputLocalHost) inputLocalHost.value = '127.0.0.1';
        if (inputLocalPort) inputLocalPort.value = '8080';
        if (inputRemoteHost) inputRemoteHost.value = '127.0.0.1';
        if (inputRemotePort) inputRemotePort.value = '80';
        if (checkAutoStart) checkAutoStart.checked = false;
    }
}

/**
 * Save Tunnel from the Add/Edit Modal
 */
export async function saveTunnelFromModal() {
    const inputId = document.getElementById('tunnel-edit-id') as HTMLInputElement;
    const inputName = document.getElementById('tunnel-edit-name') as HTMLInputElement;
    const selectSession = document.getElementById('tunnel-edit-session') as HTMLSelectElement;
    const inputLocalHost = document.getElementById('tunnel-edit-localhost') as HTMLInputElement;
    const inputLocalPort = document.getElementById('tunnel-edit-localport') as HTMLInputElement;
    const inputRemoteHost = document.getElementById('tunnel-edit-remotehost') as HTMLInputElement;
    const inputRemotePort = document.getElementById('tunnel-edit-remoteport') as HTMLInputElement;
    const checkAutoStart = document.getElementById('tunnel-edit-autostart') as HTMLInputElement;

    const sessionId = selectSession?.value;
    if (!sessionId) {
        alert('Please select a saved session for this tunnel.');
        return;
    }

    const name = inputName.value.trim() || `Port ${inputLocalPort.value} ➔ ${inputRemoteHost.value}:${inputRemotePort.value}`;
    const localPort = parseInt(inputLocalPort.value, 10);
    const remotePort = parseInt(inputRemotePort.value, 10);

    if (isNaN(localPort) || localPort <= 0 || localPort > 65535) {
        alert('Please enter a valid local port (1-65535).');
        return;
    }

    if (isNaN(remotePort) || remotePort <= 0 || remotePort > 65535) {
        alert('Please enter a valid remote port (1-65535).');
        return;
    }

    const tunnelConfig: TunnelConfig = {
        id: inputId.value || generateId(),
        name,
        session_id: sessionId,
        tunnel_type: 'local',
        local_host: inputLocalHost.value.trim() || '127.0.0.1',
        local_port: localPort,
        remote_host: inputRemoteHost.value.trim() || '127.0.0.1',
        remote_port: remotePort,
        auto_start: checkAutoStart.checked,
    };

    const session = state.savedSessions.find((s) => s.id === sessionId);
    if (!session) {
        alert('Selected session not found.');
        return;
    }

    if (!session.tunnels) {
        session.tunnels = [];
    }

    const existingIdx = session.tunnels.findIndex((t) => t.id === tunnelConfig.id);
    if (existingIdx >= 0) {
        session.tunnels[existingIdx] = tunnelConfig;
    } else {
        session.tunnels.push(tunnelConfig);
    }

    try {
        await window.__TAURI__.core.invoke('save_session', {
            session,
            password: state.masterPassword,
        });

        const modal = document.getElementById('modal-edit-tunnel');
        if (modal) modal.classList.remove('active');

        renderTunnelsList(sessionId);
    } catch (e: any) {
        alert(`Failed to save tunnel: ${e}`);
    }
}

/**
 * Render the session tunnels configuration list inside the Add/Edit Session Modal
 */
export function renderSessionModalTunnelsEditor(tunnels: TunnelConfig[]) {
    state.sessionTunnelsBeingEdited = [...tunnels];
    updateSessionModalTunnelsListUI();
}

/**
 * Update the UI table of tunnels inside the Add/Edit Session Modal
 */
export function updateSessionModalTunnelsListUI() {
    const list = document.getElementById('session-tunnels-list');
    const badge = document.getElementById('session-modal-tunnels-badge');
    if (badge) {
        badge.textContent = (state.sessionTunnelsBeingEdited?.length || 0).toString();
    }

    if (!list) return;

    if (!state.sessionTunnelsBeingEdited || state.sessionTunnelsBeingEdited.length === 0) {
        list.innerHTML = `
            <div class="session-tunnels-empty">
                No port forwarding tunnels configured for this session.
            </div>
        `;
        return;
    }

    let html = `
        <table class="session-tunnels-table">
            <thead>
                <tr>
                    <th>Rule Name</th>
                    <th>Local (Your PC)</th>
                    <th>➔</th>
                    <th>Remote LAN / Target</th>
                    <th>Auto-Start</th>
                    <th style="text-align: right;"></th>
                </tr>
            </thead>
            <tbody>
    `;

    state.sessionTunnelsBeingEdited.forEach((t, index) => {
        html += `
            <tr>
                <td><strong>${escapeHtml(t.name || 'Port ' + t.local_port)}</strong></td>
                <td><span class="tunnel-port-pill local-port">${escapeHtml(t.local_host || '127.0.0.1')}:${t.local_port}</span></td>
                <td style="color: var(--accent-primary, #6366f1); font-weight: bold; text-align: center;">➔</td>
                <td><span class="tunnel-port-pill remote-port">${escapeHtml(t.remote_host)}:${t.remote_port}</span></td>
                <td>${t.auto_start ? '<span style="color:#10b981; font-weight: 500;">Yes</span>' : '<span style="color:#6b7280;">No</span>'}</td>
                <td style="text-align: right;">
                    <button type="button" class="btn btn-sm btn-ghost btn-remove-session-tunnel" data-index="${index}" title="Remove rule" style="color: #ef4444;">
                        &times;
                    </button>
                </td>
            </tr>
        `;
    });

    html += `
            </tbody>
        </table>
    `;

    list.innerHTML = html;

    list.querySelectorAll('.btn-remove-session-tunnel').forEach((btn) => {
        btn.addEventListener('click', () => {
            const idxStr = btn.getAttribute('data-index');
            if (idxStr !== null) {
                const idx = parseInt(idxStr, 10);
                state.sessionTunnelsBeingEdited.splice(idx, 1);
                updateSessionModalTunnelsListUI();
            }
        });
    });
}

/**
 * Add a new port forward rule directly in the Add/Edit Session modal
 */
export function addTunnelToSessionEditor() {
    const nameInput = document.getElementById('new-tunnel-name') as HTMLInputElement;
    const localPortInput = document.getElementById('new-tunnel-localport') as HTMLInputElement;
    const remoteHostInput = document.getElementById('new-tunnel-remotehost') as HTMLInputElement;
    const remotePortInput = document.getElementById('new-tunnel-remoteport') as HTMLInputElement;
    const autoStartInput = document.getElementById('new-tunnel-autostart') as HTMLInputElement;

    const localPort = parseInt(localPortInput?.value || '', 10);
    const remotePort = parseInt(remotePortInput?.value || '', 10);
    const remoteHost = remoteHostInput?.value.trim() || '127.0.0.1';
    const name = nameInput?.value.trim() || `Port ${localPort} ➔ ${remoteHost}:${remotePort}`;

    if (isNaN(localPort) || localPort <= 0 || localPort > 65535) {
        alert('Please specify a valid local port number (e.g. 8080).');
        return;
    }

    if (isNaN(remotePort) || remotePort <= 0 || remotePort > 65535) {
        alert('Please specify a valid remote port number (e.g. 80).');
        return;
    }

    const newTunnel: TunnelConfig = {
        id: generateId(),
        name,
        tunnel_type: 'local',
        local_host: '127.0.0.1',
        local_port: localPort,
        remote_host: remoteHost,
        remote_port: remotePort,
        auto_start: autoStartInput ? autoStartInput.checked : false,
    };

    state.sessionTunnelsBeingEdited.push(newTunnel);
    updateSessionModalTunnelsListUI();

    // Reset inputs for quick entry of the next port forward
    if (nameInput) nameInput.value = '';
    if (localPortInput) localPortInput.value = (localPort + 1).toString();
    if (remoteHostInput) remoteHostInput.value = remoteHost;
    if (remotePortInput) remotePortInput.value = remotePort.toString();
}

/**
 * Auto-start tunnels when an SSH session connects
 */
export async function handleSessionConnected(sessionId: string) {
    const session = state.savedSessions.find((s) => s.id === sessionId);
    if (!session) return;

    if (session.auto_start_tunnels || session.tunnels?.some((t) => t.auto_start)) {
        try {
            await window.__TAURI__.core.invoke('start_session_tunnels', {
                sessionId: session.id,
                password: state.masterPassword,
            });
            await refreshActiveTunnels();
        } catch (e) {
            console.error(`Auto-start tunnels for ${session.name} failed:`, e);
        }
    }
}

function escapeHtml(text: string): string {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

/**
 * Initialize all tunnel events and UI listeners
 */
export function initTunnels() {
    // Menu item
    const menuTunnels = document.getElementById('menu-options-tunnels');
    if (menuTunnels) {
        menuTunnels.addEventListener('click', () => openTunnelsModal());
    }

    // Toolbar button
    const btnToolbarTunnels = document.getElementById('btn-toolbar-tunnels');
    if (btnToolbarTunnels) {
        btnToolbarTunnels.addEventListener('click', () => openTunnelsModal());
    }

    // Modal close
    const btnCloseTunnels = document.getElementById('tunnels-modal-close');
    const btnFooterClose = document.getElementById('btn-tunnels-close');
    if (btnCloseTunnels) btnCloseTunnels.addEventListener('click', closeTunnelsModal);
    if (btnFooterClose) btnFooterClose.addEventListener('click', closeTunnelsModal);

    // Session filter in modal
    const sessionFilter = document.getElementById('tunnels-session-filter') as HTMLSelectElement;
    if (sessionFilter) {
        sessionFilter.addEventListener('change', () => {
            renderTunnelsList(sessionFilter.value || undefined);
        });
    }

    // Add Tunnel button in manager modal
    const btnAddTunnel = document.getElementById('btn-add-tunnel');
    if (btnAddTunnel) {
        btnAddTunnel.addEventListener('click', () => {
            const currentFilter = sessionFilter?.value;
            openEditTunnelModal(null, currentFilter || undefined);
        });
    }

    // Start All / Stop All buttons
    const btnStartAll = document.getElementById('btn-tunnels-start-all');
    const btnStopAll = document.getElementById('btn-tunnels-stop-all');
    if (btnStartAll) {
        btnStartAll.addEventListener('click', () => {
            const currentFilter = sessionFilter?.value;
            startAllTunnels(currentFilter || undefined);
        });
    }
    if (btnStopAll) {
        btnStopAll.addEventListener('click', () => {
            const currentFilter = sessionFilter?.value;
            stopAllTunnels(currentFilter || undefined);
        });
    }

    // Add/Edit Tunnel Modal Close & Save
    const btnCloseEditModal = document.getElementById('tunnel-edit-modal-close');
    const btnCancelEdit = document.getElementById('btn-tunnel-edit-cancel');
    const btnSaveEdit = document.getElementById('btn-tunnel-edit-save');
    const editModal = document.getElementById('modal-edit-tunnel');

    if (btnCloseEditModal && editModal) {
        btnCloseEditModal.addEventListener('click', () => editModal.classList.remove('active'));
    }
    if (btnCancelEdit && editModal) {
        btnCancelEdit.addEventListener('click', () => editModal.classList.remove('active'));
    }
    if (btnSaveEdit) {
        btnSaveEdit.addEventListener('click', saveTunnelFromModal);
    }

    // Add button in Session Modal's Port Forwarding tab
    const btnAddSessionTunnelRule = document.getElementById('btn-add-session-tunnel-rule');
    if (btnAddSessionTunnelRule) {
        btnAddSessionTunnelRule.addEventListener('click', addTunnelToSessionEditor);
    }
}
