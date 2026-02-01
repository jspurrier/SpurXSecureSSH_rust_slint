/**
 * SpurX Secure SSH - Main Application
 * Handles terminal management, SSH connections, and UI interactions
 */

import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { WebglAddon } from '@xterm/addon-webgl';
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
let folderContext: 'session' | 'command' = 'command';

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

  // Sidebar Title
  sidebarTitle: document.getElementById('sidebar-title') as HTMLSpanElement,

  // Status bar
  statusConnection: document.getElementById('status-connection') as HTMLSpanElement,
  statusCipher: document.getElementById('status-cipher') as HTMLSpanElement,
  statusSize: document.getElementById('status-size') as HTMLSpanElement,
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

  return { terminal, fitAddon };
}

function generateTabId(): string {
  return `tab-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`;
}

async function createTab(title: string = 'New Tab'): Promise<TabInfo> {
  const tabId = generateTabId();
  const { terminal, fitAddon } = createTerminal();

  const tabInfo: TabInfo = {
    id: tabId,
    title,
    terminal,
    fitAddon,
    connected: false,
    unlisteners: [],
    connectionParams: null,
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

  // Update active state
  activeTabId = tabId;
  renderTabs();

  // Clear viewport and attach terminal
  elements.terminalViewport.innerHTML = '';
  tab.terminal.open(elements.terminalViewport);

  // Load WebGL addon if not already loaded
  try {
    const webglAddon = new WebglAddon();
    tab.terminal.loadAddon(webglAddon);
  } catch (e) {
    console.warn('WebGL addon could not be loaded:', e);
  }

  // Fit terminal to viewport
  setTimeout(() => {
    tab.fitAddon.fit();
    updateStatusBar(tab);
  }, 0);
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
          isOpen: true // Default open for now
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

  // Render root items first (if any)
  if (rootItems.length > 0) {
    if (type === 'session') {
      html += rootItems.map(item => renderSessionItem(item as SavedSession)).join('');
    } else {
      html += rootItems.map(item => renderCommandItem(item as QuickCommand)).join('');
    }
  }

  // Render folders (sorted)
  const sortedKeys = Array.from(nodes.keys()).sort();

  sortedKeys.forEach(key => {
    const node = nodes.get(key)!;
    const isExpanded = true; // TODO: State tracking

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
    <div class="command-item" data-command-id="${cmd.id}">
      <div class="command-icon ${cmd.icon || 'cyan'}"></div>
      <div class="command-info">
        <div class="command-name">${escapeHtml(cmd.name)}</div>
        <div class="command-text">${escapeHtml(cmd.command)}</div>
      </div>
    </div>
  `;
}

// Updated render functions
function renderCommandTree(): void {
  // Separate root items (empty folder or 'General' top level if desired)
  // For now, treat 'General' as just another folder
  const rootItems = quickCommands.filter(c => !c.folder);
  const folders = buildFolderTree(commandFolders, quickCommands.filter(c => c.folder));

  elements.commandTree.innerHTML = renderTreeHtml(folders, 'command', rootItems);

  attachTreeListeners(elements.commandTree, 'command');
}

function renderSessionsTree(): void {
  const rootItems = savedSessions.filter(s => !s.folder);
  const folders = buildFolderTree(sessionFolders, savedSessions.filter(s => s.folder));

  elements.sessionsTree.innerHTML = renderTreeHtml(folders, 'session', rootItems);

  attachTreeListeners(elements.sessionsTree, 'session');
}

function attachTreeListeners(container: HTMLElement, type: 'session' | 'command'): void {
  // Folder toggles
  container.querySelectorAll('.folder-header').forEach((header) => {
    header.addEventListener('click', () => {
      // Don't toggle if clicking actions (if added later)
      header.classList.toggle('expanded');
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
  elements.contextMenu.classList.remove('active');
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

  if (tab.connected) {
    disconnectItem.style.display = 'flex';
    reconnectItem.style.display = 'none';
  } else {
    disconnectItem.style.display = 'none';
    reconnectItem.style.display = 'flex';
  }

  // Can only save if we have connection details
  if (tab.connectionParams) {
    saveItem.style.display = 'flex';
  } else {
    saveItem.style.display = 'none';
  }

  // Position and show
  elements.tabContextMenu.style.display = 'block';
  elements.tabContextMenu.style.left = `${x}px`;
  elements.tabContextMenu.style.top = `${y}px`;
  elements.tabContextMenu.classList.add('active');
}

function hideTabContextMenu(): void {
  elements.tabContextMenu.style.display = 'none';
  elements.tabContextMenu.classList.remove('active');
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
  populateFolderSelect(elements.sessionFolder, 'session');
  elements.addSessionModal.classList.add('show');
  elements.sessionName.focus();
}

function showEditSessionModal(session: SavedSession): void {
  editingSessionId = session.id;
  elements.sessionModalTitle.textContent = 'Edit Session';
  elements.sessionName.value = session.name || '';
  elements.sessionHost.value = session.host;
  elements.sessionPort.value = session.port.toString();
  elements.sessionUsername.value = session.username || '';
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
    await invoke('save_session', { session });
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
// Login Modal
// ============================================

function showLoginModal(session: SavedSession): void {
  loginSession = session;
  elements.loginModalTitle.textContent = `Connect to ${session.name}`;
  elements.loginUsername.value = session.username || '';
  elements.loginPassword.value = '';

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

async function init(): Promise<void> {
  setupEventListeners();
  await loadQuickCommands();
  await loadSavedSessions();
  await createTab();
}

// Wait for DOM to be ready
if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', init);
} else {
  init();
}
