/**
 * SpurX Secure SSH - Main Application
 * Handles terminal management, SSH connections, and UI interactions
 */

import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { WebglAddon } from '@xterm/addon-webgl';
import '@xterm/xterm/css/xterm.css';

// Tauri API
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

import {
  ConnectRequest,
  ConnectResponse,
  SavedSession,
  QuickCommand,
  TabInfo,
  TabGroup,
  KnownHostEntry
} from './types';
import { elements } from "./ui/elements";
import { state } from "./state";
import { escapeHtml, showModal, closeModal } from "./ui/utils";
import { openSftpBrowser, closeSftpBrowser, loadSftpDirectory, uploadSftpFile, downloadSftpFile, createSftpFolder, deleteSftpFile } from "./features/sftp";
import { toggleAiPanel, renderAiMessages, sendAiMessage, showAiSettingsModal, hideAiSettingsModal, saveAiSettings, updatePrivacyBanner, getTerminalContent, analyzeTerminal, triggerSmartAutocomplete } from "./features/ai";
import { startYmodemSend, enableTabKeyboard } from "./features/ymodem";

// Editing state
// Command folders
// Track expanded folders for persistence
// SFTP state
// AI Assistant state
// AI Config interface matching Rust struct

// ============================================
// DOM Elements
// ============================================
// ============================================
// Terminal Management
// ============================================

function createTerminal(): { terminal: Terminal; fitAddon: FitAddon } {
  const terminal = new Terminal({
    theme: {
      background: '#0d1117',
      foreground: '#c9d1d9',
      cursor: '#58a6ff',
      cursorAccent: '#0d1117',
      selectionBackground: 'rgba(56, 139, 253, 0.4)',
      black: '#0d1117',
      red: '#ff7b72',
      green: '#3fb950',
      yellow: '#d29922',
      blue: '#58a6ff',
      magenta: '#bc8cff',
      cyan: '#39c5cf',
      white: '#c9d1d9',
      brightBlack: '#6e7681',
      brightRed: '#ffa198',
      brightGreen: '#56d364',
      brightYellow: '#e3b341',
      brightBlue: '#79c0ff',
      brightMagenta: '#d2a8ff',
      brightCyan: '#56d4dd',
      brightWhite: '#f0f6fc',
    },
    fontFamily: '"Cascadia Code", "JetBrains Mono", "Fira Code", Consolas, monospace',
    fontSize: 14,
    lineHeight: 1.2,
    cursorBlink: true,
    cursorStyle: 'block',
    allowProposedApi: true,
  });

  const fitAddon = new FitAddon();
  terminal.loadAddon(fitAddon);

  // Load WebGL addon
  try {
    const webglAddon = new WebglAddon();
    terminal.loadAddon(webglAddon);
  } catch (e) {
    console.warn('WebGL addon could not be loaded:', e);
  }

  return { terminal, fitAddon };
}

function generateTabId(): string {
  return `tab-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`;
}

function generateGroupId(): string {
  return `group-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`;
}

async function createTab(title: string = 'New Tab'): Promise<TabInfo> {
  const tabId = generateTabId();
  const { terminal, fitAddon } = createTerminal();

  // Create container
  const container = document.createElement('div');
  container.className = 'tab-container';
  container.id = `container-${tabId}`;
  elements.terminalViewport.appendChild(container);

  // Open terminal in container immediately
  terminal.open(container);

  const tabInfo: TabInfo = {
    id: tabId,
    title,
    terminal,
    fitAddon,
    connected: false,
    unlisteners: [],
    connectionParams: null,
    container,
  };

  // Attach Smart Autocomplete key handler
  terminal.attachCustomKeyEventHandler((arg: KeyboardEvent) => {
    const isSpace = arg.code === 'Space' || arg.key === ' ' || arg.keyCode === 32;
    if (arg.ctrlKey && isSpace) {
      if (arg.type === 'keydown') {
        triggerSmartAutocomplete(tabInfo);
      }
      return false;
    }
    
    // Fix Delete key printing ~ on network devices
    if (arg.key === 'Delete') {
      if (arg.type === 'keydown') {
        if (tabInfo.sessionId && tabInfo.connected) {
          const encoder = new TextEncoder();
          invoke('ssh_write', {
            sessionId: tabInfo.sessionId,
            data: Array.from(encoder.encode('\x7f')),
          }).catch(e => console.error('Failed to write delete key:', e));
        }
      }
      return false;
    }

    return true;
  });

  state.tabs.set(tabId, tabInfo);
  renderTabs();
  switchToTab(tabId);

  return tabInfo;
}

// ============================================
// Tab Group Management
// ============================================

const GROUP_COLORS = ['#00d4ff', '#10b981', '#f59e0b', '#f97316', '#8b5cf6', '#ec4899'];

function createTabGroup(name: string, initialTabIds: string[] = []): TabGroup {
  const groupId = generateGroupId();
  const colorIndex = state.tabGroups.size % GROUP_COLORS.length;
  const group: TabGroup = { id: groupId, name, colorIndex, collapsed: false, tabIds: [] };
  state.tabGroups.set(groupId, group);
  for (const tabId of initialTabIds) addTabToGroup(tabId, groupId);
  renderTabs();
  return group;
}

function addTabToGroup(tabId: string, groupId: string): void {
  const tab = state.tabs.get(tabId);
  const group = state.tabGroups.get(groupId);
  if (!tab || !group) return;
  // Remove from old group if needed
  if (tab.groupId && tab.groupId !== groupId) removeTabFromGroup(tabId);
  if (!group.tabIds.includes(tabId)) group.tabIds.push(tabId);
  tab.groupId = groupId;
}

function removeTabFromGroup(tabId: string): void {
  const tab = state.tabs.get(tabId);
  if (!tab || !tab.groupId) return;
  const group = state.tabGroups.get(tab.groupId);
  if (group) {
    group.tabIds = group.tabIds.filter(id => id !== tabId);
    if (group.tabIds.length === 0) state.tabGroups.delete(group.id);
  }
  tab.groupId = undefined;
  renderTabs();
}

function renameTabGroup(groupId: string, name: string): void {
  const group = state.tabGroups.get(groupId);
  if (!group) return;
  group.name = name;
  renderTabs();
}

function collapseTabGroup(groupId: string): void {
  const group = state.tabGroups.get(groupId);
  if (!group) return;
  group.collapsed = !group.collapsed;
  renderTabs();
}

async function closeTabGroup(groupId: string): Promise<void> {
  const group = state.tabGroups.get(groupId);
  if (!group) return;
  const ids = [...group.tabIds];
  state.tabGroups.delete(groupId);
  for (const tabId of ids) {
    const tab = state.tabs.get(tabId);
    if (tab) tab.groupId = undefined;
    await closeTab(tabId);
  }
  renderTabs();
}

function broadcastCommand(command: string, groupId?: string): void {
  const encoder = new TextEncoder();
  const data = Array.from(encoder.encode(command + '\r'));
  let sent = 0;
  state.tabs.forEach(tab => {
    if (!tab.connected || !tab.sessionId) return;
    if (groupId) {
      const group = state.tabGroups.get(groupId);
      if (!group || !group.tabIds.includes(tab.id)) return;
    }
    invoke('ssh_write', { sessionId: tab.sessionId, data });
    // Visual echo in terminal
    tab.terminal.write(`\x1b[36m[Broadcast]\x1b[0m `);
    sent++;
  });
  return;
}

function showBroadcastModal(groupId?: string): void {
  const modal = document.getElementById('modal-broadcast') as HTMLDivElement;
  const scopeSelect = document.getElementById('broadcast-scope') as HTMLSelectElement;
  const commandInput = document.getElementById('broadcast-command-input') as HTMLTextAreaElement;
  const titleEl = document.getElementById('broadcast-modal-title') as HTMLHeadingElement;

  // Rebuild scope options
  scopeSelect.innerHTML = '<option value="all">All Connected Sessions</option>';
  state.tabGroups.forEach(group => {
    const opt = document.createElement('option');
    opt.value = group.id;
    opt.textContent = `Group: ${group.name}`;
    scopeSelect.appendChild(opt);
  });

  if (groupId && state.tabGroups.has(groupId)) {
    scopeSelect.value = groupId;
    const group = state.tabGroups.get(groupId)!;
    titleEl.textContent = `Broadcast to "${group.name}"`;
  } else {
    scopeSelect.value = 'all';
    titleEl.textContent = 'Broadcast Command';
  }

  updateBroadcastInfo();
  commandInput.value = '';
  modal.classList.add('show');
  commandInput.focus();
}

function updateBroadcastInfo(): void {
  const scopeSelect = document.getElementById('broadcast-scope') as HTMLSelectElement;
  const targetInfo = document.getElementById('broadcast-target-info') as HTMLSpanElement;

  const scopeId = scopeSelect.value;
  const gId = scopeId === 'all' ? undefined : scopeId;

  let count = 0;
  state.tabs.forEach(tab => {
    if (!tab.connected) return;
    if (gId) {
      const group = state.tabGroups.get(gId);
      if (group && group.tabIds.includes(tab.id)) count++;
    } else {
      count++;
    }
  });

  if (count === 0) {
    targetInfo.textContent = 'No connected sessions in scope';
  } else if (gId) {
    const group = state.tabGroups.get(gId)!;
    targetInfo.textContent = `Will send to ${count} connected session${count !== 1 ? 's' : ''} in group "${group.name}"`;
  } else {
    targetInfo.textContent = `Will send to ${count} connected session${count !== 1 ? 's' : ''}`;
  }
}

function renderTabs(): void {
  // Save and restore the add button
  const addButton = elements.tabBar.querySelector('.tab-add');
  elements.tabBar.innerHTML = '';

  // Build an ordered list: groups with their member tabs, then ungrouped tabs
  const renderedTabIds = new Set<string>();

  // First, render each group header + members
  state.tabGroups.forEach(group => {
    // Group header element
    const headerEl = document.createElement('div');
    headerEl.className = `tab-group-header${group.collapsed ? ' collapsed' : ''}`;
    headerEl.dataset.groupId = group.id;
    headerEl.dataset.groupColor = String(group.colorIndex);
    headerEl.innerHTML = `
      <button class="group-btn group-chevron" title="${group.collapsed ? 'Expand' : 'Collapse'} group" data-action="toggle-group">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M9 18l6-6-6-6"/></svg>
      </button>
      <span class="group-color-dot"></span>
      <span class="group-name" contenteditable="false" title="Click to rename group">${escapeHtml(group.name)}</span>
      <button class="group-btn btn-broadcast" title="Broadcast command to this group" data-action="group-broadcast">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/></svg>
      </button>
      <button class="group-btn btn-close" title="Close tab group" data-action="close-group">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 6L6 18M6 6l12 12"/></svg>
      </button>
    `;

    // Toggle collapse
    headerEl.querySelector('[data-action="toggle-group"]')?.addEventListener('click', (e) => {
      e.stopPropagation();
      collapseTabGroup(group.id);
    });

    // Inline rename on click on group-name span
    const nameSpan = headerEl.querySelector('.group-name') as HTMLSpanElement;
    nameSpan.addEventListener('click', (e) => {
      e.stopPropagation();
      nameSpan.contentEditable = 'true';
      nameSpan.focus();
      // Select all text
      const range = document.createRange();
      range.selectNodeContents(nameSpan);
      const sel = window.getSelection();
      sel?.removeAllRanges();
      sel?.addRange(range);
    });
    nameSpan.addEventListener('blur', () => {
      const newName = nameSpan.textContent?.trim() || group.name;
      nameSpan.contentEditable = 'false';
      renameTabGroup(group.id, newName);
    });
    nameSpan.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') { e.preventDefault(); nameSpan.blur(); }
      if (e.key === 'Escape') { nameSpan.textContent = group.name; nameSpan.blur(); }
    });

    // Broadcast button
    headerEl.querySelector('[data-action="group-broadcast"]')?.addEventListener('click', (e) => {
      e.stopPropagation();
      showBroadcastModal(group.id);
    });

    // Close-group button
    headerEl.querySelector('[data-action="close-group"]')?.addEventListener('click', (e) => {
      e.stopPropagation();
      closeTabGroup(group.id);
    });

    elements.tabBar.appendChild(headerEl);

    // Render member tabs (respecting collapse state)
    group.tabIds.forEach(tabId => {
      const tab = state.tabs.get(tabId);
      if (!tab) return;
      renderedTabIds.add(tabId);
      if (group.collapsed) return; // hide but track

      const tabEl = buildTabElement(tab);
      tabEl.classList.add('in-group');
      tabEl.dataset.groupColor = String(group.colorIndex);
      elements.tabBar.appendChild(tabEl);
    });
  });

  // Then render ungrouped tabs
  state.tabs.forEach(tab => {
    if (renderedTabIds.has(tab.id)) return;
    elements.tabBar.appendChild(buildTabElement(tab));
  });

  // Re-add the add button
  if (addButton) {
    elements.tabBar.appendChild(addButton);
  } else {
    const newAddButton = document.createElement('button');
    newAddButton.className = 'tab-add';
    newAddButton.id = 'btn-add-tab';
    newAddButton.title = 'New Tab';
    newAddButton.textContent = '+';
    newAddButton.addEventListener('click', () => createTab());
    elements.tabBar.appendChild(newAddButton);
  }

  // Update broadcast toolbar button styling
  const broadcastBtn = document.getElementById('btn-broadcast');
  if (broadcastBtn) {
    broadcastBtn.classList.toggle('has-groups', state.tabGroups.size > 0);
  }
}

function buildTabElement(tab: TabInfo): HTMLDivElement {
  const tabElement = document.createElement('div');
  tabElement.className = `tab${tab.id === state.activeTabId ? ' active' : ''}`;
  tabElement.dataset.tabId = tab.id;

  tabElement.innerHTML = `
    <svg class="tab-icon${tab.connected ? ' connected' : ''}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
      <rect x="2" y="3" width="20" height="14" rx="2"/>
      <path d="M8 21h8M12 17v4"/>
    </svg>
    <span class="tab-title">${escapeHtml(tab.title)}</span>
    <span class="tab-close">&times;</span>
  `;

  tabElement.addEventListener('click', (e) => {
    const target = e.target as HTMLElement;
    if (target.classList.contains('tab-close')) {
      closeTab(tab.id);
    } else {
      switchToTab(tab.id);
    }
  });

  tabElement.addEventListener('contextmenu', (e) => {
    e.preventDefault();
    showTabContextMenu(e.clientX, e.clientY, tab.id);
  });

  return tabElement;
}

function switchToTab(tabId: string): void {
  const tab = state.tabs.get(tabId);
  if (!tab) return;

  state.activeTabId = tabId;
  renderTabs();

  // Hide all containers
  state.tabs.forEach(t => {
    t.container.classList.remove('active');
  });

  // Show active container
  tab.container.classList.add('active');

  // Fit terminal to viewport
  setTimeout(() => {
    tab.fitAddon.fit();
    tab.terminal.focus();
    updateStatusBar(tab);
  }, 50); // Small delay to ensure layout is applied
}

async function closeTab(tabId: string): Promise<void> {
  const tab = state.tabs.get(tabId);
  if (!tab) return;

  // Disconnect if connected
  if (tab.sessionId) {
    await invoke('ssh_disconnect', { sessionId: tab.sessionId });
  }

  // Clean up event listeners
  tab.unlisteners.forEach((unlisten) => unlisten());

  // Dispose terminal
  tab.terminal.dispose();

  // Remove container from DOM
  tab.container.remove();

  // Remove from tabs
  state.tabs.delete(tabId);

  // Switch to another tab or create new one
  if (state.tabs.size === 0) {
    await createTab();
  } else if (state.activeTabId === tabId) {
    const firstTab = state.tabs.values().next().value;
    if (firstTab) {
      switchToTab(firstTab.id);
    }
  }

  renderTabs();
}

function updateStatusBar(tab: TabInfo): void {
  const dims = tab.terminal.rows && tab.terminal.cols
    ? `${tab.terminal.rows} Rows, ${tab.terminal.cols} Cols`
    : '-';

  elements.statusConnection.textContent = tab.connected ? 'Connected' : 'Ready';
  elements.statusCipher.textContent = tab.connected ? 'ssh2: AES-256-CTR' : '-';
  elements.statusSize.textContent = dims;
}

// ============================================
// SSH Connection
// ============================================

async function connect(
  host: string,
  port: number,
  username: string,
  password?: string,
  saveSession: boolean = false,
  log_directory?: string,
  authMethod: string = 'password',
  privateKeyName?: string,
  privateKeyPassphrase?: string
): Promise<void> {
  let tab = state.activeTabId ? state.tabs.get(state.activeTabId) : null;

  // Create new tab if none exists or current is connected
  if (!tab || tab.connected) {
    tab = await createTab(host);
  }

  // Update tab title
  tab.title = `${username}@${host}`;
  renderTabs();

  // Clear terminal
  tab.terminal.clear();
  tab.terminal.writeln(`\x1b[36mConnecting to ${host}:${port}...\x1b[0m`);

  try {
    const response = await invoke<ConnectResponse>('ssh_connect', {
      request: {
        host,
        port,
        username,
        password: password || undefined,
        log_directory,
        use_global_credentials: elements.sessionUseGlobal.checked,
        auth_method: authMethod,
        private_key_name: privateKeyName || undefined,
        private_key_passphrase: privateKeyPassphrase || undefined,
      } as ConnectRequest,
    });

    if (response.success) {
      tab.sessionId = response.session_id;
      tab.connected = true;
      tab.connectionParams = {
        host,
        port,
        username,
        password: password || undefined,
        log_directory,
        use_global_credentials: elements.sessionUseGlobal.checked,
        auth_method: authMethod,
        private_key_name: privateKeyName || undefined,
        private_key_passphrase: privateKeyPassphrase || undefined,
      } as ConnectRequest;
      renderTabs();

      // Listen for terminal data
      const unlistenData = await listen<number[]>(
        `terminal-data-${response.session_id}`,
        (event) => {
          const data = new Uint8Array(event.payload);
          tab!.terminal.write(data);
        }
      );
      tab.unlisteners.push(unlistenData);

      // Listen for session close
      const unlistenClose = await listen<void>(
        `session-closed-${response.session_id}`,
        () => {
          tab!.connected = false;
          tab!.sessionId = undefined;
          tab!.terminal.writeln('\r\n\x1b[33mConnection closed.\x1b[0m');
          renderTabs();
          updateStatusBar(tab!);
        }
      );
      tab.unlisteners.push(unlistenClose);

      // Listen for errors
      const unlistenError = await listen<string>(
        `session-error-${response.session_id}`,
        (event) => {
          tab!.terminal.writeln(`\r\n\x1b[31mError: ${event.payload}\x1b[0m`);
        }
      );
      tab.unlisteners.push(unlistenError);

      // Send terminal input to backend
      enableTabKeyboard(tab);

      // Handle resize
      tab.terminal.onResize((size) => {
        updateStatusBar(tab!);
        if (tab!.sessionId && tab!.connected) {
          invoke('ssh_resize', {
            sessionId: tab!.sessionId,
            rows: size.rows,
            cols: size.cols,
          });
        }
      });

      updateStatusBar(tab);

      // Perform initial fit and resize
      setTimeout(() => {
        tab!.fitAddon.fit();
        if (tab!.terminal.rows > 0 && tab!.terminal.cols > 0) {
          invoke('ssh_resize', {
            sessionId: tab!.sessionId,
            rows: tab!.terminal.rows,
            cols: tab!.terminal.cols,
          });
        }
        tab!.terminal.focus();
      }, 100);

      // Save session if requested
      if (saveSession) {
        await invoke('save_session', {
          session: {
            id: crypto.randomUUID(),
            name: `${username}@${host}`,
            host,
            port,
            folder: undefined,
            username,
            log_directory,
            auth_method: authMethod,
            private_key_name: privateKeyName || undefined,
          } as SavedSession,
          password: state.masterPassword || null
        });
        await loadSavedSessions();
      }
    } else {
      tab.terminal.writeln(`\x1b[31mConnection failed: ${response.error}\x1b[0m`);
    }
  } catch (error) {
    tab.terminal.writeln(`\x1b[31mConnection error: ${error}\x1b[0m`);
  }
}

// ============================================
// Session Management
// ============================================

function promptMasterPassword(description: string): Promise<string> {
  return new Promise((resolve, reject) => {
    const modal = elements.passwordPromptModal;
    const desc = elements.passwordPromptDescription;
    const input = elements.passwordPromptInput;
    const confirmBtn = elements.passwordPromptConfirm;
    const cancelBtn = elements.passwordPromptCancel;
    const closeBtn = elements.passwordPromptClose;

    if (!modal || !input || !confirmBtn || !cancelBtn || !closeBtn) {
      reject(new Error('Password prompt modal elements not found'));
      return;
    }

    if (desc) desc.textContent = description;
    input.value = '';
    showModal(modal);
    input.focus();

    const cleanUp = () => {
      confirmBtn.removeEventListener('click', onConfirm);
      cancelBtn.removeEventListener('click', onCancel);
      closeBtn.removeEventListener('click', onCancel);
      closeModal(modal);
    };

    const onConfirm = () => {
      const val = input.value;
      cleanUp();
      resolve(val);
    };

    const onCancel = () => {
      cleanUp();
      reject(new Error('USER_CANCELLED'));
    };

    confirmBtn.addEventListener('click', onConfirm);
    cancelBtn.addEventListener('click', onCancel);
    closeBtn.addEventListener('click', onCancel);
    
    input.onkeydown = (e) => {
      if (e.key === 'Enter') {
        onConfirm();
      }
    };
  });
}

async function updateMasterPasswordStatusUI(isSet?: boolean): Promise<void> {
  const statusEl = document.getElementById('master-password-status');
  if (statusEl) {
    const active = isSet !== undefined ? isSet : await invoke<boolean>('is_master_password_set');
    if (active) {
      statusEl.textContent = 'Status: Enabled';
      statusEl.style.color = 'var(--accent-success)';
    } else {
      statusEl.textContent = 'Status: Disabled';
      statusEl.style.color = 'var(--text-muted)';
    }
  }
}

async function loadSavedSessions(pwdToCheck?: string): Promise<void> {
  try {
    const pwd = pwdToCheck !== undefined ? pwdToCheck : (state.masterPassword || undefined);
    const sessions = await invoke<SavedSession[]>('get_saved_sessions', { password: pwd });
    
    if (pwd) {
      state.masterPassword = pwd;
    }
    state.savedSessions = sessions;

    // Sync folders
    state.savedSessions.forEach(s => {
      if (s.folder && !state.sessionFolders.includes(s.folder)) state.sessionFolders.push(s.folder);
    });

    renderSessionsList();
    renderSessionsTree();
    await updateMasterPasswordStatusUI(true);
  } catch (error) {
    if (error === 'KEY_REQUIRED' || error === 'INVALID_PASSWORD') {
      try {
        const promptMsg = error === 'INVALID_PASSWORD' 
          ? 'Incorrect master password. Please try again:' 
          : 'Please enter your master password to unlock your sessions:';
        const inputPwd = await promptMasterPassword(promptMsg);
        await loadSavedSessions(inputPwd);
      } catch (promptError) {
        if (promptError instanceof Error && promptError.message === 'USER_CANCELLED') {
          console.log('User cancelled master password prompt');
          state.masterPassword = null;
          await updateMasterPasswordStatusUI(false);
        } else {
          console.error('Prompt error:', promptError);
        }
      }
    } else {
      console.error('Failed to load sessions:', error);
    }
  }
}



function renderSessionsList(): void {
  if (state.savedSessions.length === 0) {
    elements.sessionsList.innerHTML = '<div class="sessions-empty">No saved sessions yet</div>';
    return;
  }

  elements.sessionsList.innerHTML = state.savedSessions.map((session) => `
    <div class="session-item" data-session-id="${session.id}">
      <div class="session-icon">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <rect x="2" y="3" width="20" height="14" rx="2"/>
          <path d="M8 21h8M12 17v4"/>
        </svg>
      </div>
      <div class="session-info">
        <div class="session-name">${escapeHtml(session.name)}</div>
        <div class="session-host">${escapeHtml(session.host)}:${session.port}</div>
      </div>
      <button class="session-delete" data-session-id="${session.id}" title="Delete">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <polyline points="3 6 5 6 21 6"/>
          <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>
        </svg>
      </button>
    </div>
  `).join('');

  // Add event listeners
  elements.sessionsList.querySelectorAll('.session-item').forEach((item) => {
    const sessionId = (item as HTMLElement).dataset.sessionId;

    // Delete button
    const deleteBtn = item.querySelector('.session-delete');
    deleteBtn?.addEventListener('click', async (e) => {
      e.stopPropagation();
      if (sessionId) {
        await invoke('delete_session', { sessionId, password: state.masterPassword || null });
        await loadSavedSessions();
      }
    });

    // Click to connect
    item.addEventListener('click', () => {
      const session = state.savedSessions.find((s) => s.id === sessionId);
      if (session) {
        closeModal(elements.sessionsModal);
        showQuickConnectModal(session.host, session.port);
      }
    });
  });
}

// ============================================
// Quick Commands
// ============================================

async function loadQuickCommands(): Promise<void> {
  try {
    state.quickCommands = await invoke<QuickCommand[]>('get_quick_commands');

    // Sync folders
    state.quickCommands.forEach(c => {
      if (c.folder && !state.commandFolders.includes(c.folder)) state.commandFolders.push(c.folder);
    });

    renderCommandTree();
  } catch (error) {
    console.error('Failed to load quick commands:', error);
  }
}

// ============================================
// Folder & Tree Logic
// ============================================

interface FolderNode {
  name: string;
  path: string;
  children: Map<string, FolderNode>;
  items: (SavedSession | QuickCommand)[];
  isOpen: boolean;
}

function buildFolderTree(
  folders: string[],
  items: (SavedSession | QuickCommand)[],
  expandedPaths: Set<string> = new Set()
): Map<string, FolderNode> {
  const root = new Map<string, FolderNode>();

  // Helper to get or create node
  const getOrCreateNode = (fullPath: string): FolderNode => {
    const parts = fullPath.split('/');
    let currentPath = '';
    let currentMap = root;

    for (let i = 0; i < parts.length; i++) {
      const part = parts[i];
      currentPath = currentPath ? `${currentPath}/${part}` : part;

      if (!currentMap.has(part)) {
        currentMap.set(part, {
          name: part,
          path: currentPath,
          children: new Map(),
          items: [],
          isOpen: expandedPaths.has(currentPath)
        });
      }

      if (i === parts.length - 1) {
        return currentMap.get(part)!;
      }
      currentMap = currentMap.get(part)!.children;
    }
    return currentMap.get(parts[parts.length - 1])!;
  };

  // 1. Ensure all defined folders exist in tree
  folders.forEach(folderPath => {
    if (folderPath && folderPath !== 'General') getOrCreateNode(folderPath);
  });

  // 2. Place items
  items.forEach(item => {
    if (!item.folder) {
      // Root items (or General)
      // We can put them in a special root list or implied 'General'
      // Current design puts everything in folders. behavior: empty folder -> root items
      // Let's return root items separately? 
      // Or just add to a 'General' node if we want them grouped?
      // Use existing array for root items if needed.
    } else {
      const node = getOrCreateNode(item.folder);
      node.items.push(item);
    }
  });

  return root;
}

function renderTreeHtml(
  nodes: Map<string, FolderNode>,
  type: 'session' | 'command',
  rootItems: (SavedSession | QuickCommand)[] = []
): string {
  let html = '';

  // Render folders (sorted)
  const sortedKeys = Array.from(nodes.keys()).sort();

  sortedKeys.forEach(key => {
    const node = nodes.get(key)!;
    const isExpanded = node.isOpen; // Use node state

    html += `
      <div class="folder-container" data-path="${escapeHtml(node.path)}" data-type="${type}">
        <div class="folder-header ${isExpanded ? 'expanded' : ''}">
          <svg class="folder-icon" viewBox="0 0 24 24" fill="currentColor">
            <path d="M10 6L8 4H2v16h20V6H10z"/>
          </svg>
          <span class="folder-name">${escapeHtml(node.name)}</span>
          <div class="folder-actions">
           <!-- Context menu trigger implicit or explicit -->
          </div>
        </div>
        <div class="folder-content">
          ${renderTreeHtml(node.children, type)}
          ${type === 'session'
        ? node.items.map(i => renderSessionItem(i as SavedSession)).join('')
        : node.items.map(i => renderCommandItem(i as QuickCommand)).join('')
      }
        </div>
      </div>
    `;
  });

  // Render root items LAST (if any)
  if (rootItems.length > 0) {
    if (type === 'session') {
      html += rootItems.map(item => renderSessionItem(item as SavedSession)).join('');
    } else {
      html += rootItems.map(item => renderCommandItem(item as QuickCommand)).join('');
    }
  }

  return html;
}

function renderSessionItem(session: SavedSession): string {
  return `
    <div class="session-tree-item" data-session-id="${session.id}">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <rect x="2" y="3" width="20" height="14" rx="2"/>
        <path d="M8 21h8M12 17v4"/>
      </svg>
      <div class="session-info">
        <span class="session-name">${escapeHtml(session.name || session.host)}</span>
        <span class="session-host">${escapeHtml(session.host)}:${session.port}</span>
      </div>
    </div>
  `;
}

function renderCommandItem(cmd: QuickCommand): string {
  return `
    <div class="command-item" data-command-id="${cmd.id}"
         onmouseenter="showCommandTooltip(event, '${cmd.id}')"
         onmouseleave="hideCommandTooltip()">
      <div class="command-icon ${cmd.icon || 'cyan'}"></div>
      <div class="command-info">
        <div class="command-name">${escapeHtml(cmd.name)}</div>
        <div class="command-text">${escapeHtml(cmd.command)}</div>
      </div>
    </div>
  `;
}



// Tooltip functions
(window as any).showCommandTooltip = (e: MouseEvent, commandId: string) => {
  if (state.tooltipHideTimer) {
    window.clearTimeout(state.tooltipHideTimer);
    state.tooltipHideTimer = null;
  }
  const cmd = state.quickCommands.find(c => c.id === commandId);
  if (!cmd) return;

  const tooltip = document.getElementById('command-tooltip');
  const nameEl = document.getElementById('tooltip-name');
  const descEl = document.getElementById('tooltip-description');
  const exampleContainer = document.getElementById('tooltip-example-container');
  const exampleEl = document.getElementById('tooltip-example');
  const commandEl = document.getElementById('tooltip-command');

  if (!tooltip || !nameEl || !descEl || !exampleEl || !commandEl || !exampleContainer) return;

  nameEl.textContent = cmd.name;
  descEl.textContent = cmd.description || '';

  if (cmd.example) {
    exampleContainer.classList.remove('hidden');
    exampleEl.textContent = cmd.example;
  } else {
    exampleContainer.classList.add('hidden');
  }

  commandEl.textContent = cmd.command;

  // Position
  const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
  tooltip.style.top = `${rect.top}px`;
  tooltip.style.left = `${rect.right + 10}px`; // Show to the right

  // Check bounds
  if (rect.right + 310 > window.innerWidth) {
    tooltip.style.left = `${rect.left - 310}px`; // Show to left if no space
  }

  tooltip.classList.remove('hidden');
};

(window as any).hideCommandTooltip = () => {
  if (state.tooltipHideTimer) window.clearTimeout(state.tooltipHideTimer);
  state.tooltipHideTimer = window.setTimeout(() => {
    const tooltip = document.getElementById('command-tooltip');
    if (tooltip) tooltip.classList.add('hidden');
  }, 300); // 300ms delay to allow moving mouse into tooltip
};

// Updated render functions
function renderCommandTree(): void {
  const filterInput = document.getElementById('command-search') as HTMLInputElement;
  const filter = filterInput?.value.toLowerCase().trim() || '';

  let items = state.quickCommands;
  if (filter) {
    items = state.quickCommands.filter(c =>
      c.name.toLowerCase().includes(filter) ||
      c.command.toLowerCase().includes(filter) ||
      (c.description && c.description.toLowerCase().includes(filter)) ||
      (c.example && c.example.toLowerCase().includes(filter))
    );
  }

  const rootItems = items.filter(c => !c.folder);
  const folders = buildFolderTree(state.commandFolders, items.filter(c => c.folder), state.expandedCommandFolders);

  elements.commandTree.innerHTML = renderTreeHtml(folders, 'command', rootItems);

  attachTreeListeners(elements.commandTree, 'command');

  // Expand all folders if filtering
  if (filter) {
    elements.commandTree.querySelectorAll('.folder-header').forEach(header => header.classList.add('expanded'));
  }
}

function renderSessionsTree(): void {
  const filterInput = document.querySelector('.sidebar-search') as HTMLInputElement;
  const filter = filterInput?.value.toLowerCase().trim() || '';

  let items = state.savedSessions;
  if (filter) {
    items = state.savedSessions.filter(s =>
      s.name.toLowerCase().includes(filter) ||
      s.host.toLowerCase().includes(filter) ||
      (s.username && s.username.toLowerCase().includes(filter))
    );
  }

  const rootItems = items.filter(s => !s.folder);
  const folders = buildFolderTree(state.sessionFolders, items.filter(s => s.folder), state.expandedSessionFolders);

  elements.sessionsTree.innerHTML = renderTreeHtml(folders, 'session', rootItems);

  attachTreeListeners(elements.sessionsTree, 'session');

  // Expand all folders if filtering
  if (filter) {
    elements.sessionsTree.querySelectorAll('.folder-header').forEach(header => header.classList.add('expanded'));
  }
}

function attachTreeListeners(container: HTMLElement, type: 'session' | 'command'): void {
  // Folder toggles
  container.querySelectorAll('.folder-header').forEach((header) => {
    header.addEventListener('click', () => {
      // Don't toggle if clicking actions (if added later)
      header.classList.toggle('expanded');

      const path = (header.parentElement as HTMLElement).dataset.path;
      if (path) {
        const expandedSet = type === 'session' ? state.expandedSessionFolders : state.expandedCommandFolders;
        if (header.classList.contains('expanded')) {
          expandedSet.add(path);
        } else {
          expandedSet.delete(path);
        }
      }
    });

    // Folder context menu
    header.addEventListener('contextmenu', (e) => {
      e.preventDefault();
      e.stopPropagation();
      const path = (header.parentElement as HTMLElement).dataset.path;
      if (path) showFolderContextMenu(e as MouseEvent, path, type);
    });
  });

  if (type === 'session') {
    container.querySelectorAll('.session-tree-item').forEach((item) => {
      const sessionId = (item as HTMLElement).dataset.sessionId;
      item.addEventListener('dblclick', () => {
        const session = state.savedSessions.find((s) => s.id === sessionId);
        if (session) showLoginModal(session);
      });
      item.addEventListener('contextmenu', (e) => {
        e.preventDefault();
        e.stopPropagation();
        showContextMenu(e as MouseEvent, sessionId || null);
      });
    });
  } else {
    container.querySelectorAll('.command-item').forEach((item) => {
      const commandId = (item as HTMLElement).dataset.commandId;
      item.addEventListener('click', () => {
        const command = state.quickCommands.find((c) => c.id === commandId);
        if (command) executeQuickCommand(command);
      });
      item.addEventListener('contextmenu', (e) => {
        e.preventDefault();
        e.stopPropagation();
        showCommandContextMenu(e as MouseEvent, commandId || null);
      });
    });
  }
}

// Delete Folder Logic
async function deleteFolder(path: string, type: 'session' | 'command'): Promise<void> {
  if (!confirm(`Are you sure you want to delete folder "${path}" and all its contents?`)) return;

  if (type === 'session') {
    // Remove sessions in this folder key
    const toDelete = state.savedSessions.filter(s => s.folder === path || s.folder?.startsWith(path + '/'));
    for (const s of toDelete) {
      await invoke('delete_session', { sessionId: s.id, password: state.masterPassword || null });
    }
    // Update folders list
    state.sessionFolders = state.sessionFolders.filter(f => f !== path && !f.startsWith(path + '/'));
    await loadSavedSessions();
  } else {
    // Command deletion
    const toDelete = state.quickCommands.filter(c => c.folder === path || c.folder.startsWith(path + '/'));

    for (const c of toDelete) {
      await invoke('delete_quick_command', { commandId: c.id });
    }

    state.commandFolders = state.commandFolders.filter(f => f !== path && !f.startsWith(path + '/'));
    await loadQuickCommands();
  }
}


function executeQuickCommand(command: QuickCommand): void {
  const tab = state.activeTabId ? state.tabs.get(state.activeTabId) : null;
  if (tab && tab.sessionId) {
    const encoder = new TextEncoder();
    invoke('ssh_write', {
      sessionId: tab.sessionId,
      data: Array.from(encoder.encode(command.command)),
    });
  }
}

// ============================================
// Modal Management
// ============================================

function showQuickConnectModal(host?: string, port?: number): void {
  if (host) elements.inputHost.value = host;
  if (port) elements.inputPort.value = port.toString();
  elements.inputUsername.value = '';
  elements.inputPassword.value = '';
  elements.inputSaveSession.checked = false;

  const authSelect = document.getElementById('input-auth-method') as HTMLSelectElement | null;
  if (authSelect) authSelect.value = 'password';
  const pwdGroup = document.getElementById('quick-password-group');
  const keyGroup = document.getElementById('quick-key-group');
  if (pwdGroup) pwdGroup.style.display = 'block';
  if (keyGroup) keyGroup.style.display = 'none';
  const passphraseInput = document.getElementById('input-key-passphrase') as HTMLInputElement | null;
  if (passphraseInput) passphraseInput.value = '';

  populateSshKeysDropdowns();

  elements.quickConnectModal.classList.add('show');
  elements.inputHost.focus();
}

// ============================================
// Context Menu
// ============================================

function showContextMenu(e: MouseEvent, sessionId: string | null): void {
  state.contextMenuType = 'session';
  state.contextMenuTargetSessionId = sessionId;
  updateContextMenuItems('session');
  displayMenu(e);
}

function showFolderContextMenu(e: MouseEvent, path: string, type: 'session' | 'command'): void {
  state.contextMenuType = 'folder'; // We need to distinguish session/command folder? 
  // We'll trust handleContextMenuAction to know which structure to delete from based on `contextMenuTargetFolder`? 
  // No, we need to know the type.
  // Let's rely on finding the folder in sessionFolders vs commandFolders? 
  // But names can collide.
  // I'll store the type in a temp property or use the global `folderContext`.
  // Let's reuse `folderContext` variable for this purpose temporarily or add a var.
  // Let's set the global `folderContext` which I already have (used for add modal).
  state.folderContext = type;
  state.contextMenuTargetFolder = path;

  updateContextMenuItems('folder');
  displayMenu(e);
}

function showCommandContextMenu(e: MouseEvent, commandId: string | null): void {
  state.contextMenuType = 'command';
  state.contextMenuTargetCommandId = commandId;
  updateContextMenuItems('command');
  displayMenu(e);
}

function displayMenu(e: MouseEvent): void {
  const menu = elements.contextMenu;
  menu.style.left = `${e.clientX}px`;
  menu.style.top = `${e.clientY}px`;
  menu.classList.add('visible');

  // Adjust position
  const rect = menu.getBoundingClientRect();
  if (rect.right > window.innerWidth) {
    menu.style.left = `${e.clientX - rect.width}px`;
  }
  if (rect.bottom > window.innerHeight) {
    menu.style.top = `${e.clientY - rect.height}px`;
  }
}

function updateContextMenuItems(type: 'session' | 'command' | 'folder'): void {
  let items = '';

  if (type === 'session') {
    items = `
      <div class="context-menu-item" data-action="connect">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M5 12h14M12 5l7 7-7 7"/></svg>
        Connect
      </div>
      <div class="context-menu-item" data-action="edit">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"/></svg>
        Edit
      </div>
      <div class="context-menu-item" data-action="move">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M5 9l4-4 4 4M9 5v14"/></svg>
        Move to Folder
      </div>
      <div class="context-menu-separator"></div>
      <div class="context-menu-item danger" data-action="delete">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
        Delete
      </div>
    `;
  } else if (type === 'command') {
    items = `
      <div class="context-menu-item" data-action="run">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polygon points="5 3 19 12 5 21 5 3"/></svg>
        Run
      </div>
      <div class="context-menu-item" data-action="edit">Edit</div>
      <div class="context-menu-item" data-action="move">Move to Folder</div>
      <div class="context-menu-item" data-action="copy">Copy</div>
      <div class="context-menu-separator"></div>
      <div class="context-menu-item danger" data-action="delete">Delete</div>
    `;
  } else if (type === 'folder') {
    items = `
      <div class="context-menu-item" data-action="new-subfolder">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/><path d="M12 11v6M9 14h6"/></svg>
        New Subfolder
      </div>
      <div class="context-menu-item danger" data-action="delete-folder">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
        Delete Folder
      </div>
    `;
  }

  elements.contextMenu.innerHTML = items;

  // Re-attach listeners (since we replaced innerHTML)
  elements.contextMenu.querySelectorAll('.context-menu-item').forEach((item) => {
    item.addEventListener('click', () => {
      const action = (item as HTMLElement).dataset.action;
      if (action) {
        handleContextMenuAction(action);
      }
    });
  });
}

function hideContextMenu(): void {
  elements.contextMenu.classList.remove('visible');
  state.contextMenuTargetSessionId = null;
  state.contextMenuTargetCommandId = null;
  state.contextMenuTargetFolder = null;
  state.contextMenuType = null;
}

// ============================================
// Tab Context Menu Logic
// ============================================

function showTabContextMenu(x: number, y: number, tabId: string): void {
  hideContextMenu(); // Close other menu
  state.contextMenuTargetTabId = tabId;
  const tab = state.tabs.get(tabId);

  if (!tab) return;

  // Update menu items based on state
  const disconnectItem = elements.tabContextMenu.querySelector('[data-action="disconnect"]') as HTMLElement;
  const reconnectItem = elements.tabContextMenu.querySelector('[data-action="reconnect"]') as HTMLElement;
  const saveItem = elements.tabContextMenu.querySelector('[data-action="save"]') as HTMLElement;
  const cloneItem = elements.tabContextMenu.querySelector('[data-action="clone"]') as HTMLElement;
  const sftpItem = elements.tabContextMenu.querySelector('[data-action="sftp"]') as HTMLElement;
  const closeGroupItem = elements.tabContextMenu.querySelector('[data-action="close-group"]') as HTMLElement;
  const moveToGroupItem = elements.tabContextMenu.querySelector('[data-action="move-to-group"]') as HTMLElement;
  const removeFromGroupItem = elements.tabContextMenu.querySelector('[data-action="remove-from-group"]') as HTMLElement;
  const broadcastGroupItem = elements.tabContextMenu.querySelector('[data-action="broadcast-to-group"]') as HTMLElement;

  if (tab.connected) {
    disconnectItem.style.display = 'flex';
    reconnectItem.style.display = 'none';
    if (sftpItem) sftpItem.style.display = 'flex';
  } else {
    disconnectItem.style.display = 'none';
    reconnectItem.style.display = 'flex';
    if (sftpItem) sftpItem.style.display = 'none';
  }

  // Can only save/clone if we have connection details
  if (tab.connectionParams) {
    saveItem.style.display = 'flex';
    if (cloneItem) cloneItem.style.display = 'flex';
  } else {
    saveItem.style.display = 'none';
    if (cloneItem) cloneItem.style.display = 'none';
  }

  // Group-related items
  const isInGroup = !!tab.groupId;
  if (closeGroupItem) closeGroupItem.style.display = isInGroup ? 'flex' : 'none';
  if (removeFromGroupItem) removeFromGroupItem.style.display = isInGroup ? 'flex' : 'none';
  if (broadcastGroupItem) broadcastGroupItem.style.display = isInGroup ? 'flex' : 'none';
  // Move-to-group only shown if there are existing groups to move to
  if (moveToGroupItem) moveToGroupItem.style.display = state.tabGroups.size > 0 ? 'flex' : 'none';

  // Position and show
  elements.tabContextMenu.style.left = `${x}px`;
  elements.tabContextMenu.style.top = `${y}px`;
  elements.tabContextMenu.classList.add('visible');
}

function hideTabContextMenu(): void {
  elements.tabContextMenu.classList.remove('visible');
  state.contextMenuTargetTabId = null;
}

async function handleTabContextMenuAction(action: string): Promise<void> {
  if (!state.contextMenuTargetTabId) return;
  const tabId = state.contextMenuTargetTabId;
  const tab = state.tabs.get(tabId);

  hideTabContextMenu();

  if (!tab) return;

  switch (action) {
    case 'disconnect':
      if (tab.sessionId && tab.connected) {
        try {
          await invoke('ssh_disconnect', { sessionId: tab.sessionId });
          tab.connected = false;
          renderTabs();
          tab.terminal.writeln('\r\n\x1b[33mDisconnected by user.\x1b[0m\r\n');
        } catch (e) {
          console.error('Disconnect failed:', e);
        }
      }
      break;

    case 'reconnect':
      if (tab.connectionParams) {
        const { host, port, username, password, log_directory } = tab.connectionParams;
        if (state.activeTabId !== tabId) switchToTab(tabId);
        await connect(host, port, username, password, false, log_directory);
      }
      break;

    case 'save':
      if (tab.connectionParams) {
        showAddSessionModal();
        elements.sessionName.value = tab.title;
        elements.sessionHost.value = tab.connectionParams.host;
        elements.sessionPort.value = tab.connectionParams.port.toString();
        elements.sessionUsername.value = tab.connectionParams.username;
        elements.sessionName.focus();
      }
      break;

    case 'close':
      closeTab(tabId);
      break;

    case 'rename':
      const newName = prompt('Enter new tab name:', tab.title);
      if (newName) {
        tab.title = newName;
        renderTabs();
      }
      break;

    case 'reset-name':
      if (tab.connectionParams) {
        tab.title = `${tab.connectionParams.username}@${tab.connectionParams.host}`;
      } else {
        tab.title = 'New Tab';
      }
      renderTabs();
      break;

    case 'close-disconnected':
      const disconnectedIds: string[] = [];
      state.tabs.forEach((t) => {
        if (!t.connected) disconnectedIds.push(t.id);
      });
      for (const id of disconnectedIds) await closeTab(id);
      break;

    case 'close-others':
      const otherIds: string[] = [];
      state.tabs.forEach((t) => {
        if (t.id !== tabId) otherIds.push(t.id);
      });
      for (const id of otherIds) await closeTab(id);
      break;

    case 'close-right':
      const ids = Array.from(state.tabs.keys());
      const currentIndex = ids.indexOf(tabId);
      if (currentIndex !== -1) {
        const rightIds = ids.slice(currentIndex + 1);
        for (const id of rightIds) await closeTab(id);
      }
      break;

    case 'close-all':
      const allIds = Array.from(state.tabs.keys());
      for (const id of allIds) await closeTab(id);
      break;

    case 'clone':
      if (tab.connectionParams) {
        const { host, port, username, password, log_directory } = tab.connectionParams;
        await createTab(host); // Create new tab
        await connect(host, port, username, password, false, log_directory); // Connect in new tab
      }
      break;

    case 'sftp':
      if (state.activeTabId !== tabId) switchToTab(tabId);
      openSftpBrowser();
      break;

    case 'ymodem-send':
      if (state.activeTabId !== tabId) switchToTab(tabId);
      startYmodemSend();
      break;

    case 'add-group': {
      const groupName = prompt('New group name:', 'Group 1');
      if (groupName) createTabGroup(groupName.trim(), [tabId]);
      break;
    }

    case 'move-to-group': {
      if (state.tabGroups.size === 0) break;
      const groupList = Array.from(state.tabGroups.values());
      const opts = groupList.map((g, i) => `${i + 1}: ${g.name}`).join('\n');
      const choice = prompt(`Move to which group?\n${opts}\n\nEnter number:`);
      if (choice) {
        const idx = parseInt(choice, 10) - 1;
        if (idx >= 0 && idx < groupList.length) {
          addTabToGroup(tabId, groupList[idx].id);
          renderTabs();
        }
      }
      break;
    }

    case 'remove-from-group':
      removeTabFromGroup(tabId);
      break;

    case 'broadcast-to-group':
      if (tab.groupId) showBroadcastModal(tab.groupId);
      break;

    case 'close-group':
      if (tab.groupId) await closeTabGroup(tab.groupId);
      break;
  }
}


function handleContextMenuAction(action: string): void {
  if (state.contextMenuType === 'session' && state.contextMenuTargetSessionId) {
    const session = state.savedSessions.find((s) => s.id === state.contextMenuTargetSessionId);
    if (!session) return;

    switch (action) {
      case 'connect':
        showLoginModal(session);
        break;
      case 'edit':
        showEditSessionModal(session);
        break;
      case 'move':
        showMoveItemModal('session', session.id);
        break;
      case 'delete':
        if (confirm(`Delete session "${session.name}"?`)) {
          invoke('delete_session', { sessionId: session.id, password: state.masterPassword || null }).then(() => loadSavedSessions());
        }
        break;
    }
  } else if (state.contextMenuType === 'command' && state.contextMenuTargetCommandId) {
    const command = state.quickCommands.find(c => c.id === state.contextMenuTargetCommandId);
    if (!command) return;

    switch (action) {
      case 'run':
        executeQuickCommand(command);
        break;
      case 'edit':
        // showEditCommandModal(command); // TODO: Implement if missing
        break;
      case 'move':
        showMoveItemModal('command', command.id);
        break;
      case 'copy':
        // Copy logic
        const newCmd = { ...command, id: crypto.randomUUID(), name: command.name + ' (Copy)' };
        invoke('save_quick_command', { command: newCmd }).then(() => loadQuickCommands());
        break;
      case 'delete':
        if (confirm(`Delete command "${command.name}"?`)) {
          invoke('delete_quick_command', { commandId: command.id }).then(() => loadQuickCommands());
        }
        break;
    }
  } else if (state.contextMenuType === 'folder' && state.contextMenuTargetFolder) {
    if (action === 'delete-folder') {
      deleteFolder(state.contextMenuTargetFolder, state.folderContext); // folderContext is set in showFolderContextMenu
    } else if (action === 'new-subfolder') {
      showAddFolderModal(state.folderContext, state.contextMenuTargetFolder);
    }
  }

  hideContextMenu();
}

// ============================================
// Add/Edit Session Modal
// ============================================

function showAddSessionModal(): void {
  state.editingSessionId = null;
  elements.sessionModalTitle.textContent = 'Add Session';
  elements.sessionName.value = '';
  elements.sessionHost.value = '';
  elements.sessionPort.value = '22';
  elements.sessionUsername.value = '';
  elements.sessionPassword.value = '';
  elements.sessionLogDirectory.value = '';
  elements.sessionUseGlobal.checked = false;
  
  const authSelect = document.getElementById('session-auth-method') as HTMLSelectElement | null;
  if (authSelect) authSelect.value = 'password';
  const pwdGroup = document.getElementById('session-password-group');
  const keyGroup = document.getElementById('session-key-group');
  if (pwdGroup) pwdGroup.style.display = 'block';
  if (keyGroup) keyGroup.style.display = 'none';
  const passphraseInput = document.getElementById('session-key-passphrase') as HTMLInputElement | null;
  if (passphraseInput) passphraseInput.value = '';

  populateSshKeysDropdowns();

  document.getElementById('session-username-group')!.style.display = 'block';
  populateFolderSelect(elements.sessionFolder, 'session');
  elements.addSessionModal.classList.add('show');
  elements.sessionName.focus();
}

async function showEditSessionModal(session: SavedSession): Promise<void> {
  state.editingSessionId = session.id;
  elements.sessionModalTitle.textContent = 'Edit Session';
  elements.sessionName.value = session.name || '';
  elements.sessionHost.value = session.host;
  elements.sessionPort.value = session.port.toString();
  elements.sessionUsername.value = session.username || '';
  elements.sessionPassword.value = ''; // Clear initially
  elements.sessionLogDirectory.value = session.log_directory || '';
  elements.sessionUseGlobal.checked = session.use_global_credentials || false;

  await populateSshKeysDropdowns();

  const authSelect = document.getElementById('session-auth-method') as HTMLSelectElement | null;
  const authMethod = session.auth_method || 'password';
  if (authSelect) authSelect.value = authMethod;

  const pwdGroup = document.getElementById('session-password-group');
  const keyGroup = document.getElementById('session-key-group');
  const keySelect = document.getElementById('session-key-select') as HTMLSelectElement | null;
  const passphraseInput = document.getElementById('session-key-passphrase') as HTMLInputElement | null;

  if (passphraseInput) passphraseInput.value = '';

  if (session.use_global_credentials) {
    document.getElementById('session-username-group')!.style.display = 'none';
    if (pwdGroup) pwdGroup.style.display = 'none';
    if (keyGroup) keyGroup.style.display = 'none';
  } else {
    document.getElementById('session-username-group')!.style.display = 'block';
    if (authMethod === 'publickey') {
      if (pwdGroup) pwdGroup.style.display = 'none';
      if (keyGroup) keyGroup.style.display = 'block';
      if (keySelect && session.private_key_name) {
        keySelect.value = session.private_key_name;
      }
    } else {
      if (pwdGroup) pwdGroup.style.display = 'block';
      if (keyGroup) keyGroup.style.display = 'none';
    }
  }

  // Try to load password / passphrase securely
  try {
    const password = await invoke<string | null>('get_session_password', { sessionId: session.id });
    if (password) {
      if (authMethod === 'publickey') {
        if (passphraseInput) passphraseInput.value = password;
      } else {
        elements.sessionPassword.value = password;
      }
    }
  } catch (e) {
    console.error('Failed to load session password:', e);
  }

  populateFolderSelect(elements.sessionFolder, 'session', session.folder);
  elements.addSessionModal.classList.add('show');
  elements.sessionName.focus();
}

async function saveSessionModal(): Promise<void> {
  const name = elements.sessionName.value.trim();
  const host = elements.sessionHost.value.trim();
  const port = parseInt(elements.sessionPort.value) || 22;
  const folder = elements.sessionFolder.value.trim() || undefined;
  const log_directory = elements.sessionLogDirectory.value.trim() || undefined;

  if (!host) {
    elements.sessionHost.focus();
    return;
  }

  const authSelect = document.getElementById('session-auth-method') as HTMLSelectElement | null;
  const authMethod = authSelect ? authSelect.value : 'password';
  const keySelect = document.getElementById('session-key-select') as HTMLSelectElement | null;
  const privateKeyName = authMethod === 'publickey' && keySelect ? (keySelect.value || undefined) : undefined;

  const session: SavedSession = {
    id: state.editingSessionId || crypto.randomUUID(),
    name: name || host,
    host,
    port,
    folder,
    username: elements.sessionUsername.value.trim() || undefined,
    log_directory,
    use_global_credentials: elements.sessionUseGlobal.checked,
    auth_method: authMethod,
    private_key_name: privateKeyName,
  };

  // Track the folder
  if (folder && !state.sessionFolders.includes(folder)) {
    state.sessionFolders.push(folder);
  }

  try {
    const sessionId = session.id;
    await invoke('save_session', { session, password: state.masterPassword || null });

    // Save or clear password / passphrase
    if (authMethod === 'publickey') {
      const passphraseInput = document.getElementById('session-key-passphrase') as HTMLInputElement | null;
      const passphrase = passphraseInput ? passphraseInput.value : '';
      if (passphrase) {
        await invoke('save_session_password', { sessionId, password: passphrase });
      } else if (state.editingSessionId) {
        try {
          await invoke('delete_session_password', { sessionId });
        } catch (_) {}
      }
    } else {
      const password = elements.sessionPassword.value;
      if (password) {
        await invoke('save_session_password', { sessionId, password });
      } else if (state.editingSessionId) {
        try {
          await invoke('delete_session_password', { sessionId });
        } catch (_) {}
      }
    }

    await loadSavedSessions();
    closeModal(elements.addSessionModal);
  } catch (error) {
    console.error('Failed to save session:', error);
  }
}

// ============================================
// Add/Edit Command Modal
// ============================================

function showAddCommandModal(): void {
  state.editingCommandId = null;
  elements.commandModalTitle.textContent = 'Add Command';
  elements.commandName.value = '';
  elements.commandText.value = '';
  elements.commandDescription.value = '';
  elements.commandExample.value = '';
  populateFolderSelect(elements.commandFolder, 'command');
  elements.commandColor.value = 'cyan';
  elements.addCommandModal.classList.add('show');
  elements.commandName.focus();
}

async function saveCommandModal(): Promise<void> {
  const name = elements.commandName.value.trim();
  const command = elements.commandText.value.trim();
  const folder = elements.commandFolder.value.trim() || 'General';
  const color = elements.commandColor.value;
  const description = elements.commandDescription.value.trim();
  const example = elements.commandExample.value.trim();

  if (!name || !command) {
    if (!name) elements.commandName.focus();
    else elements.commandText.focus();
    return;
  }

  const quickCommand: QuickCommand = {
    id: state.editingCommandId || crypto.randomUUID(),
    name,
    command,
    folder,
    icon: color,
    description: description || undefined,
    example: example || undefined,
  };

  // Track the folder
  if (!state.commandFolders.includes(folder)) {
    state.commandFolders.push(folder);
  }

  try {
    await invoke('save_quick_command', { command: quickCommand });
    await loadQuickCommands(); // Reloads and renders
    closeModal(elements.addCommandModal);
  } catch (error) {
    console.error('Failed to save command:', error);
    alert('Failed to save command: ' + error);
  }
}

// ============================================
// Add Folder Modal
// ============================================

function showAddFolderModal(context: 'session' | 'command', parentPath?: string): void {
  state.folderContext = context;
  elements.folderModalTitle.textContent = context === 'session' ? 'Add Session Folder' : 'Add Command Folder';
  elements.folderName.value = parentPath ? `${parentPath}/` : '';
  elements.addFolderModal.classList.add('show');
  elements.folderName.focus();
}

function saveFolderModal(): void {
  const name = elements.folderName.value.trim();

  if (!name) {
    elements.folderName.focus();
    return;
  }

  if (state.folderContext === 'session') {
    if (!state.sessionFolders.includes(name)) {
      state.sessionFolders.push(name);
      renderSessionsTree();
    }
  } else {
    if (!state.commandFolders.includes(name)) {
      state.commandFolders.push(name);
      renderCommandTree();
    }
  }

  closeModal(elements.addFolderModal);
}

// ============================================
// Move Session Modal
// ============================================

function showMoveItemModal(type: 'session' | 'command', id: string): void {
  state.moveContext = type;
  elements.moveFolderInput.value = '';

  if (type === 'session') {
    state.moveSessionId = id;
    const session = state.savedSessions.find(s => s.id === id);
    if (session && session.folder) elements.moveFolderInput.value = session.folder;

    elements.moveFolderList.innerHTML = state.sessionFolders.map(folder =>
      `<option value="${escapeHtml(folder)}">`
    ).join('');
  } else {
    state.moveCommandId = id;
    const command = state.quickCommands.find(c => c.id === id);
    if (command && command.folder) elements.moveFolderInput.value = command.folder;

    elements.moveFolderList.innerHTML = state.commandFolders.map(folder =>
      `<option value="${escapeHtml(folder)}">`
    ).join('');
  }

  elements.moveSessionModal.classList.add('show');
  elements.moveFolderInput.focus();
}

async function saveMoveItemModal(): Promise<void> {
  const folder = elements.moveFolderInput.value.trim();

  if (!folder) {
    elements.moveFolderInput.focus();
    return;
  }

  if (state.moveContext === 'session') {
    if (!state.moveSessionId) return;
    const session = state.savedSessions.find(s => s.id === state.moveSessionId);
    if (!session) return;
    session.folder = folder;
    if (!state.sessionFolders.includes(folder)) state.sessionFolders.push(folder);

    try {
      await invoke('save_session', { session });
      await loadSavedSessions();
      closeModal(elements.moveSessionModal);
    } catch (error) {
      console.error('Failed to move session:', error);
    }
  } else {
    // Command move
    if (!state.moveCommandId) return;
    const command = state.quickCommands.find(c => c.id === state.moveCommandId);
    if (!command) return;
    command.folder = folder;
    if (!state.commandFolders.includes(folder)) state.commandFolders.push(folder);

    try {
      await invoke('save_quick_command', { command });
      await loadQuickCommands();
      closeModal(elements.moveSessionModal);
    } catch (error) {
      console.error('Failed to move command:', error);
    }
  }
}



// ============================================
// AI Assistant
// ============================================
// Show/hide the privacy banner based on the current AI provider.
// Cloud providers (Gemini, Grok) may train on submitted data.


async function showCommandHelp(): Promise<void> {
  const terminalContent = getTerminalContent();

  // No active terminal session — show a helpful starter prompt
  if (!terminalContent) {
    state.aiMessages.push({
      role: 'assistant',
      content: 'Connect to a session and I can read your terminal to suggest commands.\n\nOr just ask me anything, for example:\n- "How do I show interfaces on Cisco?"\n- "Show ARP table for VLAN 100"\n- "Check routing table on Juniper"\n- "What is the Juniper equivalent of `show ip bgp sum`?"'
    });
    renderAiMessages();
    return;
  }

  // Single direct prompt — tell AI the output is already here, read it, don't ask user for more
  const prompt = `Here is the current terminal output from my SSH session. The device output is already captured below — do NOT ask me to type anything or show you more output, it is all here.

\`\`\`
${terminalContent}
\`\`\`

Based ONLY on what you can see above:
- What device type and CLI mode am I in?
- If the device has already shown a tab-completion list or help output (from Tab or ?), list and explain each option that the device showed.
- If there is an error message (like % Invalid input or % Ambiguous command), diagnose it and give me the corrected command for THIS specific device and version.
- What is the single best command I should type next, and why?
- If this device version uses different subcommands than you'd normally expect, use what the device actually shows — not your general knowledge.`;

  await sendAiMessage(prompt);
}

// ============================================
// AI Settings Modal
// ============================================

function toggleApiKeyVisibility(): void {
  const input = elements.aiGeminiKey;
  input.type = input.type === 'password' ? 'text' : 'password';
}

async function testGeminiConnection(): Promise<void> {
  const btn = elements.aiTestGemini;
  const errorDiv = document.getElementById('ai-gemini-error');
  btn.textContent = 'Testing...';
  btn.classList.remove('success', 'error');
  if (errorDiv) errorDiv.textContent = '';

  const apiKey = elements.aiGeminiKey.value.trim();
  if (!apiKey) {
    btn.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg> No API Key';
    btn.classList.add('error');
    return;
  }

  try {
    // Temporarily save and test
    await invoke('save_ai_config', {
      config: {
        gemini_api_key: apiKey,
        grok_api_key: elements.aiGrokKey.value.trim() || null,
        ollama_host: elements.aiOllamaHost.value.trim() || 'http://localhost:11434',
        ollama_model: elements.aiOllamaModel.value.trim() || 'llama3.2',
      }
    });

    await invoke<string>('ai_chat', {
      provider: 'gemini',
      messages: [{ role: 'user', content: 'Hello, respond with just "OK" to confirm connection.' }],
    });

    btn.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"/><polyline points="22 4 12 14.01 9 11.01"/></svg> Connected!';
    btn.classList.add('success');
  } catch (e) {
    const errorMsg = e instanceof Error ? e.message : String(e);
    console.error('Gemini test failed:', errorMsg);
    btn.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg> Failed';
    btn.classList.add('error');
    if (errorDiv) errorDiv.textContent = errorMsg;
  }
}

async function testOllamaConnection(): Promise<void> {
  const btn = elements.aiTestOllama;
  btn.textContent = 'Testing...';
  btn.classList.remove('success', 'error');

  try {
    // Temporarily save and test
    await invoke('save_ai_config', {
      config: {
        gemini_api_key: elements.aiGeminiKey.value.trim() || null,
        grok_api_key: elements.aiGrokKey.value.trim() || null,
        ollama_host: elements.aiOllamaHost.value.trim() || 'http://localhost:11434',
        ollama_model: elements.aiOllamaModel.value.trim() || 'llama3.2',
      }
    });

    await invoke<string>('ai_chat', {
      provider: 'ollama',
      messages: [{ role: 'user', content: 'Hello, respond with just "OK" to confirm connection.' }],
    });

    btn.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"/><polyline points="22 4 12 14.01 9 11.01"/></svg> Connected!';
    btn.classList.add('success');
  } catch (e) {
    btn.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg> Failed';
    btn.classList.add('error');
  }
}

async function testGrokConnection(): Promise<void> {
  const btn = elements.aiTestGrok;
  const errorDiv = document.getElementById('ai-grok-error');
  btn.textContent = 'Testing...';
  btn.classList.remove('success', 'error');
  if (errorDiv) errorDiv.textContent = '';

  const apiKey = elements.aiGrokKey.value.trim();
  if (!apiKey) {
    btn.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg> No API Key';
    btn.classList.add('error');
    return;
  }

  try {
    // Temporarily save and test
    await invoke('save_ai_config', {
      config: {
        gemini_api_key: elements.aiGeminiKey.value.trim() || null,
        grok_api_key: apiKey,
        ollama_host: elements.aiOllamaHost.value.trim() || 'http://localhost:11434',
        ollama_model: elements.aiOllamaModel.value.trim() || 'llama3.2',
      }
    });

    await invoke<string>('ai_chat', {
      provider: 'grok',
      messages: [{ role: 'user', content: 'Hello, respond with just "OK" to confirm connection.' }],
    });

    btn.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"/><polyline points="22 4 12 14.01 9 11.01"/></svg> Connected!';
    btn.classList.add('success');
  } catch (e) {
    const errorMsg = e instanceof Error ? e.message : String(e);
    console.error('Grok test failed:', errorMsg);
    btn.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg> Failed';
    btn.classList.add('error');
    if (errorDiv) errorDiv.textContent = errorMsg;
  }
}

// ============================================
// Password Prompt Utility
// ============================================
let passwordPromptResolve: ((value: string | null) => void) | null = null;

async function promptPassword(title: string, description: string): Promise<string | null> {
  elements.passwordPromptTitle.textContent = title;
  elements.passwordPromptDescription.textContent = description;
  elements.passwordPromptInput.value = '';
  
  showModal(elements.passwordPromptModal);
  elements.passwordPromptInput.focus();

  return new Promise((resolve) => {
    passwordPromptResolve = resolve;
  });
}

function handlePasswordPrompt(result: string | null) {
  closeModal(elements.passwordPromptModal);
  if (passwordPromptResolve) {
    passwordPromptResolve(result);
    passwordPromptResolve = null;
  }
}

elements.passwordPromptConfirm.addEventListener('click', () => {
  handlePasswordPrompt(elements.passwordPromptInput.value);
});
elements.passwordPromptCancel.addEventListener('click', () => handlePasswordPrompt(null));
elements.passwordPromptClose.addEventListener('click', () => handlePasswordPrompt(null));
elements.passwordPromptInput.addEventListener('keydown', (e) => {
  if (e.key === 'Enter') handlePasswordPrompt(elements.passwordPromptInput.value);
  if (e.key === 'Escape') handlePasswordPrompt(null);
});

// ============================================
// Login Modal
// ============================================

async function showLoginModal(session: SavedSession): Promise<void> {
  state.loginSession = session;
  elements.loginModalTitle.textContent = `Connect to ${session.name}`;

  let pass: string | null = null;
  if (session.use_global_credentials) {
    elements.loginUsername.value = currentSettings?.global_username || '';
    elements.loginPassword.value = '';
    try {
      pass = await invoke<string | null>('get_global_password');
      if (pass) elements.loginPassword.value = pass;
    } catch(e) {}
  } else {
    elements.loginUsername.value = session.username || '';
    elements.loginPassword.value = '';
    try {
      pass = await invoke<string | null>('get_session_password', { sessionId: session.id });
      if (pass) {
        elements.loginPassword.value = pass;
      }
    } catch (e) {
      console.error('Failed to load session password:', e);
    }
  }

  const user = elements.loginUsername.value.trim();
  const authMethod = session.auth_method || 'password';
  const privateKeyName = session.private_key_name;

  const loginPwdLabel = document.querySelector('label[for="login-password"]');
  if (loginPwdLabel) {
    if (authMethod === 'publickey') {
      loginPwdLabel.textContent = 'Passphrase:';
      elements.loginPassword.placeholder = 'Key passphrase (optional)';
    } else {
      loginPwdLabel.textContent = 'Password:';
      elements.loginPassword.placeholder = 'password';
    }
  }

  if (user && (authMethod === 'publickey' || pass !== null)) {
    await connect(
      session.host,
      session.port,
      user,
      authMethod === 'password' ? (pass || '') : '',
      false,
      session.log_directory,
      authMethod,
      privateKeyName,
      authMethod === 'publickey' ? (pass || '') : ''
    );
    return;
  }

  elements.loginModal.classList.add('show');

  if (user) {
    elements.loginPassword.focus();
  } else {
    elements.loginUsername.focus();
  }
}

// ============================================
// Event Listeners
// ============================================

function setupEventListeners(): void {
  // Toolbar buttons
  document.getElementById('btn-quick-connect')?.addEventListener('click', () => {
    showQuickConnectModal();
  });

  document.getElementById('btn-sessions')?.addEventListener('click', async () => {
    await loadSavedSessions();
    elements.sessionsModal.classList.add('show');
  });

  document.getElementById('btn-new-tab')?.addEventListener('click', () => {
    createTab();
  });

  document.getElementById('btn-add-tab')?.addEventListener('click', () => {
    createTab();
  });

  // Broadcast button in toolbar — opens broadcast modal scoped to all sessions
  document.getElementById('btn-broadcast')?.addEventListener('click', () => {
    showBroadcastModal();
  });

  // YModem Send button in toolbar
  document.getElementById('btn-ymodem-send')?.addEventListener('click', () => {
    startYmodemSend();
  });

  // Broadcast modal event listeners
  document.getElementById('broadcast-modal-close')?.addEventListener('click', () => {
    const modal = document.getElementById('modal-broadcast');
    modal?.classList.remove('show');
  });

  document.getElementById('btn-broadcast-cancel')?.addEventListener('click', () => {
    const modal = document.getElementById('modal-broadcast');
    modal?.classList.remove('show');
  });

  document.getElementById('broadcast-scope')?.addEventListener('change', updateBroadcastInfo);

  document.getElementById('btn-broadcast-send')?.addEventListener('click', () => {
    const commandInput = document.getElementById('broadcast-command-input') as HTMLTextAreaElement;
    const scopeSelect = document.getElementById('broadcast-scope') as HTMLSelectElement;
    const command = commandInput.value.trim();
    if (!command) return;
    const scopeId = scopeSelect.value;
    broadcastCommand(command, scopeId === 'all' ? undefined : scopeId);
    const modal = document.getElementById('modal-broadcast');
    modal?.classList.remove('show');
  });

  // Keyboard shortcut: Ctrl+B to open broadcast modal
  document.addEventListener('keydown', (e) => {
    if (e.ctrlKey && e.key === 'b') {
      e.preventDefault();
      showBroadcastModal();
    }
  });

  // Quick connect from address bar
  elements.addressInput?.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
      const value = elements.addressInput.value.trim();
      if (value) {
        showQuickConnectModal(value);
      }
    }
  });

  // Sidebar toggle (collapse button in sidebar header)
  document.getElementById('sidebar-toggle')?.addEventListener('click', () => {
    elements.sidebar.classList.add('collapsed');
    // Refit terminal when sidebar toggles
    setTimeout(() => {
      const tab = state.activeTabId ? state.tabs.get(state.activeTabId) : null;
      if (tab) {
        tab.fitAddon.fit();
        updateStatusBar(tab);
      }
    }, 300);
  });

  // Toolbar sidebar toggle button (reopen sidebar)
  document.getElementById('btn-toggle-sidebar')?.addEventListener('click', () => {
    elements.sidebar.classList.toggle('collapsed');
    // Refit terminal when sidebar toggles
    setTimeout(() => {
      const tab = state.activeTabId ? state.tabs.get(state.activeTabId) : null;
      if (tab) {
        tab.fitAddon.fit();
        updateStatusBar(tab);
      }
    }, 300);
  });

  // ============================================
  // AI Panel Event Listeners
  // ============================================

  // Toggle AI panel from toolbar button
  elements.aiToggleBtn?.addEventListener('click', toggleAiPanel);

  // Toggle AI panel from panel header button
  elements.aiPanelToggle?.addEventListener('click', toggleAiPanel);

  // AI Provider selection
  elements.aiProviderSelect?.addEventListener('change', () => {
    state.aiProvider = elements.aiProviderSelect.value as 'gemini' | 'ollama';
    updatePrivacyBanner();
  });

  // Dismiss privacy banner for the session
  document.getElementById('ai-privacy-dismiss')?.addEventListener('click', () => {
    const banner = document.getElementById('ai-privacy-banner');
    if (banner) banner.classList.add('hidden');
  });

  // AI Send button
  elements.aiSendBtn?.addEventListener('click', () => {
    const message = elements.aiInput.value.trim();
    if (message) {
      sendAiMessage(message);
    }
  });

  // AI Input - Enter to send, Shift+Enter for newline
  elements.aiInput?.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      const message = elements.aiInput.value.trim();
      if (message) {
        sendAiMessage(message);
      }
    }
  });

  // AI Quick Actions
  elements.aiAnalyzeBtn?.addEventListener('click', analyzeTerminal);
  elements.aiHelpBtn?.addEventListener('click', showCommandHelp);

  // AI Settings button - open settings modal
  elements.aiSettingsBtn?.addEventListener('click', showAiSettingsModal);

  // AI Settings Modal controls
  elements.aiSettingsClose?.addEventListener('click', hideAiSettingsModal);
  elements.aiSettingsCancel?.addEventListener('click', hideAiSettingsModal);
  elements.aiSettingsSave?.addEventListener('click', saveAiSettings);
  elements.aiToggleKeyVisibility?.addEventListener('click', toggleApiKeyVisibility);
  elements.aiTestGemini?.addEventListener('click', testGeminiConnection);
  elements.aiToggleGrokVisibility?.addEventListener('click', () => {
    elements.aiGrokKey.type = elements.aiGrokKey.type === 'password' ? 'text' : 'password';
  });
  elements.aiTestGrok?.addEventListener('click', testGrokConnection);
  elements.aiTestOllama?.addEventListener('click', testOllamaConnection);

  // Filtering
  document.querySelector('.sidebar-search')?.addEventListener('input', () => {
    renderSessionsTree();
  });

  document.getElementById('command-search')?.addEventListener('input', () => {
    renderCommandTree();
  });

  // Tooltip interaction
  const tooltip = document.getElementById('command-tooltip');
  tooltip?.addEventListener('mouseenter', () => {
    if (state.tooltipHideTimer) {
      window.clearTimeout(state.tooltipHideTimer);
      state.tooltipHideTimer = null;
    }
  });
  tooltip?.addEventListener('mouseleave', () => {
    (window as any).hideCommandTooltip();
  });

  // Sidebar tab switching
  document.querySelectorAll('.sidebar-tab').forEach((tabBtn) => {
    tabBtn.addEventListener('click', () => {
      const panelName = (tabBtn as HTMLElement).dataset.panel;

      // Update tab active states
      document.querySelectorAll('.sidebar-tab').forEach((btn) => {
        btn.classList.toggle('active', (btn as HTMLElement).dataset.panel === panelName);
      });

      // Show/hide panels
      document.querySelectorAll('.sidebar-panel').forEach((panel) => {
        const panelData = (panel as HTMLElement).dataset.panel;
        panel.classList.toggle('hidden', panelData !== panelName);
      });

      // Load sessions when switching to sessions panel
      if (panelName === 'sessions') {
        elements.sidebarTitle.textContent = 'Sessions Manager';
        loadSavedSessions();
      } else {
        elements.sidebarTitle.textContent = 'Command Manager';
      }
    });
  });

  // Quick Connect Modal
  document.getElementById('modal-close')?.addEventListener('click', () => {
    closeModal(elements.quickConnectModal);
  });

  document.getElementById('btn-cancel')?.addEventListener('click', () => {
    closeModal(elements.quickConnectModal);
  });

  document.getElementById('btn-connect-modal')?.addEventListener('click', async () => {
    const host = elements.inputHost.value.trim();
    const port = parseInt(elements.inputPort.value) || 22;
    const username = elements.inputUsername.value.trim();
    const password = elements.inputPassword.value;
    const saveSession = elements.inputSaveSession.checked;

    const authSelect = document.getElementById('input-auth-method') as HTMLSelectElement | null;
    const authMethod = authSelect ? authSelect.value : 'password';
    const keySelect = document.getElementById('input-key-select') as HTMLSelectElement | null;
    const privateKeyName = keySelect ? keySelect.value : undefined;
    const passphraseInput = document.getElementById('input-key-passphrase') as HTMLInputElement | null;
    const privateKeyPassphrase = passphraseInput ? passphraseInput.value : undefined;

    if (!host || !username) {
      alert('Please enter host and username');
      return;
    }

    closeModal(elements.quickConnectModal);
    await connect(host, port, username, password, saveSession, undefined, authMethod, privateKeyName, privateKeyPassphrase);
  });

  // Toggle quick connect auth method fields
  document.getElementById('input-auth-method')?.addEventListener('change', (e) => {
    const method = (e.target as HTMLSelectElement).value;
    const pwdGroup = document.getElementById('quick-password-group');
    const keyGroup = document.getElementById('quick-key-group');
    if (pwdGroup) pwdGroup.style.display = method === 'password' ? 'block' : 'none';
    if (keyGroup) keyGroup.style.display = method === 'publickey' ? 'block' : 'none';
  });

  // Toggle add/edit session auth method fields
  document.getElementById('session-auth-method')?.addEventListener('change', (e) => {
    const method = (e.target as HTMLSelectElement).value;
    const pwdGroup = document.getElementById('session-password-group');
    const keyGroup = document.getElementById('session-key-group');
    if (pwdGroup) pwdGroup.style.display = method === 'password' ? 'block' : 'none';
    if (keyGroup) keyGroup.style.display = method === 'publickey' ? 'block' : 'none';
  });

  // Sessions Modal
  document.getElementById('sessions-modal-close')?.addEventListener('click', () => {
    closeModal(elements.sessionsModal);
  });

  document.getElementById('btn-sessions-close')?.addEventListener('click', () => {
    closeModal(elements.sessionsModal);
  });

  // Add Session button (in sidebar)
  document.getElementById('btn-add-session')?.addEventListener('click', () => {
    showAddSessionModal();
  });

  // Add Session Folder button
  document.getElementById('btn-add-session-folder')?.addEventListener('click', () => {
    showAddFolderModal('session');
  });

  // Add Session Modal
  document.getElementById('session-modal-close')?.addEventListener('click', () => {
    closeModal(elements.addSessionModal);
  });

  document.getElementById('btn-session-cancel')?.addEventListener('click', () => {
    closeModal(elements.addSessionModal);
  });

  elements.sessionUseGlobal.addEventListener('change', (e) => {
    const checked = (e.target as HTMLInputElement).checked;
    const authSelect = document.getElementById('session-auth-method') as HTMLSelectElement | null;
    const method = authSelect ? authSelect.value : 'password';
    if (checked) {
      document.getElementById('session-username-group')!.style.display = 'none';
      document.getElementById('session-password-group')!.style.display = 'none';
      const keyGroup = document.getElementById('session-key-group');
      if (keyGroup) keyGroup.style.display = 'none';
      const authGroup = document.getElementById('session-auth-method-group');
      if (authGroup) authGroup.style.display = 'none';
    } else {
      document.getElementById('session-username-group')!.style.display = 'block';
      const authGroup = document.getElementById('session-auth-method-group');
      if (authGroup) authGroup.style.display = 'block';
      const pwdGroup = document.getElementById('session-password-group');
      const keyGroup = document.getElementById('session-key-group');
      if (pwdGroup) pwdGroup.style.display = method === 'password' ? 'block' : 'none';
      if (keyGroup) keyGroup.style.display = method === 'publickey' ? 'block' : 'none';
    }
  });

  document.getElementById('btn-session-save')?.addEventListener('click', () => {
    saveSessionModal();
  });

  // Add Command button (in sidebar)
  document.getElementById('btn-add-command')?.addEventListener('click', () => {
    showAddCommandModal();
  });

  // Add Command Modal
  document.getElementById('command-modal-close')?.addEventListener('click', () => {
    closeModal(elements.addCommandModal);
  });

  document.getElementById('btn-command-cancel')?.addEventListener('click', () => {
    closeModal(elements.addCommandModal);
  });

  document.getElementById('btn-command-save')?.addEventListener('click', () => {
    saveCommandModal();
  });

  // Add Command Folder button (in sidebar - id is btn-add-folder)
  document.getElementById('btn-add-folder')?.addEventListener('click', () => {
    showAddFolderModal('command');
  });

  // Add Folder Modal
  document.getElementById('folder-modal-close')?.addEventListener('click', () => {
    closeModal(elements.addFolderModal);
  });

  document.getElementById('btn-folder-cancel')?.addEventListener('click', () => {
    closeModal(elements.addFolderModal);
  });

  document.getElementById('btn-folder-save')?.addEventListener('click', () => {
    saveFolderModal();
  });

  // Move Modal
  document.getElementById('move-modal-close')?.addEventListener('click', () => {
    closeModal(elements.moveSessionModal);
  });

  document.getElementById('btn-move-cancel')?.addEventListener('click', () => {
    closeModal(elements.moveSessionModal);
  });

  document.getElementById('btn-move-save')?.addEventListener('click', () => {
    saveMoveItemModal();
  });

  // Context menu items
  elements.contextMenu?.querySelectorAll('.context-menu-item').forEach((item) => {
    item.addEventListener('click', () => {
      const action = (item as HTMLElement).dataset.action;
      if (action) {
        handleContextMenuAction(action);
      }
    });
  });

  // Hide context menu on click outside
  document.addEventListener('click', (e) => {
    if (!elements.contextMenu?.contains(e.target as Node)) {
      hideContextMenu();
    }
    if (!elements.tabContextMenu?.contains(e.target as Node)) {
      hideTabContextMenu();
    }
  });

  // Tab Context Menu Items
  elements.tabContextMenu?.querySelectorAll('.context-menu-item').forEach((item) => {
    item.addEventListener('click', () => {
      const action = (item as HTMLElement).dataset.action;
      if (action) {
        handleTabContextMenuAction(action);
      }
    });
  });

  // Close modals on overlay click
  [
    elements.quickConnectModal,
    elements.sessionsModal,
    elements.addSessionModal,
    elements.addCommandModal,
    elements.addFolderModal,
    elements.moveSessionModal
  ].forEach((modal) => {
    modal?.addEventListener('click', (e) => {
      if (e.target === modal) {
        closeModal(modal);
      }
    });
  });

  // Window resize handler
  window.addEventListener('resize', () => {
    const tab = state.activeTabId ? state.tabs.get(state.activeTabId) : null;
    if (tab) {
      tab.fitAddon.fit();
      updateStatusBar(tab);
    }
  });

  // Keyboard shortcuts
  document.addEventListener('keydown', (e) => {
    // Alt+R - Focus address bar
    if (e.altKey && e.key === 'r') {
      e.preventDefault();
      elements.addressInput.focus();
    }

    // Ctrl+T - New tab
    if (e.ctrlKey && e.key === 't') {
      e.preventDefault();
      createTab();
    }

    // Ctrl+W - Close tab
    if (e.ctrlKey && e.key === 'w') {
      e.preventDefault();
      if (state.activeTabId) {
        closeTab(state.activeTabId);
      }
    }

    // Escape - Close modals and context menu
    if (e.key === 'Escape') {
      closeModal(elements.quickConnectModal);
      closeModal(elements.sessionsModal);
      closeModal(elements.addSessionModal);
      closeModal(elements.addCommandModal);
      closeModal(elements.addFolderModal);
      closeModal(elements.addFolderModal);
      closeModal(elements.moveSessionModal);
      closeModal(elements.loginModal);
      hideContextMenu();
      hideTabContextMenu();
    }
  });
  // Login Modal Events
  document.getElementById('login-modal-close')?.addEventListener('click', () => {
    closeModal(elements.loginModal);
  });

  document.getElementById('btn-login-cancel')?.addEventListener('click', () => {
    closeModal(elements.loginModal);
  });

  document.getElementById('btn-login-connect')?.addEventListener('click', async () => {
    if (!state.loginSession) return;
    const username = elements.loginUsername.value.trim();
    const password = elements.loginPassword.value;

    if (!username) {
      alert('Username is required');
      return;
    }

    closeModal(elements.loginModal);
    
    const authMethod = state.loginSession.auth_method || 'password';
    const privateKeyName = state.loginSession.private_key_name;

    await connect(
      state.loginSession.host,
      state.loginSession.port,
      username,
      authMethod === 'password' ? password : '',
      false,
      state.loginSession.log_directory,
      authMethod,
      privateKeyName,
      authMethod === 'publickey' ? password : ''
    );
  });

  // Login input enter key
  elements.loginPassword.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') document.getElementById('btn-login-connect')?.click();
  });

  // Hide tab context menu on click outside
  document.addEventListener('click', (e) => {
    if (!elements.tabContextMenu?.contains(e.target as Node)) {
      hideTabContextMenu();
    }
  });

  // Tab Context Menu Items
  elements.tabContextMenu?.querySelectorAll('.context-menu-item').forEach((item) => {
    item.addEventListener('click', () => {
      const action = (item as HTMLElement).dataset.action;
      if (action) {
        handleTabContextMenuAction(action);
      }
    });
  });
}

// ============================================
// Utility Functions
// ============================================
// Helper to populate folder select
function populateFolderSelect(select: HTMLSelectElement, type: 'session' | 'command', selectedValue?: string): void {
  const foldersList = type === 'session' ? state.sessionFolders : state.commandFolders;
  // Use buildFolderTree to get structure (pass empty items to just get folder structure)
  const rootMap = buildFolderTree(foldersList, []);

  let html = '<option value="">(No Folder)</option>';

  const traverse = (nodes: Map<string, FolderNode>, depth: number) => {
    const sortedKeys = Array.from(nodes.keys()).sort();
    sortedKeys.forEach(key => {
      const node = nodes.get(key)!;
      // Indent using non-breaking spaces for visual hierarchy
      const indent = '&nbsp;&nbsp;'.repeat(depth * 2); // Double indent for visibility
      const isSelected = node.path === selectedValue ? ' selected' : '';
      html += `<option value="${escapeHtml(node.path)}"${isSelected}>${indent}${escapeHtml(node.name)}</option>`;

      traverse(node.children, depth + 1);
    });
  };

  traverse(rootMap, 0);
  select.innerHTML = html;
}

// ============================================
// Initialize
// ============================================

// Settings interface
interface AppSettings {
  font_family: string;
  font_size: number;
  cursor_style: string;
  cursor_blink: boolean;
  scrollback_lines: number;
  theme: string;
  color_scheme: string;
  default_port: number;
  connection_timeout: number;
  keepalive_interval: number;
  global_username: string | null;
  confirm_on_close: boolean;
  save_window_size: boolean;
  restore_sessions: boolean;
  download_directory: string | null;
  log_directory: string | null;
  // Logging settings
  logging_enabled: boolean;
  log_format: string;
  log_timestamps: boolean;
  debug_mode: boolean;
}

interface ImportPayload {
  sessions: SavedSession[];
  commands: QuickCommand[];
}

// Current app settings
let currentSettings: AppSettings | null = null;
let pendingImportSessions: SavedSession[] = [];
let pendingImportCommands: QuickCommand[] = [];

// Cache for export counts to avoid DOM scraping
let exportTotalSessions = 0;
let exportTotalCommands = 0;

// Conflict Resolution State
let sessionConflictQueue: SavedSession[] = [];
let commandConflictQueue: QuickCommand[] = [];
let bulkAction: 'update' | 'skip' | 'keepBoth' | null = null;
let currentConflictCallback: ((choice: 'update' | 'skip' | 'keepBoth') => void) | null = null;

// Load application settings
async function loadAppSettings(): Promise<void> {
  try {
    currentSettings = await invoke<AppSettings>('get_settings');
  } catch (e) {
    console.error('Failed to load settings:', e);
  }
}

// Load application version for About modal
async function loadAppVersion(): Promise<void> {
  try {
    const { getVersion } = await import('@tauri-apps/api/app');
    const version = await getVersion();
    const versionEl = document.getElementById('about-version');
    if (versionEl) {
      versionEl.textContent = `Version ${version}`;
    }

    const checkBtn = document.getElementById('about-update-check');
    if (checkBtn) {
      checkBtn.onclick = checkForUpdatesManual;
    }
  } catch (e) {
    console.error('Failed to load app version:', e);
  }
}


interface VersionCheckResponse {
  update_available: boolean;
  latest_version: string;
  current_version: string;
  url: string;
  error?: string;
}

async function checkForUpdatesManual(): Promise<void> {
  const btn = document.getElementById('about-update-check') as HTMLButtonElement;
  if (btn) {
    btn.disabled = true;
    btn.textContent = 'Checking...';
  }

  try {
    const response = await invoke<VersionCheckResponse>('check_latest_version');

    if (response.error) {
      alert('Update check failed: ' + response.error);
    } else if (response.update_available) {
      const yes = await confirm(
        `Update available: v${response.latest_version}\n` +
        `Current version: v${response.current_version}\n\n` +
        `Do you want to visit the download page?`
      );

      if (yes && response.url) {
        try {
          const { openUrl } = await import('@tauri-apps/plugin-opener');
          await openUrl(response.url);
        } catch (e) {
          // Fallback if plugin fails or not loaded
          window.open(response.url, '_blank');
        }
      }
    } else {
      alert(`You are on the latest version (v${response.current_version}).`);
    }
  } catch (e) {
    console.error('Manual update check failed:', e);
    alert('Update check failed: ' + e);
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.textContent = 'Check for Updates';
    }
  }
}

// Populate settings form from current settings
function populateSettingsForm(): void {
  if (!currentSettings) return;

  elements.settingsFontFamily.value = currentSettings.font_family;
  elements.settingsFontSize.value = currentSettings.font_size.toString();
  elements.settingsScrollback.value = currentSettings.scrollback_lines.toString();
  elements.settingsCursorStyle.value = currentSettings.cursor_style;
  elements.settingsCursorBlink.checked = currentSettings.cursor_blink;
  elements.settingsTheme.value = currentSettings.theme;
  elements.settingsColorScheme.value = currentSettings.color_scheme;
  elements.settingsDefaultPort.value = currentSettings.default_port.toString();
  elements.settingsTimeout.value = currentSettings.connection_timeout.toString();
  elements.settingsKeepalive.value = currentSettings.keepalive_interval.toString();
  elements.settingsConfirmClose.checked = currentSettings.confirm_on_close;
  elements.settingsSaveWindowSize.checked = currentSettings.save_window_size;
  elements.settingsRestoreSessions.checked = currentSettings.restore_sessions;
  elements.settingsDownloadDir.value = currentSettings.download_directory || '';
  
  const globalUser = document.getElementById('settings-global-username') as HTMLInputElement;
  if (globalUser) globalUser.value = currentSettings.global_username || '';
  
  const globalPwd = document.getElementById('settings-global-password') as HTMLInputElement;
  if (globalPwd) {
    globalPwd.value = '';
    invoke<string | null>('get_global_password').then(pwd => {
      if (pwd) globalPwd.value = pwd;
    }).catch(console.error);
  }

  // Logging settings
  const logDirInput = document.getElementById('settings-log-dir') as HTMLInputElement;
  const logFormatInput = document.getElementById('settings-log-format') as HTMLInputElement;
  const logEnabledInput = document.getElementById('settings-logging-enabled') as HTMLInputElement;
  const logTimestampsInput = document.getElementById('settings-log-timestamps') as HTMLInputElement;

  if (logDirInput) logDirInput.value = currentSettings.log_directory || '';
  if (logFormatInput) logFormatInput.value = currentSettings.log_format || '{session}_{date}_{time}.log';
  if (logEnabledInput) logEnabledInput.checked = currentSettings.logging_enabled || false;
  if (logTimestampsInput) logTimestampsInput.checked = currentSettings.log_timestamps ?? true;

  const debugModeInput = document.getElementById('settings-debug-mode') as HTMLInputElement;
  if (debugModeInput) debugModeInput.checked = currentSettings.debug_mode || false;
}

// Save settings from form
async function saveAppSettings(): Promise<void> {
  const logDirInput = document.getElementById('settings-log-dir') as HTMLInputElement;
  const logFormatInput = document.getElementById('settings-log-format') as HTMLInputElement;
  const logEnabledInput = document.getElementById('settings-logging-enabled') as HTMLInputElement;
  const logTimestampsInput = document.getElementById('settings-log-timestamps') as HTMLInputElement;
  const debugModeInput = document.getElementById('settings-debug-mode') as HTMLInputElement;

  const settings: AppSettings = {
    font_family: elements.settingsFontFamily.value,
    font_size: parseInt(elements.settingsFontSize.value) || 14,
    cursor_style: elements.settingsCursorStyle.value,
    cursor_blink: elements.settingsCursorBlink.checked,
    scrollback_lines: parseInt(elements.settingsScrollback.value) || 10000,
    theme: elements.settingsTheme.value,
    color_scheme: elements.settingsColorScheme.value,
    default_port: parseInt(elements.settingsDefaultPort.value) || 22,
    connection_timeout: parseInt(elements.settingsTimeout.value) || 30,
    keepalive_interval: parseInt(elements.settingsKeepalive.value) || 60,
    confirm_on_close: elements.settingsConfirmClose.checked,
    save_window_size: elements.settingsSaveWindowSize.checked,
    restore_sessions: elements.settingsRestoreSessions.checked,
    download_directory: elements.settingsDownloadDir.value || null,
    global_username: (document.getElementById('settings-global-username') as HTMLInputElement)?.value || null,
    log_directory: logDirInput?.value || null,
    logging_enabled: logEnabledInput?.checked || false,
    log_format: logFormatInput?.value || '{session}_{date}_{time}.log',
    log_timestamps: logTimestampsInput?.checked ?? true,
    debug_mode: debugModeInput?.checked || false,
  };

  try {
    await invoke('save_settings', { settings });
    currentSettings = settings;
    
    const globalPwdInput = document.getElementById('settings-global-password') as HTMLInputElement;
    if (globalPwdInput && globalPwdInput.value) {
      await invoke('save_global_password', { password: globalPwdInput.value });
    } else if (globalPwdInput && !globalPwdInput.value) {
      await invoke('delete_global_password');
    }
    closeModal(elements.settingsModal);
  } catch (e) {
    console.error('Failed to save settings:', e);
    alert('Failed to save settings: ' + e);
  }
}

// ============================================
// Host Key Manager
// ============================================

async function refreshHostKeys(): Promise<void> {
  if (!elements.hostKeysList) return;

  elements.hostKeysList.innerHTML = '<div class="host-key-empty">Loading keys...</div>';

  try {
    const keys = await invoke<KnownHostEntry[]>('get_known_hosts');
    renderHostKeys(keys);
  } catch (e) {
    console.error('Failed to load host keys:', e);
    elements.hostKeysList.innerHTML = `<div class="host-key-empty">Failed to load keys: ${e}</div>`;
  }
}

function renderHostKeys(keys: KnownHostEntry[]): void {
  const container = elements.hostKeysList;
  if (!container) return;

  const search = elements.hostKeysSearch?.value.toLowerCase() || '';
  const filtered = keys.filter(k => k.name.toLowerCase().includes(search));

  if (filtered.length === 0) {
    container.innerHTML = '<div class="host-key-empty">No host keys found</div>';
    return;
  }

  container.innerHTML = '';

  filtered.forEach(key => {
    const item = document.createElement('div');
    item.className = 'host-key-item';

    // Truncate fingerprint if too long or use what we have
    const fp = key.fingerprint.length > 50 ? key.fingerprint.substring(0, 50) + '...' : key.fingerprint;

    item.innerHTML = `
      <div class="host-key-info">
        <span class="host-key-name">${escapeHtml(key.name)}</span>
        <span class="host-key-meta">${escapeHtml(key.key_type)} • ${escapeHtml(fp)}</span>
      </div>
      <div class="host-key-actions">
        <button class="btn btn-secondary btn-sm btn-delete-key" title="Delete Key">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14">
            <polyline points="3 6 5 6 21 6"></polyline>
            <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path>
          </svg>
        </button>
      </div>
    `;

    // Bind delete event
    const delBtn = item.querySelector('.btn-delete-key');
    delBtn?.addEventListener('click', () => deleteHostKey(key.name));

    container.appendChild(item);
  });
}

async function deleteHostKey(host: string): Promise<void> {
  if (!confirm(`Are you sure you want to delete the host key for "${host}"?`)) return;

  try {
    await invoke('delete_known_host', { host });
    await refreshHostKeys(); // Reload list
  } catch (e) {
    alert('Failed to delete host key: ' + e);
  }
}

interface KeyInfo {
  name: string;
  algorithm: string;
  public_key: string;
  fingerprint: string;
}

async function populateSshKeysDropdowns(): Promise<void> {
  const inputSelect = document.getElementById('input-key-select') as HTMLSelectElement | null;
  const sessionSelect = document.getElementById('session-key-select') as HTMLSelectElement | null;

  try {
    const keys = await invoke<KeyInfo[]>('list_keys');
    const optionsHtml = keys.length === 0
      ? '<option value="">No keys found. Generate one in Settings.</option>'
      : keys.map(k => `<option value="${escapeHtml(k.name)}">${escapeHtml(k.name)} (${escapeHtml(k.algorithm)})</option>`).join('');

    if (inputSelect) {
      inputSelect.innerHTML = optionsHtml;
    }
    if (sessionSelect) {
      sessionSelect.innerHTML = optionsHtml;
    }
  } catch (e) {
    console.error('Failed to list keys for dropdowns:', e);
    const errHtml = '<option value="">Error loading keys</option>';
    if (inputSelect) inputSelect.innerHTML = errHtml;
    if (sessionSelect) sessionSelect.innerHTML = errHtml;
  }
}

async function refreshSshKeys(): Promise<void> {
  const container = document.getElementById('ssh-keys-list');
  if (!container) return;

  container.innerHTML = '<div class="host-key-empty">Loading keys...</div>';

  try {
    const keys = await invoke<KeyInfo[]>('list_keys');
    if (keys.length === 0) {
      container.innerHTML = '<div class="host-key-empty">No SSH keys found. Generate one to get started!</div>';
      return;
    }

    container.innerHTML = '';
    keys.forEach(key => {
      const item = document.createElement('div');
      item.className = 'host-key-item';
      const fp = key.fingerprint.length > 50 ? key.fingerprint.substring(0, 50) + '...' : key.fingerprint;

      item.innerHTML = `
        <div class="host-key-info">
          <span class="host-key-name">${escapeHtml(key.name)}</span>
          <span class="host-key-meta">${escapeHtml(key.algorithm)} • ${escapeHtml(fp)}</span>
        </div>
        <div class="host-key-actions">
          <button class="btn btn-secondary btn-sm btn-delete-ssh-key" title="Delete Key">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14">
              <polyline points="3 6 5 6 21 6"></polyline>
              <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path>
            </svg>
          </button>
        </div>
      `;

      const delBtn = item.querySelector('.btn-delete-ssh-key');
      delBtn?.addEventListener('click', async () => {
        if (!confirm(`Are you sure you want to delete SSH key "${key.name}"? This cannot be undone.`)) return;
        try {
          await invoke('delete_key', { name: key.name });
          await refreshSshKeys();
          await populateSshKeysDropdowns();
        } catch (e) {
          alert('Failed to delete key: ' + e);
        }
      });

      container.appendChild(item);
    });
  } catch (e) {
    console.error('Failed to list SSH keys:', e);
    container.innerHTML = `<div class="host-key-empty">Failed to load keys: ${e}</div>`;
  }
}

// Setup settings modal event listeners
function setupSettingsListeners(): void {
  // Settings cog button opens modal
  document.getElementById('btn-settings')?.addEventListener('click', () => {
    populateSettingsForm();
    showModal(elements.settingsModal);
  });

  // Settings tabs
  document.querySelectorAll('.settings-tab').forEach(tab => {
    tab.addEventListener('click', () => {
      const tabId = (tab as HTMLElement).dataset.tab;
      if (!tabId) return;

      // Update active tab
      document.querySelectorAll('.settings-tab').forEach(t => t.classList.remove('active'));
      tab.classList.add('active');

      // Update active panel
      document.querySelectorAll('.settings-panel').forEach(p => p.classList.remove('active'));
      document.getElementById(`settings-${tabId}`)?.classList.add('active');

      // Reset global password visibility when leaving the connection tab
      const globalPwdInput = document.getElementById('settings-global-password') as HTMLInputElement | null;
      if (globalPwdInput && globalPwdInput.type === 'text') {
        globalPwdInput.type = 'password';
        const btn = globalPwdInput.parentElement?.querySelector('.btn-eye') as HTMLElement | null;
        if (btn) {
          (btn.querySelector('.eye-open') as HTMLElement).style.display = '';
          (btn.querySelector('.eye-closed') as HTMLElement).style.display = 'none';
        }
      }

      // Load host keys if security tab
      if (tabId === 'security') {
        refreshHostKeys();
        refreshSshKeys();
      }
    });
  });

  // Host Keys Search
  elements.hostKeysSearch?.addEventListener('input', () => {
    // We need to re-fetch or store keys locally. For simplicity, just refresh which re-renders.
    // Better: separate fetch from render.
    // Let's implement client-side filter in renderHostKeys properly by storing keys?
    // Current refreshHostKeys re-fetches. Let's just re-trigger refresh for MVP, 
    // or modify refreshHostKeys to use global if performance issue.
    // Actually, refreshHostKeys calls renderHostKeys which uses the input value.
    refreshHostKeys();
  });

  // Refresh Keys Button
  elements.btnRefreshKeys?.addEventListener('click', refreshHostKeys);

  // Close button
  document.getElementById('settings-modal-close')?.addEventListener('click', () => {
    closeModal(elements.settingsModal);
  });

  // Cancel button
  document.getElementById('btn-settings-cancel')?.addEventListener('click', () => {
    closeModal(elements.settingsModal);
  });

  // Save button
  document.getElementById('btn-settings-save')?.addEventListener('click', saveAppSettings);

  // Import/Export buttons in settings
  document.getElementById('btn-settings-import')?.addEventListener('click', () => {
    closeModal(elements.settingsModal);
    resetImportModal();
    showModal(elements.importModal);
  });

  document.getElementById('btn-settings-export')?.addEventListener('click', () => {
    closeModal(elements.settingsModal);
    showExportModal();
  });

  // Browse log directory button
  document.getElementById('btn-browse-log-dir')?.addEventListener('click', async () => {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const selected = await open({ directory: true });
      if (selected && typeof selected === 'string') {
        const logDirInput = document.getElementById('settings-log-dir') as HTMLInputElement;
        if (logDirInput) logDirInput.value = selected;
      }
    } catch (e) {
      console.error('Failed to open directory picker:', e);
    }
  });

  // SSH Key Manager Listeners
  document.getElementById('btn-generate-key-modal')?.addEventListener('click', () => {
    const keyModal = document.getElementById('modal-generate-key');
    if (keyModal) {
      const nameInput = document.getElementById('new-key-name') as HTMLInputElement | null;
      const passInput = document.getElementById('new-key-passphrase') as HTMLInputElement | null;
      if (nameInput) nameInput.value = 'id_ed25519_' + Math.random().toString(36).substr(2, 5);
      if (passInput) passInput.value = '';
      showModal(keyModal);
    }
  });

  document.getElementById('btn-generate-key-cancel')?.addEventListener('click', () => {
    const keyModal = document.getElementById('modal-generate-key');
    if (keyModal) closeModal(keyModal);
  });
  document.getElementById('generate-key-close')?.addEventListener('click', () => {
    const keyModal = document.getElementById('modal-generate-key');
    if (keyModal) closeModal(keyModal);
  });

  document.getElementById('btn-generate-key-confirm')?.addEventListener('click', async () => {
    const nameInput = document.getElementById('new-key-name') as HTMLInputElement | null;
    const algoSelect = document.getElementById('new-key-algo') as HTMLSelectElement | null;
    const passInput = document.getElementById('new-key-passphrase') as HTMLInputElement | null;

    const name = nameInput ? nameInput.value.trim() : '';
    const algorithm = algoSelect ? algoSelect.value : 'ed25519';
    const passphrase = passInput ? passInput.value : '';

    if (!name) {
      alert('Key name is required');
      return;
    }

    try {
      await invoke('generate_key_pair', {
        name,
        algorithm,
        passphrase: passphrase || null
      });

      const keyModal = document.getElementById('modal-generate-key');
      if (keyModal) closeModal(keyModal);

      await refreshSshKeys();
      await populateSshKeysDropdowns();
    } catch (e) {
      alert('Failed to generate keypair: ' + e);
    }
  });

  document.getElementById('btn-refresh-ssh-keys')?.addEventListener('click', () => {
    refreshSshKeys();
  });

  // Master Password Settings Listeners
  document.getElementById('btn-change-master-password')?.addEventListener('click', async () => {
    const isSet = await invoke<boolean>('is_master_password_set');
    const oldGroup = document.getElementById('master-pwd-old-group');
    const titleEl = document.getElementById('master-pwd-modal-title');
    const oldInput = document.getElementById('master-pwd-old') as HTMLInputElement | null;
    const newInput = document.getElementById('master-pwd-new') as HTMLInputElement | null;

    if (oldInput) oldInput.value = '';
    if (newInput) newInput.value = '';

    if (isSet) {
      if (oldGroup) oldGroup.style.display = 'block';
      if (titleEl) titleEl.textContent = 'Change Master Password';
    } else {
      if (oldGroup) oldGroup.style.display = 'none';
      if (titleEl) titleEl.textContent = 'Set Master Password';
    }

    const pwdModal = document.getElementById('modal-change-master-pwd');
    if (pwdModal) showModal(pwdModal);
  });

  document.getElementById('btn-master-pwd-cancel')?.addEventListener('click', () => {
    const pwdModal = document.getElementById('modal-change-master-pwd');
    if (pwdModal) closeModal(pwdModal);
  });
  document.getElementById('master-pwd-close')?.addEventListener('click', () => {
    const pwdModal = document.getElementById('modal-change-master-pwd');
    if (pwdModal) closeModal(pwdModal);
  });

  document.getElementById('btn-master-pwd-save')?.addEventListener('click', async () => {
    const oldInput = document.getElementById('master-pwd-old') as HTMLInputElement | null;
    const newInput = document.getElementById('master-pwd-new') as HTMLInputElement | null;

    const oldPassword = oldInput ? oldInput.value : '';
    const newPassword = newInput ? newInput.value : '';

    try {
      await invoke('change_master_password', {
        oldPassword: oldPassword || null,
        newPassword: newPassword || null
      });

      state.masterPassword = newPassword || null;

      const pwdModal = document.getElementById('modal-change-master-pwd');
      if (pwdModal) closeModal(pwdModal);

      await loadSavedSessions();
      await updateMasterPasswordStatusUI();
    } catch (e) {
      alert('Failed to change master password: ' + e);
    }
  });
}

// Setup import modal event listeners
function setupImportListeners(): void {
  // File Menu: Open Connection
  document.getElementById('menu-file-open')?.addEventListener('click', () => {
    showQuickConnectModal();
  });

    elements.importFormat.addEventListener('change', () => {
    // Hidden - password now handled via pop-up prompt in previewImport
  });

  elements.exportFormat.addEventListener('change', () => {
    const format = elements.exportFormat.value;
    if (format === 'spurx_encrypted') {
      elements.exportMasterPasswordGroup.style.display = 'block';
      elements.exportMasterPasswordGroup.classList.remove('hidden');
    } else {
      elements.exportMasterPasswordGroup.style.display = 'none';
      elements.exportMasterPasswordGroup.classList.add('hidden');
    }
  });

  // File Menu: Import Sessions
  document.getElementById('menu-file-import')?.addEventListener('click', () => {
    resetImportModal();
    showModal(elements.importModal);
  });

  // File Menu: Export Sessions
  document.getElementById('menu-file-export')?.addEventListener('click', () => {
    showExportModal();
  });

  // File Menu: Exit
  document.getElementById('menu-file-exit')?.addEventListener('click', async () => {
    const { exit } = await import('@tauri-apps/plugin-process');
    await exit(0);
  });

  // Edit Menu: Copy
  document.getElementById('menu-edit-copy')?.addEventListener('click', async () => {
    if (!state.activeTabId) return;
    const tab = state.tabs.get(state.activeTabId);
    if (tab?.terminal) {
      const selection = tab.terminal.getSelection();
      if (selection) {
        await navigator.clipboard.writeText(selection);
      }
    }
  });

  // Edit Menu: Paste
  document.getElementById('menu-edit-paste')?.addEventListener('click', async () => {
    if (!state.activeTabId) return;
    const tab = state.tabs.get(state.activeTabId);
    if (tab?.terminal && tab.sessionId) {
      const text = await navigator.clipboard.readText();
      if (text) {
        await invoke('ssh_write', { sessionId: tab.sessionId, data: text });
      }
    }
  });

  // Edit Menu: Select All
  document.getElementById('menu-edit-select-all')?.addEventListener('click', () => {
    if (!state.activeTabId) return;
    const tab = state.tabs.get(state.activeTabId);
    if (tab?.terminal) {
      tab.terminal.selectAll();
    }
  });

  // Edit Menu: Clear Terminal
  document.getElementById('menu-edit-clear')?.addEventListener('click', () => {
    if (!state.activeTabId) return;
    const tab = state.tabs.get(state.activeTabId);
    if (tab?.terminal) {
      tab.terminal.clear();
    }
  });

  // Help Menu: Guide - Opens GitHub
  document.getElementById('menu-help-guide')?.addEventListener('click', async () => {
    const { openUrl } = await import('@tauri-apps/plugin-opener');
    await openUrl('https://github.com/jspurrier/SpurXSecureSSH_rust_tauri#readme');
  });

  // Help Menu: About - Shows About Modal
  document.getElementById('menu-help-about')?.addEventListener('click', () => {
    const aboutModal = document.getElementById('modal-about');
    if (aboutModal) showModal(aboutModal);
  });

  // About modal close handlers
  document.getElementById('about-modal-close')?.addEventListener('click', () => {
    const aboutModal = document.getElementById('modal-about');
    if (aboutModal) closeModal(aboutModal);
  });
  document.getElementById('btn-about-close')?.addEventListener('click', () => {
    const aboutModal = document.getElementById('modal-about');
    if (aboutModal) closeModal(aboutModal);
  });

  // About modal GitHub link
  document.getElementById('about-github')?.addEventListener('click', async (e) => {
    e.preventDefault();
    const { openUrl } = await import('@tauri-apps/plugin-opener');
    await openUrl('https://github.com/jspurrier/SpurXSecureSSH_rust_Public');
  });

  // About modal License link
  document.getElementById('about-license')?.addEventListener('click', async (e) => {
    e.preventDefault();
    const { openUrl } = await import('@tauri-apps/plugin-opener');
    await openUrl('https://github.com/jspurrier/SpurXSecureSSH_rust_tauri/blob/main/LICENSE');
  });

  // F1 keyboard shortcut for Help Guide
  document.addEventListener('keydown', async (e) => {
    if (e.key === 'F1') {
      e.preventDefault();
      const { openUrl } = await import('@tauri-apps/plugin-opener');
      await openUrl('https://github.com/jspurrier/SpurXSecureSSH_rust_Public#readme');
    }
  });

  // Options Menu: Settings
  document.getElementById('menu-options-settings')?.addEventListener('click', () => {
    populateSettingsForm();
    showModal(elements.settingsModal);
  });

  // Transfer Menu: SFTP Browser
  document.getElementById('menu-transfer-sftp')?.addEventListener('click', (e) => {
    e.stopPropagation();
    console.log('SFTP Browser clicked');
    openSftpBrowser();
  });

  // Transfer Menu: YModem Send
  document.getElementById('menu-transfer-ymodem-send')?.addEventListener('click', (e) => {
    e.stopPropagation();
    console.log('YModem Send clicked');
    startYmodemSend();
  });

  // SFTP modal close handlers
  document.getElementById('sftp-modal-close')?.addEventListener('click', closeSftpBrowser);
  document.getElementById('btn-sftp-close')?.addEventListener('click', closeSftpBrowser);

  // SFTP navigation
  document.getElementById('sftp-btn-up')?.addEventListener('click', () => {
    if (state.sftpCurrentPath === '/') return;
    const parts = state.sftpCurrentPath.split('/').filter(p => p);
    parts.pop();
    state.sftpCurrentPath = '/' + parts.join('/');
    loadSftpDirectory(state.sftpCurrentPath);
  });

  document.getElementById('sftp-btn-refresh')?.addEventListener('click', () => {
    loadSftpDirectory(state.sftpCurrentPath);
  });

  // SFTP actions
  document.getElementById('sftp-btn-upload')?.addEventListener('click', uploadSftpFile);
  document.getElementById('sftp-btn-download')?.addEventListener('click', downloadSftpFile);
  document.getElementById('sftp-btn-newfolder')?.addEventListener('click', createSftpFolder);
  document.getElementById('sftp-btn-delete')?.addEventListener('click', deleteSftpFile);

  // Import button in sidebar opens modal
  document.getElementById('btn-import-sessions')?.addEventListener('click', () => {
    resetImportModal();
    showModal(elements.importModal);
  });

  // Also add to File menu
  document.getElementById('menu-file')?.addEventListener('click', () => {
    // Menu dropdown handling could be added here
  });

  // Close button
  document.getElementById('import-modal-close')?.addEventListener('click', () => {
    closeModal(elements.importModal);
  });

  // Cancel button
  document.getElementById('btn-import-cancel')?.addEventListener('click', () => {
    closeModal(elements.importModal);
  });

  // Browse button - using file input fallback
  document.getElementById('btn-import-browse')?.addEventListener('click', async () => {
    const format = elements.importFormat.value;
    let ext = 'reg';
    let name = 'PuTTY Registry';

    if (format === 'securecrt') {
      ext = 'xml';
      name = 'SecureCRT XML';
    } else if (format === 'spurx_encrypted') {
      ext = 'spurx';
      name = 'SpurX Encrypted';
    } else if (format === 'spurx_plaintext') {
      ext = 'json';
      name = 'SpurX JSON';
    }

    // Create a hidden file input for file selection
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = `.${ext}`;

    input.onchange = async () => {
      const file = input.files?.[0];
      if (!file) return;

      elements.importFilePath.value = file.name;

      // Read and preview
      const reader = new FileReader();
      reader.onload = async () => {
        // Create a temporary file path to pass to backend
        try {
          // Use Tauri's file dialog if available
          const { open } = await import('@tauri-apps/plugin-dialog');
          const selected = await open({
            filters: [{
              name: name,
              extensions: [ext]
            }]
          });

          if (selected && typeof selected === 'string') {
            elements.importFilePath.value = selected;
            await previewImport(selected);
          }
        } catch {
          // Fallback: show hint about file path
          elements.importPreview.innerHTML = '<p class="import-hint">Please enter the full file path manually or use native file dialog.</p>';
        }
      };
      reader.readAsArrayBuffer(file);
    };

    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const selected = await open({
        filters: [{
          name: name,
          extensions: [ext]
        }]
      });

      if (selected && typeof selected === 'string') {
        elements.importFilePath.value = selected;
        await previewImport(selected);
      }
    } catch {
      // Fall back to HTML file input
      input.click();
    }
  });

  // Import confirm button - handles duplicate checks and conflict resolution queue
  document.getElementById('btn-import-confirm')?.addEventListener('click', async () => {
    if (pendingImportSessions.length === 0 && pendingImportCommands.length === 0) return;

    // 1. Process Sessions
    const duplicatesS: SavedSession[] = [];
    const nonDuplicatesS: SavedSession[] = [];

    for (const imported of pendingImportSessions) {
      const existing = state.savedSessions.find(s => s.host === imported.host && s.port === imported.port);
      if (existing) duplicatesS.push(imported);
      else nonDuplicatesS.push(imported);
    }

    for (const session of nonDuplicatesS) {
      await invoke('save_session', { session });
    }

    // 2. Process Commands
    const duplicatesC: QuickCommand[] = [];
    const nonDuplicatesC: QuickCommand[] = [];
    const existingCommands = await invoke<QuickCommand[]>('get_quick_commands');

    for (const imported of pendingImportCommands) {
      const existing = existingCommands.find(c => c.name === imported.name);
      if (existing) duplicatesC.push(imported);
      else nonDuplicatesC.push(imported);
    }

    for (const command of nonDuplicatesC) {
      await invoke('save_quick_command', { command });
    }

    // 3. Initiate conflict resolution if needed
    if (duplicatesS.length === 0 && duplicatesC.length === 0) {
      await loadSavedSessions();
      await loadQuickCommands();
      closeModal(elements.importModal);
      alert(`Import successful!`);
      return;
    }

    sessionConflictQueue = duplicatesS;
    commandConflictQueue = duplicatesC;
    bulkAction = null;
    processNextConflict();
  });

  // Function to process the next conflict in the queue
  async function processNextConflict() {
    // Process sessions first, then commands
    if (sessionConflictQueue.length > 0) {
      const session = sessionConflictQueue[0];
      const existing = state.savedSessions.find(s => s.host === session.host && s.port === session.port);

      if (!existing) {
        sessionConflictQueue.shift();
        processNextConflict();
        return;
      }

      const handleSessionChoice = async (choice: 'update' | 'skip' | 'keepBoth') => {
        if (choice === 'update') {
          session.id = existing.id;
          await invoke('save_session', { session });
        } else if (choice === 'keepBoth') {
          session.name = `${session.name} (Imported)`;
          await invoke('save_session', { session });
        }
        
        sessionConflictQueue.shift();
        
        if (elements.conflictApplyAll.checked) {
          bulkAction = choice;
          while (sessionConflictQueue.length > 0) {
            const s = sessionConflictQueue.shift()!;
            const ex = state.savedSessions.find(e => e.host === s.host && e.port === s.port);
            if (ex) {
              if (choice === 'update') { s.id = ex.id; await invoke('save_session', { session: s }); }
              else if (choice === 'keepBoth') { s.name = `${s.name} (Imported)`; await invoke('save_session', { session: s }); }
            }
          }
        }
        processNextConflict();
      };

      if (bulkAction) {
        await handleSessionChoice(bulkAction);
      } else {
        elements.conflictMessage.innerHTML = `Session conflict: A session for <strong id="conflict-host">${session.host}:${session.port}</strong> already exists.`;
        currentConflictCallback = handleSessionChoice;
        showModal(elements.conflictModal);
      }
      return;
    }

    if (commandConflictQueue.length > 0) {
      const command = commandConflictQueue[0];
      const existingCommands = await invoke<QuickCommand[]>('get_quick_commands');
      const existing = existingCommands.find(c => c.name === command.name);

      if (!existing) {
        commandConflictQueue.shift();
        processNextConflict();
        return;
      }

      const handleCommandChoice = async (choice: 'update' | 'skip' | 'keepBoth') => {
        if (choice === 'update') {
          command.id = existing.id;
          await invoke('save_quick_command', { command });
        } else if (choice === 'keepBoth') {
          command.name = `${command.name} (Imported)`;
          await invoke('save_quick_command', { command });
        }
        
        commandConflictQueue.shift();
        
        if (elements.conflictApplyAll.checked) {
          bulkAction = choice;
          while (commandConflictQueue.length > 0) {
            const c = commandConflictQueue.shift()!;
            const ex = existingCommands.find(e => e.name === c.name);
            if (ex) {
              if (choice === 'update') { c.id = ex.id; await invoke('save_quick_command', { command: c }); }
              else if (choice === 'keepBoth') { c.name = `${c.name} (Imported)`; await invoke('save_quick_command', { command: c }); }
            }
          }
        }
        processNextConflict();
      };

      if (bulkAction) {
        await handleCommandChoice(bulkAction);
      } else {
        elements.conflictMessage.innerHTML = `Command conflict: A command named <strong>${command.name}</strong> already exists.`;
        currentConflictCallback = handleCommandChoice;
        showModal(elements.conflictModal);
      }
      return;
    }

    // All done
    await loadSavedSessions();
    await loadQuickCommands();
    closeModal(elements.importModal);
    bulkAction = null;
    alert("Import process completed.");
  }

  // Conflict modal action buttons
  document.querySelectorAll('.conflict-action').forEach(btn => {
    btn.addEventListener('click', () => {
      const action = (btn as HTMLElement).dataset.action as 'update' | 'skip' | 'keepBoth';
      if (currentConflictCallback) {
        closeModal(elements.conflictModal);
        currentConflictCallback(action);
      }
    });
  });

  // Conflict modal close
  document.getElementById('conflict-modal-close')?.addEventListener('click', () => {
    closeModal(elements.conflictModal);
    // Default to skip if closed
    if (currentConflictCallback) currentConflictCallback('skip');
  });

  // Export modal handlers
  document.getElementById('export-modal-close')?.addEventListener('click', () => {
    closeModal(elements.exportModal);
  });

  elements.exportIncludeSessions.addEventListener('change', updateExportSummary);
  elements.exportIncludeCommands.addEventListener('change', updateExportSummary);

  document.getElementById('btn-export-cancel')?.addEventListener('click', () => {
    closeModal(elements.exportModal);
  });

  document.getElementById('btn-export-confirm')?.addEventListener('click', async () => {
    try {
      const { save } = await import('@tauri-apps/plugin-dialog');
      const format = elements.exportFormat.value;
      let ext = format === 'securecrt' ? 'xml' : 'reg';
      if (format === 'spurx_encrypted') ext = 'spurx';
      if (format === 'spurx_plaintext') ext = 'json';
      let name = format === 'securecrt' ? 'SecureCRT XML' : 'PuTTY Registry';
      if (format === 'spurx_encrypted') name = 'SpurX Encrypted';
      if (format === 'spurx_plaintext') name = 'SpurX JSON';

      const filePath = await save({
        filters: [{ name, extensions: [ext] }],
        defaultPath: `spurx_backup.${ext}`
      });

      if (filePath) {
        const includeSessions = elements.exportIncludeSessions.checked;
        const includeCommands = elements.exportIncludeCommands.checked;

        const sessions = includeSessions ? state.savedSessions : [];
        let commands: QuickCommand[] = [];
        if (includeCommands) {
          commands = await invoke<QuickCommand[]>('get_quick_commands');
        }

        await invoke('export_sessions', {
          path: filePath,
          format,
          sessions,
          commands,
          password: format === 'spurx_encrypted' ? (elements.exportMasterPassword.value || null) : null
        });
        closeModal(elements.exportModal);
        
        let message = 'Successfully exported ';
        if (includeSessions && includeCommands) message += `${sessions.length} sessions and ${commands.length} commands!`;
        else if (includeSessions) message += `${sessions.length} sessions!`;
        else if (includeCommands) message += `${commands.length} commands!`;
        else message = "Export completed (no data selected).";
        
        alert(message);
      }
    } catch (e) {
      console.error('Failed to export data:', e);
      alert('Failed to export data: ' + e);
    }
  });
}

// Show export modal
async function showExportModal(): Promise<void> {
  exportTotalSessions = state.savedSessions.length;
  elements.exportSessionCount.textContent = exportTotalSessions.toString();
  
  // Clear sensitive data on open
  elements.exportMasterPassword.value = '';
  
  // Use cached quickCommands if available, otherwise fetch
  if (state.quickCommands.length > 0) {
    exportTotalCommands = state.quickCommands.length;
    elements.exportCommandCount.textContent = exportTotalCommands.toString();
  } else {
    try {
      const commands = await invoke<QuickCommand[]>('get_quick_commands');
      exportTotalCommands = commands.length;
      elements.exportCommandCount.textContent = exportTotalCommands.toString();
    } catch {
      elements.exportCommandCount.textContent = '0';
    }
  }
  
  // Update summary total
  updateExportSummary();
  
  showModal(elements.exportModal);
}

function updateExportSummary(): void {
  let total = 0;
  if (elements.exportIncludeSessions.checked) total += exportTotalSessions;
  if (elements.exportIncludeCommands.checked) total += exportTotalCommands;
  
  if (elements.exportSummaryCount) {
    elements.exportSummaryCount.textContent = total.toString();
  }
}

// Reset import modal
function resetImportModal(): void {
  elements.importFilePath.value = '';
  elements.importMasterPassword.value = '';
  elements.importPreview.innerHTML = '<p class="import-hint">Select a file to preview data</p>';
  pendingImportSessions = [];
  (document.getElementById('btn-import-confirm') as HTMLButtonElement).disabled = true;
}

// Preview sessions from file
async function previewImport(filePath: string): Promise<void> {
  let attempts = 0;
  const format = elements.importFormat.value;

  while (attempts < 3) {
    try {
      const pwd = elements.importMasterPassword.value || null;
      const payload = await invoke<ImportPayload>('import_sessions', { path: filePath, format, password: pwd });
      
      pendingImportSessions = payload.sessions || [];
      pendingImportCommands = payload.commands || [];

      if (pendingImportSessions.length === 0 && pendingImportCommands.length === 0) {
        elements.importPreview.innerHTML = '<p class="import-hint">No sessions or commands found in this file</p>';
        return;
      }

      // Build preview HTML
      let html = '';
      
      // Sessions section
      if (pendingImportSessions.length > 0) {
        html += '<div class="preview-section-title" style="font-size: 11px; font-weight: 600; text-transform: uppercase; color: var(--text-muted); margin-bottom: 8px;">Sessions</div>';
        
        // Performance: Cap rendered items to 50
        const displaySessions = pendingImportSessions.slice(0, 50);
        displaySessions.forEach(session => {
          html += `
            <div class="import-session-item">
              <svg class="import-session-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <rect x="2" y="3" width="20" height="14" rx="2" ry="2"/>
                <line x1="8" y1="21" x2="16" y2="21"/>
                <line x1="12" y1="17" x2="12" y2="21"/>
              </svg>
              <div class="import-session-info">
                <div class="import-session-name">${escapeHtml(session.name)}</div>
                <div class="import-session-host">${escapeHtml(session.host)}:${session.port}</div>
              </div>
              ${session.folder ? `<span class="import-session-folder">${escapeHtml(session.folder)}</span>` : ''}
            </div>
          `;
        });
        
        if (pendingImportSessions.length > 50) {
          html += `<div class="import-more-hint" style="font-size: 11px; color: var(--text-muted); padding: 8px; text-align: center; font-style: italic;">...and ${pendingImportSessions.length - 50} more sessions</div>`;
        }
      }

      // Commands section
      if (pendingImportCommands.length > 0) {
        html += `<div class="preview-section-title" style="font-size: 11px; font-weight: 600; text-transform: uppercase; color: var(--text-muted); margin-top: 16px; margin-bottom: 8px;">Quick Commands</div>`;
        
        // Performance: Cap rendered items to 50
        const displayCommands = pendingImportCommands.slice(0, 50);
        displayCommands.forEach(cmd => {
          html += `
            <div class="import-session-item">
              <svg class="import-session-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M4 17l6-6-6-6M12 19h8"/>
              </svg>
              <div class="import-session-info">
                <div class="import-session-name">${escapeHtml(cmd.name)}</div>
                <div class="import-session-host" style="font-family: monospace;">${escapeHtml(cmd.command.substring(0, 40))}${cmd.command.length > 40 ? '...' : ''}</div>
              </div>
              <span class="import-session-folder">${escapeHtml(cmd.folder)}</span>
            </div>
          `;
        });

        if (pendingImportCommands.length > 50) {
          html += `<div class="import-more-hint" style="font-size: 11px; color: var(--text-muted); padding: 8px; text-align: center; font-style: italic;">...and ${pendingImportCommands.length - 50} more commands</div>`;
        }
      }

      html += `
        <div class="import-stats">
          <span>Found <span class="import-stats-count">${pendingImportSessions.length}</span> sessions & <span class="import-stats-count">${pendingImportCommands.length}</span> commands</span>
          <span>Ready to import</span>
        </div>
      `;

      elements.importPreview.innerHTML = html;
      (document.getElementById('btn-import-confirm') as HTMLButtonElement).disabled = false;
      return; // Success!

    } catch (e) {
      const errorMsg = String(e);
      const isEncrypted = format === 'spurx_encrypted';
      const needsPassword = errorMsg.includes('Password required') || errorMsg.includes('Decryption failed');

      if (isEncrypted && needsPassword) {
        const title = errorMsg.includes('failed') ? "Incorrect Password" : "Master Password Required";
        const description = errorMsg.includes('failed') 
          ? "The password provided was incorrect. Please try again." 
          : "This file is encrypted and requires a password to preview its contents.";
        
        const newPwd = await promptPassword(title, description);
        
        if (newPwd !== null) {
          elements.importMasterPassword.value = newPwd;
          attempts++;
          continue; // Retry with new password
        }
      }
      
      // If we get here, it's a non-password error or user canceled prompt
      elements.importPreview.innerHTML = `<p class="import-hint" style="color: var(--accent-danger);">Error: ${e}</p>`;
      pendingImportSessions = [];
      pendingImportCommands = [];
      break; 
    }
  }
}

// ============================================
// SFTP Browser Functions
// ============================================
// ============================================
// Auto Updater
// ============================================

async function checkGitHubUpdate() {
  try {
    // Fetch package.json from master branch
    const response = await fetch('https://raw.githubusercontent.com/jspurrier/SpurXSecureSSH_rust_tauri/master/package.json');
    if (!response.ok) return;

    const pkg = await response.json();
    const remoteVersion = pkg.version;

    // Get local version
    const { getVersion } = await import('@tauri-apps/api/app');
    const currentVersion = await getVersion();

    console.log(`Checking updates: Local v${currentVersion}, Remote v${remoteVersion}`);

    if (remoteVersion !== currentVersion && compareVersions(remoteVersion, currentVersion) > 0) {
      showUpdateNotification(remoteVersion);
    } else {
      console.log('App is up to date.');
    }
  } catch (e) {
    console.error('Failed to check for updates:', e);
  }
}

function compareVersions(v1: string, v2: string): number {
  const parts1 = v1.split('.').map(Number);
  const parts2 = v2.split('.').map(Number);
  for (let i = 0; i < 3; i++) {
    const p1 = parts1[i] || 0;
    const p2 = parts2[i] || 0;
    if (p1 > p2) return 1;
    if (p1 < p2) return -1;
  }
  return 0;
}

function showUpdateNotification(version: string) {
  const notification = document.getElementById('update-notification');
  const versionSpan = document.getElementById('update-version');
  const restartBtn = document.getElementById('btn-update-restart');
  const closeBtn = document.getElementById('btn-update-close');

  if (notification && versionSpan && restartBtn) {
    versionSpan.textContent = `v${version}`;
    notification.classList.remove('hidden');

    // Update button to "Download"
    restartBtn.textContent = 'Download Update';
    restartBtn.removeAttribute('disabled');

    // Replace element to remove old listeners
    const newBtn = restartBtn.cloneNode(true);
    restartBtn.parentNode?.replaceChild(newBtn, restartBtn);

    newBtn.addEventListener('click', async () => {
      await invoke('plugin:opener|open', { path: 'https://github.com/jspurrier/SpurXSecureSSH_rust_tauri/releases/latest' });
    });

    closeBtn?.addEventListener('click', () => {
      notification.classList.add('hidden');
    });
  }
}

async function init(): Promise<void> {
  setupOsShortcuts();
  setupEventListeners();
  setupSettingsListeners();
  setupImportListeners();
  await loadAppSettings();
  await populateSshKeysDropdowns();
  await loadQuickCommands();
  await loadSavedSessions();
  await loadAppVersion();
  await createTab();

  // Check for updates
  await checkGitHubUpdate();
}

// Detect OS and update keyboard shortcut display
function setupOsShortcuts(): void {
  const isMac = navigator.platform.toLowerCase().includes('mac') ||
    navigator.userAgent.toLowerCase().includes('mac');

  if (isMac) {
    // Update all shortcut labels to show Mac shortcuts
    document.querySelectorAll('.menu-shortcut[data-shortcut-mac]').forEach(el => {
      const macShortcut = (el as HTMLElement).dataset.shortcutMac;
      if (macShortcut) {
        el.textContent = macShortcut;
      }
    });
  }
}

// Wait for DOM to be ready
if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', init);
} else {
  init();
}
