/**
 * SpurX Secure SSH - Main Application
 * Handles terminal management, SSH connections, and UI interactions
 */

import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { WebglAddon } from '@xterm/addon-webgl';
import { check } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import '@xterm/xterm/css/xterm.css';

// Tauri API
declare global {
  interface Window {
    __TAURI__: {
      core: {
        invoke: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
      };
      event: {
        listen: <T>(event: string, handler: (event: { payload: T }) => void) => Promise<() => void>;
      };
    };
  }
}

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// ============================================
// Types
// ============================================

interface ConnectRequest {
  host: string;
  port: number;
  username: string;
  password: string;
}

interface ConnectResponse {
  session_id: string;
  success: boolean;
  error?: string;
}

interface SavedSession {
  id: string;
  name: string;
  host: string;
  port: number;
  folder?: string;
  username?: string;
}

interface QuickCommand {
  id: string;
  name: string;
  command: string;
  folder: string;
  icon?: string;
  description?: string;
  example?: string;
}

interface SftpFileEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
  modified?: number;
  permissions?: number;
}

interface TabInfo {
  id: string;
  sessionId?: string;
  title: string;
  terminal: Terminal;
  fitAddon: FitAddon;
  connected: boolean;
  unlisteners: Array<() => void>;
  connectionParams: ConnectRequest | null;
  container: HTMLDivElement;
}

// ============================================
// State
// ============================================

const tabs: Map<string, TabInfo> = new Map();
let activeTabId: string | null = null;
let savedSessions: SavedSession[] = [];
let quickCommands: QuickCommand[] = [];

// Editing state
let editingSessionId: string | null = null;
let editingCommandId: string | null = null;

let contextMenuTargetSessionId: string | null = null;
let contextMenuTargetCommandId: string | null = null;
let contextMenuTargetFolder: string | null = null;
let contextMenuType: 'session' | 'command' | 'folder' | null = null;
let moveSessionId: string | null = null;
let moveCommandId: string | null = null;
let moveContext: 'session' | 'command' = 'session';
let loginSession: SavedSession | null = null;
let contextMenuTargetTabId: string | null = null;

// Command folders
let commandFolders: string[] = ['General'];
let sessionFolders: string[] = ['General'];
// Track expanded folders for persistence
const expandedSessionFolders: Set<string> = new Set();
const expandedCommandFolders: Set<string> = new Set();
let folderContext: 'session' | 'command' = 'command';
let tooltipHideTimer: number | null = null;

// SFTP state
let sftpSessionId: string | null = null;
let sftpCurrentPath = '/';
let sftpSelectedFile: SftpFileEntry | null = null;
let sftpConnectionParams: ConnectRequest | null = null;

// AI Assistant state
let aiPanelOpen = false;
const aiMessages: Array<{ role: 'user' | 'assistant'; content: string }> = [];
let aiProvider: 'gemini' | 'grok' | 'ollama' = 'gemini';

// AI Config interface matching Rust struct
interface AiConfig {
  gemini_api_key: string | null;
  grok_api_key: string | null;
  ollama_host: string;
  ollama_model: string;
}

interface KnownHostEntry {
  name: string;
  key_type: string;
  fingerprint: string;
}

// ============================================
// DOM Elements
// ============================================

const elements = {
  tabBar: document.getElementById('tab-bar') as HTMLDivElement,
  terminalViewport: document.getElementById('terminal-viewport') as HTMLDivElement,
  sidebar: document.getElementById('sidebar') as HTMLDivElement,
  commandTree: document.getElementById('command-tree') as HTMLDivElement,

  // Sidebar panels
  sessionsPanel: document.getElementById('sessions-panel') as HTMLDivElement,
  commandsPanel: document.getElementById('commands-panel') as HTMLDivElement,
  sessionsTree: document.getElementById('sessions-tree') as HTMLDivElement,

  // Modals
  quickConnectModal: document.getElementById('modal-quick-connect') as HTMLDivElement,
  sessionsModal: document.getElementById('modal-sessions') as HTMLDivElement,
  sessionsList: document.getElementById('sessions-list') as HTMLDivElement,
  addSessionModal: document.getElementById('modal-add-session') as HTMLDivElement,
  addCommandModal: document.getElementById('modal-add-command') as HTMLDivElement,
  moveSessionModal: document.getElementById('modal-move-session') as HTMLDivElement,

  // Context menu
  contextMenu: document.getElementById('context-menu') as HTMLDivElement,
  tabContextMenu: document.getElementById('tab-context-menu') as HTMLDivElement,

  // Form inputs - Quick Connect
  inputHost: document.getElementById('input-host') as HTMLInputElement,
  inputPort: document.getElementById('input-port') as HTMLInputElement,
  inputUsername: document.getElementById('input-username') as HTMLInputElement,
  inputPassword: document.getElementById('input-password') as HTMLInputElement,
  inputSaveSession: document.getElementById('input-save-session') as HTMLInputElement,
  addressInput: document.getElementById('address-input') as HTMLInputElement,

  // Form inputs - Add/Edit Session
  sessionName: document.getElementById('session-name') as HTMLInputElement,
  sessionHost: document.getElementById('session-host') as HTMLInputElement,
  sessionPort: document.getElementById('session-port') as HTMLInputElement,
  sessionFolder: document.getElementById('session-folder') as HTMLSelectElement,
  sessionModalTitle: document.getElementById('session-modal-title') as HTMLHeadingElement,

  // Form inputs - Add/Edit Command
  commandName: document.getElementById('command-name') as HTMLInputElement,
  commandText: document.getElementById('command-text') as HTMLInputElement,
  commandDescription: document.getElementById('command-description') as HTMLInputElement,
  commandExample: document.getElementById('command-example') as HTMLTextAreaElement,
  commandFolder: document.getElementById('command-folder') as HTMLSelectElement,
  commandColor: document.getElementById('command-color') as HTMLSelectElement,
  commandModalTitle: document.getElementById('command-modal-title') as HTMLHeadingElement,

  // Form inputs - Add Folder
  addFolderModal: document.getElementById('modal-add-folder') as HTMLDivElement,
  folderName: document.getElementById('folder-name') as HTMLInputElement,
  folderModalTitle: document.getElementById('folder-modal-title') as HTMLHeadingElement,

  // Form inputs - Move Session
  moveFolderInput: document.getElementById('move-folder-input') as HTMLInputElement,
  moveFolderList: document.getElementById('folder-list') as HTMLDataListElement,

  // Login Modal
  loginModal: document.getElementById('modal-login') as HTMLDivElement,
  loginModalTitle: document.getElementById('login-modal-title') as HTMLHeadingElement,
  loginUsername: document.getElementById('login-username') as HTMLInputElement,
  loginPassword: document.getElementById('login-password') as HTMLInputElement,

  // Session User Input
  sessionUsername: document.getElementById('session-username') as HTMLInputElement,
  sessionPassword: document.getElementById('session-password') as HTMLInputElement,

  // Sidebar Title
  sidebarTitle: document.getElementById('sidebar-title') as HTMLSpanElement,

  // Status bar
  statusConnection: document.getElementById('status-connection') as HTMLSpanElement,
  statusCipher: document.getElementById('status-cipher') as HTMLSpanElement,
  statusSize: document.getElementById('status-size') as HTMLSpanElement,

  // AI Panel
  aiPanel: document.getElementById('ai-panel') as HTMLDivElement,
  aiToggleBtn: document.getElementById('btn-toggle-ai') as HTMLButtonElement,
  aiPanelToggle: document.getElementById('ai-panel-toggle') as HTMLButtonElement,
  aiProviderSelect: document.getElementById('ai-provider-select') as HTMLSelectElement,
  aiSettingsBtn: document.getElementById('ai-settings-btn') as HTMLButtonElement,
  aiMessages: document.getElementById('ai-messages') as HTMLDivElement,
  aiInput: document.getElementById('ai-input') as HTMLTextAreaElement,
  aiSendBtn: document.getElementById('ai-send-btn') as HTMLButtonElement,
  aiAnalyzeBtn: document.getElementById('ai-analyze-btn') as HTMLButtonElement,
  aiHelpBtn: document.getElementById('ai-help-btn') as HTMLButtonElement,

  // AI Settings Modal
  aiSettingsModal: document.getElementById('modal-ai-settings') as HTMLDivElement,
  aiGeminiKey: document.getElementById('ai-gemini-key') as HTMLInputElement,
  aiToggleKeyVisibility: document.getElementById('ai-toggle-key-visibility') as HTMLButtonElement,
  aiTestGemini: document.getElementById('ai-test-gemini') as HTMLButtonElement,
  aiGrokKey: document.getElementById('ai-grok-key') as HTMLInputElement,
  aiToggleGrokVisibility: document.getElementById('ai-toggle-grok-visibility') as HTMLButtonElement,
  aiTestGrok: document.getElementById('ai-test-grok') as HTMLButtonElement,
  aiOllamaHost: document.getElementById('ai-ollama-host') as HTMLInputElement,
  aiOllamaModel: document.getElementById('ai-ollama-model') as HTMLInputElement,
  aiTestOllama: document.getElementById('ai-test-ollama') as HTMLButtonElement,
  aiSettingsSave: document.getElementById('ai-settings-save') as HTMLButtonElement,
  aiSettingsCancel: document.getElementById('ai-settings-cancel') as HTMLButtonElement,
  aiSettingsClose: document.getElementById('ai-settings-close') as HTMLButtonElement,

  // Settings Modal
  settingsModal: document.getElementById('modal-settings') as HTMLDivElement,
  settingsFontFamily: document.getElementById('settings-font-family') as HTMLInputElement,
  settingsFontSize: document.getElementById('settings-font-size') as HTMLInputElement,
  settingsScrollback: document.getElementById('settings-scrollback') as HTMLInputElement,
  settingsCursorStyle: document.getElementById('settings-cursor-style') as HTMLSelectElement,
  settingsCursorBlink: document.getElementById('settings-cursor-blink') as HTMLInputElement,
  settingsTheme: document.getElementById('settings-theme') as HTMLSelectElement,
  settingsColorScheme: document.getElementById('settings-color-scheme') as HTMLSelectElement,
  settingsDefaultPort: document.getElementById('settings-default-port') as HTMLInputElement,
  settingsTimeout: document.getElementById('settings-timeout') as HTMLInputElement,
  settingsKeepalive: document.getElementById('settings-keepalive') as HTMLInputElement,
  settingsConfirmClose: document.getElementById('settings-confirm-close') as HTMLInputElement,
  settingsSaveWindowSize: document.getElementById('settings-save-window-size') as HTMLInputElement,
  settingsRestoreSessions: document.getElementById('settings-restore-sessions') as HTMLInputElement,
  settingsDownloadDir: document.getElementById('settings-download-dir') as HTMLInputElement,

  // Import Sessions Modal
  importModal: document.getElementById('modal-import-sessions') as HTMLDivElement,
  importFormat: document.getElementById('import-format') as HTMLSelectElement,
  importFilePath: document.getElementById('import-file-path') as HTMLInputElement,
  importPreview: document.getElementById('import-preview') as HTMLDivElement,

  // Export Sessions Modal
  exportModal: document.getElementById('modal-export-sessions') as HTMLDivElement,
  exportFormat: document.getElementById('export-format') as HTMLSelectElement,
  exportSessionCount: document.getElementById('export-session-count') as HTMLSpanElement,

  // Host Keys Manager
  hostKeysSearch: document.getElementById('host-keys-search') as HTMLInputElement,
  hostKeysList: document.getElementById('host-keys-list') as HTMLDivElement,
  btnRefreshKeys: document.getElementById('btn-refresh-keys') as HTMLButtonElement,
};

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

  tabs.set(tabId, tabInfo);
  renderTabs();
  switchToTab(tabId);

  return tabInfo;
}

function renderTabs(): void {
  // Clear existing tabs (except add button)
  const addButton = elements.tabBar.querySelector('.tab-add');
  elements.tabBar.innerHTML = '';

  tabs.forEach((tab) => {
    const tabElement = document.createElement('div');
    tabElement.className = `tab${tab.id === activeTabId ? ' active' : ''}`;
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

    elements.tabBar.appendChild(tabElement);
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
}

function switchToTab(tabId: string): void {
  const tab = tabs.get(tabId);
  if (!tab) return;

  activeTabId = tabId;
  renderTabs();

  // Hide all containers
  tabs.forEach(t => {
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
  const tab = tabs.get(tabId);
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
  tabs.delete(tabId);

  // Switch to another tab or create new one
  if (tabs.size === 0) {
    await createTab();
  } else if (activeTabId === tabId) {
    const firstTab = tabs.values().next().value;
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
  password: string,
  saveSession: boolean = false
): Promise<void> {
  let tab = activeTabId ? tabs.get(activeTabId) : null;

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
        password,
      } as ConnectRequest,
    });

    if (response.success) {
      tab.sessionId = response.session_id;
      tab.connected = true;
      tab.connectionParams = { host, port, username, password }; // Save for reconnect
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
      tab.terminal.onData((data) => {
        if (tab!.sessionId) {
          const encoder = new TextEncoder();
          invoke('ssh_write', {
            sessionId: tab!.sessionId,
            data: Array.from(encoder.encode(data)),
          });
        }
      });

      // Handle resize
      tab.terminal.onResize(() => {
        updateStatusBar(tab!);
        // TODO: Send resize to backend
      });

      updateStatusBar(tab);

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
          } as SavedSession,
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

async function loadSavedSessions(): Promise<void> {
  try {
    savedSessions = await invoke<SavedSession[]>('get_saved_sessions');

    // Sync folders
    savedSessions.forEach(s => {
      if (s.folder && !sessionFolders.includes(s.folder)) sessionFolders.push(s.folder);
    });

    renderSessionsList();
    renderSessionsTree();
  } catch (error) {
    console.error('Failed to load sessions:', error);
  }
}



function renderSessionsList(): void {
  if (savedSessions.length === 0) {
    elements.sessionsList.innerHTML = '<div class="sessions-empty">No saved sessions yet</div>';
    return;
  }

  elements.sessionsList.innerHTML = savedSessions.map((session) => `
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
        await invoke('delete_session', { sessionId });
        await loadSavedSessions();
      }
    });

    // Click to connect
    item.addEventListener('click', () => {
      const session = savedSessions.find((s) => s.id === sessionId);
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
    quickCommands = await invoke<QuickCommand[]>('get_quick_commands');

    // Sync folders
    quickCommands.forEach(c => {
      if (c.folder && !commandFolders.includes(c.folder)) commandFolders.push(c.folder);
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
  if (tooltipHideTimer) {
    window.clearTimeout(tooltipHideTimer);
    tooltipHideTimer = null;
  }
  const cmd = quickCommands.find(c => c.id === commandId);
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
  if (tooltipHideTimer) window.clearTimeout(tooltipHideTimer);
  tooltipHideTimer = window.setTimeout(() => {
    const tooltip = document.getElementById('command-tooltip');
    if (tooltip) tooltip.classList.add('hidden');
  }, 300); // 300ms delay to allow moving mouse into tooltip
};

// Updated render functions
function renderCommandTree(): void {
  const filterInput = document.getElementById('command-search') as HTMLInputElement;
  const filter = filterInput?.value.toLowerCase().trim() || '';

  let items = quickCommands;
  if (filter) {
    items = quickCommands.filter(c =>
      c.name.toLowerCase().includes(filter) ||
      c.command.toLowerCase().includes(filter) ||
      (c.description && c.description.toLowerCase().includes(filter)) ||
      (c.example && c.example.toLowerCase().includes(filter))
    );
  }

  const rootItems = items.filter(c => !c.folder);
  const folders = buildFolderTree(commandFolders, items.filter(c => c.folder), expandedCommandFolders);

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

  let items = savedSessions;
  if (filter) {
    items = savedSessions.filter(s =>
      s.name.toLowerCase().includes(filter) ||
      s.host.toLowerCase().includes(filter) ||
      (s.username && s.username.toLowerCase().includes(filter))
    );
  }

  const rootItems = items.filter(s => !s.folder);
  const folders = buildFolderTree(sessionFolders, items.filter(s => s.folder), expandedSessionFolders);

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
        const expandedSet = type === 'session' ? expandedSessionFolders : expandedCommandFolders;
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
        const session = savedSessions.find((s) => s.id === sessionId);
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
        const command = quickCommands.find((c) => c.id === commandId);
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
    const toDelete = savedSessions.filter(s => s.folder === path || s.folder?.startsWith(path + '/'));
    // We need 'delete_session' for each? Or bulk delete?
    // Bulk delete logic:
    for (const s of toDelete) {
      await invoke('delete_session', { sessionId: s.id });
    }
    // Update folders list
    sessionFolders = sessionFolders.filter(f => f !== path && !f.startsWith(path + '/'));
    await loadSavedSessions();
  } else {
    // Command deletion
    const toDelete = quickCommands.filter(c => c.folder === path || c.folder.startsWith(path + '/'));

    for (const c of toDelete) {
      await invoke('delete_quick_command', { commandId: c.id });
    }

    commandFolders = commandFolders.filter(f => f !== path && !f.startsWith(path + '/'));
    await loadQuickCommands();
  }
}


function executeQuickCommand(command: QuickCommand): void {
  const tab = activeTabId ? tabs.get(activeTabId) : null;
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
  elements.quickConnectModal.classList.add('show');
  elements.inputHost.focus();
}

function closeModal(modal: HTMLElement): void {
  modal.classList.remove('show');
}

function showModal(modal: HTMLElement): void {
  modal.classList.add('show');
}

// ============================================
// Context Menu
// ============================================

function showContextMenu(e: MouseEvent, sessionId: string | null): void {
  contextMenuType = 'session';
  contextMenuTargetSessionId = sessionId;
  updateContextMenuItems('session');
  displayMenu(e);
}

function showFolderContextMenu(e: MouseEvent, path: string, type: 'session' | 'command'): void {
  contextMenuType = 'folder'; // We need to distinguish session/command folder? 
  // We'll trust handleContextMenuAction to know which structure to delete from based on `contextMenuTargetFolder`? 
  // No, we need to know the type.
  // Let's rely on finding the folder in sessionFolders vs commandFolders? 
  // But names can collide.
  // I'll store the type in a temp property or use the global `folderContext`.
  // Let's reuse `folderContext` variable for this purpose temporarily or add a var.
  // Let's set the global `folderContext` which I already have (used for add modal).
  folderContext = type;
  contextMenuTargetFolder = path;

  updateContextMenuItems('folder');
  displayMenu(e);
}

function showCommandContextMenu(e: MouseEvent, commandId: string | null): void {
  contextMenuType = 'command';
  contextMenuTargetCommandId = commandId;
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
  contextMenuTargetSessionId = null;
  contextMenuTargetCommandId = null;
  contextMenuTargetFolder = null;
  contextMenuType = null;
}

// ============================================
// Tab Context Menu Logic
// ============================================

function showTabContextMenu(x: number, y: number, tabId: string): void {
  hideContextMenu(); // Close other menu
  contextMenuTargetTabId = tabId;
  const tab = tabs.get(tabId);

  if (!tab) return;

  // Update menu items based on state
  const disconnectItem = elements.tabContextMenu.querySelector('[data-action="disconnect"]') as HTMLElement;
  const reconnectItem = elements.tabContextMenu.querySelector('[data-action="reconnect"]') as HTMLElement;
  const saveItem = elements.tabContextMenu.querySelector('[data-action="save"]') as HTMLElement;

  const cloneItem = elements.tabContextMenu.querySelector('[data-action="clone"]') as HTMLElement;
  const sftpItem = elements.tabContextMenu.querySelector('[data-action="sftp"]') as HTMLElement;

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

  // Position and show
  elements.tabContextMenu.style.left = `${x}px`;
  elements.tabContextMenu.style.top = `${y}px`;
  elements.tabContextMenu.classList.add('visible');
}

function hideTabContextMenu(): void {
  elements.tabContextMenu.classList.remove('visible');
  contextMenuTargetTabId = null;
}

async function handleTabContextMenuAction(action: string): Promise<void> {
  if (!contextMenuTargetTabId) return;
  const tabId = contextMenuTargetTabId;
  const tab = tabs.get(tabId);

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
        const { host, port, username, password } = tab.connectionParams;
        if (activeTabId !== tabId) switchToTab(tabId);
        await connect(host, port, username, password, false);
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
      tabs.forEach((t) => {
        if (!t.connected) disconnectedIds.push(t.id);
      });
      for (const id of disconnectedIds) await closeTab(id);
      break;

    case 'close-others':
      const otherIds: string[] = [];
      tabs.forEach((t) => {
        if (t.id !== tabId) otherIds.push(t.id);
      });
      for (const id of otherIds) await closeTab(id);
      break;

    case 'close-right':
      const ids = Array.from(tabs.keys());
      const currentIndex = ids.indexOf(tabId);
      if (currentIndex !== -1) {
        const rightIds = ids.slice(currentIndex + 1);
        for (const id of rightIds) await closeTab(id);
      }
      break;

    case 'close-all':
      const allIds = Array.from(tabs.keys());
      for (const id of allIds) await closeTab(id);
      break;

    case 'clone':
      if (tab.connectionParams) {
        const { host, port, username, password } = tab.connectionParams;
        await createTab(host); // Create new tab
        await connect(host, port, username, password, false); // Connect in new tab
      }
      break;

    case 'sftp':
      if (activeTabId !== tabId) switchToTab(tabId);
      openSftpBrowser();
      break;

    case 'close-group':
    case 'add-group':
      // NYI - No op
      break;
  }
}


function handleContextMenuAction(action: string): void {
  if (contextMenuType === 'session' && contextMenuTargetSessionId) {
    const session = savedSessions.find((s) => s.id === contextMenuTargetSessionId);
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
          invoke('delete_session', { sessionId: session.id }).then(() => loadSavedSessions());
        }
        break;
    }
  } else if (contextMenuType === 'command' && contextMenuTargetCommandId) {
    const command = quickCommands.find(c => c.id === contextMenuTargetCommandId);
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
  } else if (contextMenuType === 'folder' && contextMenuTargetFolder) {
    if (action === 'delete-folder') {
      deleteFolder(contextMenuTargetFolder, folderContext); // folderContext is set in showFolderContextMenu
    } else if (action === 'new-subfolder') {
      showAddFolderModal(folderContext, contextMenuTargetFolder);
    }
  }

  hideContextMenu();
}

// ============================================
// Add/Edit Session Modal
// ============================================

function showAddSessionModal(): void {
  editingSessionId = null;
  elements.sessionModalTitle.textContent = 'Add Session';
  elements.sessionName.value = '';
  elements.sessionHost.value = '';
  elements.sessionPort.value = '22';
  elements.sessionUsername.value = '';
  elements.sessionPassword.value = '';
  populateFolderSelect(elements.sessionFolder, 'session');
  elements.addSessionModal.classList.add('show');
  elements.sessionName.focus();
}

async function showEditSessionModal(session: SavedSession): Promise<void> {
  editingSessionId = session.id;
  elements.sessionModalTitle.textContent = 'Edit Session';
  elements.sessionName.value = session.name || '';
  elements.sessionHost.value = session.host;
  elements.sessionPort.value = session.port.toString();
  elements.sessionUsername.value = session.username || '';
  elements.sessionPassword.value = ''; // Clear initially

  // Try to load password securely
  try {
    const password = await invoke<string | null>('get_session_password', { sessionId: session.id });
    if (password) {
      elements.sessionPassword.value = password;
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

  if (!host) {
    elements.sessionHost.focus();
    return;
  }

  const session: SavedSession = {
    id: editingSessionId || crypto.randomUUID(),
    name: name || host,
    host, // 4 spaces
    port,
    folder,
    username: elements.sessionUsername.value.trim() || undefined,
  };

  // Track the folder
  if (folder && !sessionFolders.includes(folder)) {
    sessionFolders.push(folder);
  }

  try {
    const sessionId = session.id;
    await invoke('save_session', { session });

    // Save password if provided
    const password = elements.sessionPassword.value;
    if (password) {
      await invoke('save_session_password', { sessionId, password });
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
  editingCommandId = null;
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
    id: editingCommandId || crypto.randomUUID(),
    name,
    command,
    folder,
    icon: color,
    description: description || undefined,
    example: example || undefined,
  };

  // Track the folder
  if (!commandFolders.includes(folder)) {
    commandFolders.push(folder);
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
  folderContext = context;
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

  if (folderContext === 'session') {
    if (!sessionFolders.includes(name)) {
      sessionFolders.push(name);
      renderSessionsTree();
    }
  } else {
    if (!commandFolders.includes(name)) {
      commandFolders.push(name);
      renderCommandTree();
    }
  }

  closeModal(elements.addFolderModal);
}

// ============================================
// Move Session Modal
// ============================================

function showMoveItemModal(type: 'session' | 'command', id: string): void {
  moveContext = type;
  elements.moveFolderInput.value = '';

  if (type === 'session') {
    moveSessionId = id;
    const session = savedSessions.find(s => s.id === id);
    if (session && session.folder) elements.moveFolderInput.value = session.folder;

    elements.moveFolderList.innerHTML = sessionFolders.map(folder =>
      `<option value="${escapeHtml(folder)}">`
    ).join('');
  } else {
    moveCommandId = id;
    const command = quickCommands.find(c => c.id === id);
    if (command && command.folder) elements.moveFolderInput.value = command.folder;

    elements.moveFolderList.innerHTML = commandFolders.map(folder =>
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

  if (moveContext === 'session') {
    if (!moveSessionId) return;
    const session = savedSessions.find(s => s.id === moveSessionId);
    if (!session) return;
    session.folder = folder;
    if (!sessionFolders.includes(folder)) sessionFolders.push(folder);

    try {
      await invoke('save_session', { session });
      await loadSavedSessions();
      closeModal(elements.moveSessionModal);
    } catch (error) {
      console.error('Failed to move session:', error);
    }
  } else {
    // Command move
    if (!moveCommandId) return;
    const command = quickCommands.find(c => c.id === moveCommandId);
    if (!command) return;
    command.folder = folder;
    if (!commandFolders.includes(folder)) commandFolders.push(folder);

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

function toggleAiPanel(): void {
  aiPanelOpen = !aiPanelOpen;
  elements.aiPanel.classList.toggle('collapsed', !aiPanelOpen);
  elements.aiToggleBtn.classList.toggle('active', aiPanelOpen);

  // Resize terminal when panel toggles
  setTimeout(() => {
    const tab = activeTabId ? tabs.get(activeTabId) : null;
    if (tab) {
      tab.fitAddon.fit();
    }
  }, 300); // Wait for animation
}

function renderAiMessages(): void {
  if (aiMessages.length === 0) {
    elements.aiMessages.innerHTML = `
      <div class="ai-welcome">
        <div class="ai-welcome-icon">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
            <path d="M12 2a2 2 0 0 1 2 2c0 .74-.4 1.39-1 1.73V7h1a7 7 0 0 1 7 7h1a1 1 0 0 1 1 1v3a1 1 0 0 1-1 1h-1v1a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-1H2a1 1 0 0 1-1-1v-3a1 1 0 0 1 1-1h1a7 7 0 0 1 7-7h1V5.73c-.6-.34-1-.99-1-1.73a2 2 0 0 1 2-2z" />
            <circle cx="8" cy="14" r="1" fill="currentColor" />
            <circle cx="16" cy="14" r="1" fill="currentColor" />
            <path d="M9 18h6" />
          </svg>
        </div>
        <h3>AI Assistant</h3>
        <p>Ask for help with commands, analyze terminal output, or get device-specific guidance.</p>
      </div>
    `;
    return;
  }

  let html = '';
  for (const msg of aiMessages) {
    // Convert markdown code blocks to HTML with copy buttons
    let content = escapeHtml(msg.content);

    // Handle fenced code blocks with copy button
    content = content.replace(/```(\w*)\n([\s\S]*?)```/g, (_match, _lang, code) => {
      const codeId = 'code-' + Math.random().toString(36).substr(2, 9);
      return `<div class="code-block-wrapper">
        <button class="code-copy-btn" data-code-id="${codeId}" title="Copy to clipboard">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <rect x="9" y="9" width="13" height="13" rx="2" ry="2"/>
            <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>
          </svg>
        </button>
        <pre><code id="${codeId}">${code.trim()}</code></pre>
      </div>`;
    });

    // Handle inline code
    content = content.replace(/`([^`]+)`/g, '<code>$1</code>');
    content = content.replace(/\n/g, '<br>');

    html += `
      <div class="ai-message ${msg.role}">
        <div class="ai-message-content">${content}</div>
      </div>
    `;
  }

  elements.aiMessages.innerHTML = html;
  elements.aiMessages.scrollTop = elements.aiMessages.scrollHeight;

  // Add click handlers for copy buttons
  document.querySelectorAll('.code-copy-btn').forEach(btn => {
    btn.addEventListener('click', async () => {
      const codeId = btn.getAttribute('data-code-id');
      const codeEl = document.getElementById(codeId!);
      if (codeEl) {
        try {
          await navigator.clipboard.writeText(codeEl.textContent || '');
          btn.classList.add('copied');
          btn.innerHTML = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M20 6L9 17l-5-5"/>
          </svg>`;
          setTimeout(() => {
            btn.classList.remove('copied');
            btn.innerHTML = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <rect x="9" y="9" width="13" height="13" rx="2" ry="2"/>
              <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>
            </svg>`;
          }, 2000);
        } catch (e) {
          console.error('Failed to copy:', e);
        }
      }
    });
  });
}

function getTerminalContent(): string {
  const tab = activeTabId ? tabs.get(activeTabId) : null;
  if (!tab) return '';

  // Get visible rows from terminal buffer
  const buffer = tab.terminal.buffer.active;
  const lines: string[] = [];

  for (let i = 0; i < buffer.length; i++) {
    const line = buffer.getLine(i);
    if (line) {
      lines.push(line.translateToString(true));
    }
  }

  // Return last 100 lines to avoid too much context
  return lines.slice(-100).join('\n').trim();
}

function showAiTyping(): void {
  const typingHtml = `
    <div class="ai-typing" id="ai-typing">
      <div class="ai-typing-dots">
        <span></span>
        <span></span>
        <span></span>
      </div>
    </div>
  `;
  elements.aiMessages.insertAdjacentHTML('beforeend', typingHtml);
  elements.aiMessages.scrollTop = elements.aiMessages.scrollHeight;
}

function hideAiTyping(): void {
  document.getElementById('ai-typing')?.remove();
}

function showAiError(message: string): void {
  const errorHtml = `
    <div class="ai-error">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <circle cx="12" cy="12" r="10" />
        <path d="M12 8v4M12 16h.01" />
      </svg>
      <span>${escapeHtml(message)}</span>
    </div>
  `;
  elements.aiMessages.insertAdjacentHTML('beforeend', errorHtml);
  elements.aiMessages.scrollTop = elements.aiMessages.scrollHeight;
}

async function sendAiMessage(userMessage: string): Promise<void> {
  if (!userMessage.trim()) return;

  // Add user message
  aiMessages.push({ role: 'user', content: userMessage });
  renderAiMessages();
  elements.aiInput.value = '';
  elements.aiSendBtn.disabled = true;

  showAiTyping();

  try {
    // Call backend AI chat function
    const response = await invoke<string>('ai_chat', {
      provider: aiProvider,
      messages: aiMessages,
    });

    hideAiTyping();
    aiMessages.push({ role: 'assistant', content: response });
    renderAiMessages();
  } catch (error) {
    hideAiTyping();
    showAiError(error instanceof Error ? error.message : 'Failed to get AI response. Check your API key settings.');
  } finally {
    elements.aiSendBtn.disabled = false;
  }
}

async function analyzeTerminal(): Promise<void> {
  const content = getTerminalContent();
  if (!content) {
    showAiError('No terminal content to analyze. Connect to a session first.');
    return;
  }

  const prompt = `Analyze this terminal output and tell me:
1. What type of device/system this appears to be (Cisco, Juniper, Linux, etc.)
2. Current context or mode
3. Any errors or notable information

Terminal content:
\`\`\`
${content}
\`\`\``;

  await sendAiMessage(prompt);
}

async function showCommandHelp(): Promise<void> {
  // Just add a starter message with context hint
  aiMessages.push({
    role: 'assistant',
    content: 'I can help you find the right commands. What are you trying to do? For example:\n- "How do I show interfaces on Cisco?"\n- "Show ARP table for VLAN 100"\n- "Check routing table on Juniper"'
  });
  renderAiMessages();
}

// ============================================
// AI Settings Modal
// ============================================

async function showAiSettingsModal(): Promise<void> {
  // Load current config from backend
  try {
    const config = await invoke<AiConfig>('get_ai_config');
    elements.aiGeminiKey.value = config.gemini_api_key || '';
    elements.aiGrokKey.value = config.grok_api_key || '';
    elements.aiOllamaHost.value = config.ollama_host || 'http://localhost:11434';
    elements.aiOllamaModel.value = config.ollama_model || 'llama3.2';
  } catch (e) {
    // Use defaults
    elements.aiGeminiKey.value = '';
    elements.aiGrokKey.value = '';
    elements.aiOllamaHost.value = 'http://localhost:11434';
    elements.aiOllamaModel.value = 'llama3.2';
  }

  elements.aiSettingsModal.classList.add('show');
}

function hideAiSettingsModal(): void {
  elements.aiSettingsModal.classList.remove('show');
  // Reset test button states
  elements.aiTestGemini.classList.remove('success', 'error');
  elements.aiTestGrok.classList.remove('success', 'error');
  elements.aiTestOllama.classList.remove('success', 'error');
}

async function saveAiSettings(): Promise<void> {
  const config: AiConfig = {
    gemini_api_key: elements.aiGeminiKey.value.trim() || null,
    grok_api_key: elements.aiGrokKey.value.trim() || null,
    ollama_host: elements.aiOllamaHost.value.trim() || 'http://localhost:11434',
    ollama_model: elements.aiOllamaModel.value.trim() || 'llama3.2',
  };

  try {
    await invoke('save_ai_config', { config });
    hideAiSettingsModal();
  } catch (e) {
    alert('Failed to save AI settings: ' + (e instanceof Error ? e.message : e));
  }
}

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
// Login Modal
// ============================================

async function showLoginModal(session: SavedSession): Promise<void> {
  loginSession = session;
  elements.loginModalTitle.textContent = `Connect to ${session.name}`;
  elements.loginUsername.value = session.username || '';
  elements.loginPassword.value = '';

  // Try to load password securely
  try {
    const password = await invoke<string | null>('get_session_password', { sessionId: session.id });
    if (password) {
      elements.loginPassword.value = password;
    }
  } catch (e) {
    console.error('Failed to load session password:', e);
  }

  elements.loginModal.classList.add('show');

  if (session.username) {
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
      const tab = activeTabId ? tabs.get(activeTabId) : null;
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
      const tab = activeTabId ? tabs.get(activeTabId) : null;
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
    aiProvider = elements.aiProviderSelect.value as 'gemini' | 'ollama';
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
    if (tooltipHideTimer) {
      window.clearTimeout(tooltipHideTimer);
      tooltipHideTimer = null;
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

    if (!host || !username) {
      alert('Please enter host and username');
      return;
    }

    closeModal(elements.quickConnectModal);
    await connect(host, port, username, password, saveSession);
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
    const tab = activeTabId ? tabs.get(activeTabId) : null;
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
      if (activeTabId) {
        closeTab(activeTabId);
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
    if (!loginSession) return;
    const username = elements.loginUsername.value.trim();
    const password = elements.loginPassword.value;

    if (!username) {
      alert('Username is required');
      return;
    }

    closeModal(elements.loginModal);
    await connect(loginSession.host, loginSession.port, username, password, false);
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

function escapeHtml(text: string): string {
  const div = document.createElement('div');
  div.textContent = text;
  return div.innerHTML;
}

// Helper to populate folder select
function populateFolderSelect(select: HTMLSelectElement, type: 'session' | 'command', selectedValue?: string): void {
  const foldersList = type === 'session' ? sessionFolders : commandFolders;
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
  confirm_on_close: boolean;
  save_window_size: boolean;
  restore_sessions: boolean;
  download_directory: string | null;
  log_directory: string | null;
  // Logging settings
  logging_enabled: boolean;
  log_format: string;
  log_timestamps: boolean;
}

// Current app settings
let currentSettings: AppSettings | null = null;

// Sessions pending import
let pendingImportSessions: SavedSession[] = [];

// Load application settings
async function loadAppSettings(): Promise<void> {
  try {
    currentSettings = await invoke<AppSettings>('get_settings');
  } catch (e) {
    console.error('Failed to load settings:', e);
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

  // Logging settings
  const logDirInput = document.getElementById('settings-log-dir') as HTMLInputElement;
  const logFormatInput = document.getElementById('settings-log-format') as HTMLInputElement;
  const logEnabledInput = document.getElementById('settings-logging-enabled') as HTMLInputElement;
  const logTimestampsInput = document.getElementById('settings-log-timestamps') as HTMLInputElement;

  if (logDirInput) logDirInput.value = currentSettings.log_directory || '';
  if (logFormatInput) logFormatInput.value = currentSettings.log_format || '{session}_{date}_{time}.log';
  if (logEnabledInput) logEnabledInput.checked = currentSettings.logging_enabled || false;
  if (logTimestampsInput) logTimestampsInput.checked = currentSettings.log_timestamps ?? true;
}

// Save settings from form
async function saveAppSettings(): Promise<void> {
  const logDirInput = document.getElementById('settings-log-dir') as HTMLInputElement;
  const logFormatInput = document.getElementById('settings-log-format') as HTMLInputElement;
  const logEnabledInput = document.getElementById('settings-logging-enabled') as HTMLInputElement;
  const logTimestampsInput = document.getElementById('settings-log-timestamps') as HTMLInputElement;

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
    log_directory: logDirInput?.value || null,
    logging_enabled: logEnabledInput?.checked || false,
    log_format: logFormatInput?.value || '{session}_{date}_{time}.log',
    log_timestamps: logTimestampsInput?.checked ?? true,
  };

  try {
    await invoke('save_settings', { settings });
    currentSettings = settings;
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

      // Load host keys if security tab
      if (tabId === 'security') {
        refreshHostKeys();
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
}

// Setup import modal event listeners
function setupImportListeners(): void {
  // File Menu: Open Connection
  document.getElementById('menu-file-open')?.addEventListener('click', () => {
    showQuickConnectModal();
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
    if (!activeTabId) return;
    const tab = tabs.get(activeTabId);
    if (tab?.terminal) {
      const selection = tab.terminal.getSelection();
      if (selection) {
        await navigator.clipboard.writeText(selection);
      }
    }
  });

  // Edit Menu: Paste
  document.getElementById('menu-edit-paste')?.addEventListener('click', async () => {
    if (!activeTabId) return;
    const tab = tabs.get(activeTabId);
    if (tab?.terminal && tab.sessionId) {
      const text = await navigator.clipboard.readText();
      if (text) {
        await invoke('ssh_write', { sessionId: tab.sessionId, data: text });
      }
    }
  });

  // Edit Menu: Select All
  document.getElementById('menu-edit-select-all')?.addEventListener('click', () => {
    if (!activeTabId) return;
    const tab = tabs.get(activeTabId);
    if (tab?.terminal) {
      tab.terminal.selectAll();
    }
  });

  // Edit Menu: Clear Terminal
  document.getElementById('menu-edit-clear')?.addEventListener('click', () => {
    if (!activeTabId) return;
    const tab = tabs.get(activeTabId);
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
    await openUrl('https://github.com/jspurrier/SpurXSecureSSH_rust_tauri');
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
      await openUrl('https://github.com/jspurrier/SpurXSecureSSH_rust_tauri#readme');
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

  // SFTP modal close handlers
  document.getElementById('sftp-modal-close')?.addEventListener('click', closeSftpBrowser);
  document.getElementById('btn-sftp-close')?.addEventListener('click', closeSftpBrowser);

  // SFTP navigation
  document.getElementById('sftp-btn-up')?.addEventListener('click', () => {
    if (sftpCurrentPath === '/') return;
    const parts = sftpCurrentPath.split('/').filter(p => p);
    parts.pop();
    sftpCurrentPath = '/' + parts.join('/');
    loadSftpDirectory(sftpCurrentPath);
  });

  document.getElementById('sftp-btn-refresh')?.addEventListener('click', () => {
    loadSftpDirectory(sftpCurrentPath);
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
    // Create a hidden file input for file selection
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = elements.importFormat.value === 'securecrt' ? '.xml' : '.reg';

    input.onchange = async () => {
      const file = input.files?.[0];
      if (!file) return;

      elements.importFilePath.value = file.name;

      // Read and preview
      const reader = new FileReader();
      reader.onload = async () => {

        // Create a temporary file path to pass to backend
        // For now, we'll show a message that direct file reading is needed
        try {
          // Use Tauri's file dialog if available
          const { open } = await import('@tauri-apps/plugin-dialog');
          const selected = await open({
            filters: [{
              name: elements.importFormat.value === 'securecrt' ? 'SecureCRT XML' : 'PuTTY Registry',
              extensions: elements.importFormat.value === 'securecrt' ? ['xml'] : ['reg']
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
      reader.readAsText(file);
    };

    // Try using Tauri dialog first
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const selected = await open({
        filters: [{
          name: elements.importFormat.value === 'securecrt' ? 'SecureCRT XML' : 'PuTTY Registry',
          extensions: elements.importFormat.value === 'securecrt' ? ['xml'] : ['reg']
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

  // Import confirm button
  document.getElementById('btn-import-confirm')?.addEventListener('click', async () => {
    if (pendingImportSessions.length === 0) return;

    try {
      // Save each session
      for (const session of pendingImportSessions) {
        await invoke('save_session', { session });
      }

      // Refresh session list
      await loadSavedSessions();
      closeModal(elements.importModal);
    } catch (e) {
      console.error('Failed to import sessions:', e);
      alert('Failed to import sessions: ' + e);
    }
  });

  // Export modal handlers
  document.getElementById('export-modal-close')?.addEventListener('click', () => {
    closeModal(elements.exportModal);
  });

  document.getElementById('btn-export-cancel')?.addEventListener('click', () => {
    closeModal(elements.exportModal);
  });

  document.getElementById('btn-export-confirm')?.addEventListener('click', async () => {
    try {
      const { save } = await import('@tauri-apps/plugin-dialog');
      const format = elements.exportFormat.value;
      const ext = format === 'securecrt' ? 'xml' : 'reg';
      const name = format === 'securecrt' ? 'SecureCRT XML' : 'PuTTY Registry';

      const filePath = await save({
        filters: [{ name, extensions: [ext] }],
        defaultPath: `sessions.${ext}`
      });

      if (filePath) {
        await invoke('export_sessions', {
          path: filePath,
          format,
          sessions: savedSessions
        });
        closeModal(elements.exportModal);
        alert(`Successfully exported ${savedSessions.length} sessions!`);
      }
    } catch (e) {
      console.error('Failed to export sessions:', e);
      alert('Failed to export sessions: ' + e);
    }
  });
}

// Show export modal
function showExportModal(): void {
  elements.exportSessionCount.textContent = savedSessions.length.toString();
  showModal(elements.exportModal);
}

// Reset import modal
function resetImportModal(): void {
  elements.importFilePath.value = '';
  elements.importPreview.innerHTML = '<p class="import-hint">Select a file to preview sessions</p>';
  pendingImportSessions = [];
  (document.getElementById('btn-import-confirm') as HTMLButtonElement).disabled = true;
}

// Preview sessions from file
async function previewImport(filePath: string): Promise<void> {
  try {
    const format = elements.importFormat.value;
    pendingImportSessions = await invoke<SavedSession[]>('import_sessions', { path: filePath, format });

    if (pendingImportSessions.length === 0) {
      elements.importPreview.innerHTML = '<p class="import-hint">No sessions found in this file</p>';
      return;
    }

    // Build preview HTML
    let html = '';
    pendingImportSessions.forEach(session => {
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

    html += `
      <div class="import-stats">
        <span>Found <span class="import-stats-count">${pendingImportSessions.length}</span> session(s)</span>
        <span>Ready to import</span>
      </div>
    `;

    elements.importPreview.innerHTML = html;
    (document.getElementById('btn-import-confirm') as HTMLButtonElement).disabled = false;
  } catch (e) {
    elements.importPreview.innerHTML = `<p class="import-hint" style="color: var(--accent-danger);">Error: ${e}</p>`;
    pendingImportSessions = [];
  }
}

// ============================================
// SFTP Browser Functions
// ============================================

async function openSftpBrowser(): Promise<void> {
  // Check if we have an active connection
  if (!activeTabId) {
    alert('No active connection. Please connect to a server first.');
    return;
  }

  const tab = tabs.get(activeTabId);
  if (!tab?.sessionId || !tab.connectionParams) {
    alert('No active SSH connection. Please connect to a server first.');
    return;
  }

  // Store connection params for SFTP
  sftpConnectionParams = tab.connectionParams;
  sftpSessionId = 'sftp-' + tab.sessionId;

  try {
    // Connect SFTP
    await invoke('sftp_connect', {
      sessionId: sftpSessionId,
      host: sftpConnectionParams.host,
      port: sftpConnectionParams.port,
      username: sftpConnectionParams.username,
      password: sftpConnectionParams.password,
    });

    // Show modal and load root directory
    const sftpModal = document.getElementById('modal-sftp');
    if (sftpModal) {
      showModal(sftpModal);
      sftpCurrentPath = '/home/' + sftpConnectionParams.username;
      await loadSftpDirectory(sftpCurrentPath);
    }
  } catch (e) {
    console.error('SFTP connect failed:', e);
    alert('Failed to connect SFTP: ' + e);
  }
}

async function closeSftpBrowser(): Promise<void> {
  const sftpModal = document.getElementById('modal-sftp');
  if (sftpModal) closeModal(sftpModal);

  // Disconnect SFTP
  if (sftpSessionId) {
    try {
      await invoke('sftp_disconnect', { sessionId: sftpSessionId });
    } catch (e) {
      console.error('SFTP disconnect error:', e);
    }
    sftpSessionId = null;
  }

  sftpSelectedFile = null;
  sftpConnectionParams = null;
}

async function loadSftpDirectory(path: string): Promise<void> {
  const fileList = document.getElementById('sftp-file-list');
  const pathInput = document.getElementById('sftp-current-path') as HTMLInputElement;

  if (!fileList || !sftpSessionId) return;

  fileList.innerHTML = '<div class="sftp-empty">Loading...</div>';
  pathInput.value = path;
  sftpCurrentPath = path;
  sftpSelectedFile = null;
  updateSftpButtons();

  try {
    const entries = await invoke<SftpFileEntry[]>('sftp_list', {
      sessionId: sftpSessionId,
      path: path,
    });

    if (entries.length === 0) {
      fileList.innerHTML = '<div class="sftp-empty">Empty directory</div>';
      return;
    }

    // Sort: directories first, then alphabetically
    entries.sort((a, b) => {
      if (a.is_dir && !b.is_dir) return -1;
      if (!a.is_dir && b.is_dir) return 1;
      return a.name.localeCompare(b.name);
    });

    fileList.innerHTML = entries.map(entry => `
      <div class="sftp-file-item" data-path="${escapeHtml(entry.path)}" data-is-dir="${entry.is_dir}" data-name="${escapeHtml(entry.name)}">
        <svg class="sftp-file-icon ${entry.is_dir ? 'folder' : ''}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          ${entry.is_dir
        ? '<path d="M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2v11z"/>'
        : '<path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/><polyline points="14 2 14 8 20 8"/>'
      }
        </svg>
        <span class="sftp-file-name">${escapeHtml(entry.name)}</span>
        <span class="sftp-file-size">${entry.is_dir ? '' : formatFileSize(entry.size)}</span>
      </div>
    `).join('');

    // Add click handlers
    fileList.querySelectorAll('.sftp-file-item').forEach(item => {
      item.addEventListener('click', () => {
        // Deselect others
        fileList.querySelectorAll('.sftp-file-item').forEach(i => i.classList.remove('selected'));
        item.classList.add('selected');

        const el = item as HTMLElement;
        sftpSelectedFile = {
          name: el.dataset.name || '',
          path: el.dataset.path || '',
          is_dir: el.dataset.isDir === 'true',
          size: 0,
        };
        updateSftpButtons();
      });

      item.addEventListener('dblclick', () => {
        const el = item as HTMLElement;
        if (el.dataset.isDir === 'true') {
          loadSftpDirectory(el.dataset.path || '');
        }
      });
    });
  } catch (e) {
    console.error('SFTP list failed:', e);
    fileList.innerHTML = `<div class="sftp-empty" style="color: var(--accent-danger);">Error: ${e}</div>`;
  }
}

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return bytes + ' B';
  if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + ' KB';
  if (bytes < 1024 * 1024 * 1024) return (bytes / (1024 * 1024)).toFixed(1) + ' MB';
  return (bytes / (1024 * 1024 * 1024)).toFixed(1) + ' GB';
}

function updateSftpButtons(): void {
  const downloadBtn = document.getElementById('sftp-btn-download') as HTMLButtonElement;
  const deleteBtn = document.getElementById('sftp-btn-delete') as HTMLButtonElement;

  if (downloadBtn) downloadBtn.disabled = !sftpSelectedFile || sftpSelectedFile.is_dir;
  if (deleteBtn) deleteBtn.disabled = !sftpSelectedFile;
}

async function uploadSftpFile(): Promise<void> {
  if (!sftpSessionId) return;

  const { open } = await import('@tauri-apps/plugin-dialog');
  const file = await open({ multiple: false });

  if (!file) return;

  try {
    const remotePath = sftpCurrentPath + '/' + (file as string).split(/[/\\]/).pop();
    await invoke('sftp_upload', {
      sessionId: sftpSessionId,
      localPath: file as string,
      remotePath: remotePath,
    });
    await loadSftpDirectory(sftpCurrentPath);
  } catch (e) {
    console.error('SFTP upload failed:', e);
    alert('Upload failed: ' + e);
  }
}

async function downloadSftpFile(): Promise<void> {
  if (!sftpSessionId || !sftpSelectedFile || sftpSelectedFile.is_dir) return;

  const { save } = await import('@tauri-apps/plugin-dialog');
  const file = await save({ defaultPath: sftpSelectedFile.name });

  if (!file) return;

  try {
    await invoke('sftp_download', {
      sessionId: sftpSessionId,
      remotePath: sftpSelectedFile.path,
      localPath: file,
    });
    alert('Download complete!');
  } catch (e) {
    console.error('SFTP download failed:', e);
    alert('Download failed: ' + e);
  }
}

async function createSftpFolder(): Promise<void> {
  if (!sftpSessionId) return;

  const name = prompt('Enter folder name:');
  if (!name) return;

  try {
    await invoke('sftp_mkdir', {
      sessionId: sftpSessionId,
      path: sftpCurrentPath + '/' + name,
    });
    await loadSftpDirectory(sftpCurrentPath);
  } catch (e) {
    console.error('SFTP mkdir failed:', e);
    alert('Failed to create folder: ' + e);
  }
}

async function deleteSftpFile(): Promise<void> {
  if (!sftpSessionId || !sftpSelectedFile) return;

  if (!confirm(`Delete "${sftpSelectedFile.name}"?`)) return;

  try {
    if (sftpSelectedFile.is_dir) {
      await invoke('sftp_delete_dir', {
        sessionId: sftpSessionId,
        path: sftpSelectedFile.path,
      });
    } else {
      await invoke('sftp_delete_file', {
        sessionId: sftpSessionId,
        path: sftpSelectedFile.path,
      });
    }
    await loadSftpDirectory(sftpCurrentPath);
  } catch (e) {
    console.error('SFTP delete failed:', e);
    alert('Delete failed: ' + e);
  }
}

// ============================================
// Auto Updater
// ============================================

async function initializeUpdater() {
  try {
    const update = await check();
    if (update && update.available) {
      console.log(`Update checked: ${update.version} is available`);
      const notification = document.getElementById('update-notification');
      const versionSpan = document.getElementById('update-version');
      const restartBtn = document.getElementById('btn-update-restart');
      const closeBtn = document.getElementById('btn-update-close');

      if (notification && versionSpan && restartBtn) {
        versionSpan.textContent = `v${update.version}`;
        notification.classList.remove('hidden');

        restartBtn.addEventListener('click', async () => {
          restartBtn.textContent = 'Updating...';
          restartBtn.setAttribute('disabled', 'true');
          try {
            await update.downloadAndInstall();
            await relaunch();
          } catch (e) {
            console.error('Update failed:', e);
            restartBtn.textContent = 'Failed';
            setTimeout(() => {
              restartBtn.textContent = 'Restart to Update';
              restartBtn.removeAttribute('disabled');
            }, 3000);
          }
        });

        closeBtn?.addEventListener('click', () => {
          notification.classList.add('hidden');
        });
      }
    } else {
      console.log('Update checked: App is up to date');
    }
  } catch (e) {
    console.error('Failed to check for updates:', e);
  }
}

async function init(): Promise<void> {
  setupOsShortcuts();
  setupEventListeners();
  setupSettingsListeners();
  setupImportListeners();
  await loadAppSettings();
  await loadQuickCommands();
  await loadSavedSessions();
  await createTab();

  // Check for updates
  await initializeUpdater();
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
