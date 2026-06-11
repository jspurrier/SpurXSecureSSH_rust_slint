import { state } from "../state";
import { escapeHtml, showModal, closeModal, formatFileSize } from "../ui/utils";
import { SftpFileEntry } from "../types";

const { invoke } = window.__TAURI__.core;

let localCurrentPath = '';
let localSelectedFile: SftpFileEntry | null = null;
let sftpProgressUnlisten: (() => void) | null = null;

interface TransferItem {
  id: string;
  filePath: string;
  bytesTransferred: number;
  totalBytes: number;
  type: 'upload' | 'download';
  status: 'active' | 'completed' | 'failed';
  speed?: number;
}
let sftpTransfers: TransferItem[] = [];

export async function openSftpBrowser(): Promise<void> {
    if (!state.activeTabId) {
      alert('No active connection. Please connect to a server first.');
      return;
    }

    const tab = state.tabs.get(state.activeTabId);
    if (!tab?.sessionId || !tab.connectionParams) {
      alert('No active SSH connection. Please connect to a server first.');
      return;
    }

    state.sftpConnectionParams = tab.connectionParams;
    state.sftpSessionId = 'sftp-' + tab.sessionId;
    try {
      // Connect SFTP
      await invoke('sftp_connect', {
        sessionId: state.sftpSessionId,
        host: state.sftpConnectionParams.host,
        port: state.sftpConnectionParams.port,
        username: state.sftpConnectionParams.username,
        password: state.sftpConnectionParams.password || null,
        authMethod: state.sftpConnectionParams.auth_method || 'password',
        privateKeyName: state.sftpConnectionParams.private_key_name || null,
        privateKeyPassphrase: state.sftpConnectionParams.private_key_passphrase || null,
      });

      // Show modal and load remote root directory
      const sftpModal = document.getElementById('modal-sftp');
      if (sftpModal) {
        showModal(sftpModal);
        
        // Remote Dir load
        state.sftpCurrentPath = '/home/' + state.sftpConnectionParams.username;
        await loadSftpDirectory(state.sftpCurrentPath);

        // Local Dir load (Home Dir)
        try {
          const home = await invoke<string>('local_get_home_dir');
          await loadLocalDirectory(home);
        } catch (e) {
          console.error('Failed to get local home directory:', e);
          await loadLocalDirectory('C:\\');
        }

        // Setup Drag & Drop
        setupSftpDragAndDrop();

        // Setup Local Navigation Listeners
        setupLocalNavigationListeners();

        // Setup Queue Clear Listener
        setupQueueListeners();

        // Setup progress listener
        const { listen } = window.__TAURI__.event;
        if (sftpProgressUnlisten) {
          sftpProgressUnlisten();
          sftpProgressUnlisten = null;
        }
        sftpProgressUnlisten = await listen<any>(`sftp-progress-${state.sftpSessionId}`, (event) => {
          const p = event.payload;
          const fileName = p.file_path.split('/').pop() || p.file_path;
          const item = sftpTransfers.find(t => 
            t.status === 'active' && 
            t.type === p.transfer_type && 
            (t.filePath === fileName || p.file_path.endsWith(t.filePath))
          );
          if (item) {
            item.bytesTransferred = p.bytes_transferred;
            item.totalBytes = p.total_bytes;
            renderSftpQueue();
          }
        });
      }
    } catch (e) {
      console.error('SFTP connect failed:', e);
      alert('Failed to connect SFTP: ' + e);
    }
}

export async function closeSftpBrowser(): Promise<void> {
    const sftpModal = document.getElementById('modal-sftp');
    if (sftpModal) closeModal(sftpModal);
    if (state.sftpSessionId) {
      try {
        await invoke('sftp_disconnect', { sessionId: state.sftpSessionId });
      } catch (e) {
        console.error('SFTP disconnect error:', e);
      }
      state.sftpSessionId = null;
    }

    if (sftpProgressUnlisten) {
      sftpProgressUnlisten();
      sftpProgressUnlisten = null;
    }

    state.sftpSelectedFile = null;
    state.sftpConnectionParams = null;
    localSelectedFile = null;
}

export async function loadSftpDirectory(path: string): Promise<void> {
    const fileList = document.getElementById('sftp-file-list');
    const pathInput = document.getElementById('sftp-current-path') as HTMLInputElement;
    if (!fileList || !state.sftpSessionId) return;
    fileList.innerHTML = '<div class="sftp-empty">Loading...</div>';
    pathInput.value = path;
    state.sftpCurrentPath = path;
    state.sftpSelectedFile = null;
    updateSftpButtons();
    try {
      const entries = await invoke<SftpFileEntry[]>('sftp_list', {
        sessionId: state.sftpSessionId,
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
        <div class="sftp-file-item remote-file-item" draggable="true" data-path="${escapeHtml(entry.path)}" data-is-dir="${entry.is_dir}" data-name="${escapeHtml(entry.name)}">
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
      fileList.querySelectorAll('.remote-file-item').forEach(item => {
        item.addEventListener('click', () => {
          fileList.querySelectorAll('.remote-file-item').forEach(i => i.classList.remove('selected'));
          item.classList.add('selected');

          const el = item as HTMLElement;
          state.sftpSelectedFile = {
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

        // HTML5 Dragstart
        item.addEventListener('dragstart', (e: any) => {
          const el = item as HTMLElement;
          e.dataTransfer.setData('text/plain', JSON.stringify({
            source: 'remote',
            name: el.dataset.name,
            path: el.dataset.path,
            isDir: el.dataset.isDir === 'true'
          }));
        });
      });
    } catch (e) {
      console.error('SFTP list failed:', e);
      fileList.innerHTML = `<div class="sftp-empty" style="color: var(--accent-danger);">Error: ${e}</div>`;
    }
}

export async function loadLocalDirectory(path: string): Promise<void> {
  const fileList = document.getElementById('sftp-local-file-list');
  const pathInput = document.getElementById('sftp-local-path') as HTMLInputElement;
  if (!fileList) return;

  fileList.innerHTML = '<div class="sftp-empty">Loading...</div>';
  pathInput.value = path;
  localCurrentPath = path;
  localSelectedFile = null;
  updateSftpButtons();

  try {
    const entries = await invoke<any[]>('local_list_dir', { path });

    if (entries.length === 0) {
      fileList.innerHTML = '<div class="sftp-empty">Empty directory</div>';
      return;
    }

    entries.sort((a, b) => {
      if (a.is_dir && !b.is_dir) return -1;
      if (!a.is_dir && b.is_dir) return 1;
      return a.name.localeCompare(b.name);
    });

    fileList.innerHTML = entries.map(entry => `
      <div class="sftp-file-item local-file-item" draggable="true" data-path="${escapeHtml(entry.path)}" data-is-dir="${entry.is_dir}" data-name="${escapeHtml(entry.name)}">
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

    // Click handlers for local items
    fileList.querySelectorAll('.local-file-item').forEach(item => {
      item.addEventListener('click', () => {
        fileList.querySelectorAll('.local-file-item').forEach(i => i.classList.remove('selected'));
        item.classList.add('selected');
        
        const el = item as HTMLElement;
        localSelectedFile = {
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
          loadLocalDirectory(el.dataset.path || '');
        }
      });

      // HTML5 Dragstart
      item.addEventListener('dragstart', (e: any) => {
        const el = item as HTMLElement;
        e.dataTransfer.setData('text/plain', JSON.stringify({
          source: 'local',
          name: el.dataset.name,
          path: el.dataset.path,
          isDir: el.dataset.isDir === 'true'
        }));
      });
    });
  } catch (e) {
    console.error('Local list failed:', e);
    fileList.innerHTML = `<div class="sftp-empty" style="color: var(--accent-danger);">Error: ${e}</div>`;
  }
}

export function updateSftpButtons(): void {
    const downloadBtn = document.getElementById('sftp-btn-download') as HTMLButtonElement;
    const deleteBtn = document.getElementById('sftp-btn-delete') as HTMLButtonElement;
    
    // Enable download if remote file is selected and not a directory
    if (downloadBtn) {
      downloadBtn.disabled = !state.sftpSelectedFile || state.sftpSelectedFile.is_dir;
    }
    
    // Enable delete if remote file OR local file is selected
    if (deleteBtn) {
      deleteBtn.disabled = !state.sftpSelectedFile && !localSelectedFile;
    }
}

export async function uploadSftpFile(): Promise<void> {
    if (!state.sftpSessionId) return;
    
    let file = localSelectedFile?.path || null;
    if (!file) {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const selected = await open({ multiple: false });
      if (!selected) return;
      file = selected as string;
    }
    
    try {
      const remotePath = state.sftpCurrentPath + '/' + file.split(/[/\\]/).pop();
      startSftpTransfer(file, remotePath, 'upload');
    } catch (e) {
      console.error('SFTP upload failed:', e);
      alert('Upload failed: ' + e);
    }
}

export async function downloadSftpFile(): Promise<void> {
    if (!state.sftpSessionId || !state.sftpSelectedFile || state.sftpSelectedFile.is_dir) return;
    
    let file = localCurrentPath ? (localCurrentPath + '/' + state.sftpSelectedFile.name) : null;
    if (!file) {
      const { save } = await import('@tauri-apps/plugin-dialog');
      const selected = await save({ defaultPath: state.sftpSelectedFile.name });
      if (!selected) return;
      file = selected;
    }
    
    try {
      startSftpTransfer(state.sftpSelectedFile.path, file, 'download');
    } catch (e) {
      console.error('SFTP download failed:', e);
      alert('Download failed: ' + e);
    }
}

export async function createSftpFolder(): Promise<void> {
    if (!state.sftpSessionId) return;
    const name = prompt('Enter folder name:');
    if (!name) return;
    try {
      await invoke('sftp_mkdir', {
        sessionId: state.sftpSessionId,
        path: state.sftpCurrentPath + '/' + name,
      });
      await loadSftpDirectory(state.sftpCurrentPath);
    } catch (e) {
      console.error('SFTP mkdir failed:', e);
      alert('Failed to create folder: ' + e);
    }
}

export async function deleteSftpFile(): Promise<void> {
    if (state.sftpSessionId && state.sftpSelectedFile) {
      if (!confirm(`Delete remote file/folder "${state.sftpSelectedFile.name}"?`)) return;
      try {
        if (state.sftpSelectedFile.is_dir) {
          await invoke('sftp_delete_dir', {
            sessionId: state.sftpSessionId,
            path: state.sftpSelectedFile.path,
          });
        } else {
          await invoke('sftp_delete_file', {
            sessionId: state.sftpSessionId,
            path: state.sftpSelectedFile.path,
          });
        }
        await loadSftpDirectory(state.sftpCurrentPath);
      } catch (e) {
        console.error('SFTP delete failed:', e);
        alert('Delete failed: ' + e);
      }
    }
    else if (localSelectedFile) {
      if (!confirm(`Delete local file/folder "${localSelectedFile.name}"?`)) return;
      try {
        await invoke('local_delete', { path: localSelectedFile.path });
        await loadLocalDirectory(localCurrentPath);
      } catch (e) {
        console.error('Local delete failed:', e);
        alert('Delete failed: ' + e);
      }
    }
}

async function startSftpTransfer(localOrRemotePath: string, targetPath: string, type: 'upload' | 'download'): Promise<void> {
  if (!state.sftpSessionId) return;

  const transferId = Math.random().toString(36).substr(2, 9);
  const fileName = localOrRemotePath.split(/[/\\]/).pop() || localOrRemotePath;
  
  const transferItem: TransferItem = {
    id: transferId,
    filePath: fileName,
    bytesTransferred: 0,
    totalBytes: 0,
    type,
    status: 'active',
  };

  sftpTransfers.push(transferItem);
  renderSftpQueue();

  try {
    if (type === 'upload') {
      await invoke('sftp_upload', {
        sessionId: state.sftpSessionId,
        localPath: localOrRemotePath,
        remotePath: targetPath,
      });
      await loadSftpDirectory(state.sftpCurrentPath);
    } else {
      await invoke('sftp_download', {
        sessionId: state.sftpSessionId,
        remotePath: localOrRemotePath,
        localPath: targetPath,
      });
      await loadLocalDirectory(localCurrentPath);
    }
    
    const idx = sftpTransfers.findIndex(t => t.id === transferId);
    if (idx !== -1) {
      sftpTransfers[idx].status = 'completed';
      sftpTransfers[idx].bytesTransferred = sftpTransfers[idx].totalBytes;
    }
  } catch (e) {
    console.error('Transfer failed:', e);
    const idx = sftpTransfers.findIndex(t => t.id === transferId);
    if (idx !== -1) {
      sftpTransfers[idx].status = 'failed';
    }
    alert(`Transfer of "${fileName}" failed: ${e}`);
  } finally {
    renderSftpQueue();
  }
}

function renderSftpQueue(): void {
  const queueList = document.getElementById('sftp-queue-list');
  if (!queueList) return;

  if (sftpTransfers.length === 0) {
    queueList.innerHTML = '<div class="sftp-queue-empty">No active transfers</div>';
    return;
  }

  queueList.innerHTML = sftpTransfers.map(item => {
    const percent = item.totalBytes > 0 ? Math.round((item.bytesTransferred / item.totalBytes) * 100) : 0;
    const sizeStr = item.totalBytes > 0 ? formatFileSize(item.totalBytes) : 'Unknown size';
    const transStr = formatFileSize(item.bytesTransferred);
    
    let statusClass = '';
    let statusText = `${transStr} / ${sizeStr} (${percent}%)`;
    if (item.status === 'completed') {
      statusClass = 'completed';
      statusText = `Completed (${sizeStr})`;
    } else if (item.status === 'failed') {
      statusClass = 'failed';
      statusText = 'Failed';
    }

    return `
      <div class="sftp-queue-item ${statusClass}">
        <div class="sftp-queue-item-meta">
          <span class="sftp-queue-item-name">${item.type === 'upload' ? '↑' : '↓'} ${escapeHtml(item.filePath)}</span>
          <span>${statusText}</span>
        </div>
        <div class="sftp-queue-item-progress-bar">
          <div class="sftp-queue-item-progress-fill" style="width: ${percent}%"></div>
        </div>
      </div>
    `;
  }).join('');
}

function setupSftpDragAndDrop(): void {
  const localPane = document.getElementById('sftp-pane-local');
  const remotePane = document.getElementById('sftp-pane-remote');

  if (localPane) {
    localPane.addEventListener('dragover', (e) => {
      e.preventDefault();
      localPane.classList.add('drag-over');
    });

    localPane.addEventListener('dragleave', () => {
      localPane.classList.remove('drag-over');
    });

    localPane.addEventListener('drop', async (e: any) => {
      e.preventDefault();
      localPane.classList.remove('drag-over');
      try {
        const dataStr = e.dataTransfer.getData('text/plain');
        if (!dataStr) return;
        const dragData = JSON.parse(dataStr);
        if (dragData.source === 'remote') {
          if (dragData.isDir) {
            alert('Directory download via drag-and-drop is not supported yet. Use file download.');
            return;
          }
          const targetLocalPath = localCurrentPath + '/' + dragData.name;
          startSftpTransfer(dragData.path, targetLocalPath, 'download');
        }
      } catch (err) {
        console.error('Drop local failed:', err);
      }
    });
  }

  if (remotePane) {
    remotePane.addEventListener('dragover', (e) => {
      e.preventDefault();
      remotePane.classList.add('drag-over');
    });

    remotePane.addEventListener('dragleave', () => {
      remotePane.classList.remove('drag-over');
    });

    remotePane.addEventListener('drop', async (e: any) => {
      e.preventDefault();
      remotePane.classList.remove('drag-over');
      try {
        const dataStr = e.dataTransfer.getData('text/plain');
        if (!dataStr) return;
        const dragData = JSON.parse(dataStr);
        if (dragData.source === 'local') {
          if (dragData.isDir) {
            alert('Directory upload via drag-and-drop is not supported yet. Use file upload.');
            return;
          }
          const targetRemotePath = state.sftpCurrentPath + '/' + dragData.name;
          startSftpTransfer(dragData.path, targetRemotePath, 'upload');
        }
      } catch (err) {
        console.error('Drop remote failed:', err);
      }
    });
  }
}

function setupLocalNavigationListeners(): void {
  const upBtn = document.getElementById('sftp-local-btn-up');
  if (upBtn) {
    const newUpBtn = upBtn.cloneNode(true);
    upBtn.parentNode?.replaceChild(newUpBtn, upBtn);
    newUpBtn.addEventListener('click', () => {
      const parts = localCurrentPath.split(/[/\\]/).filter(p => p.length > 0);
      if (parts.length > 1) {
        const isWindowsDrive = parts[0].endsWith(':');
        if (parts.length === 2 && isWindowsDrive) {
          loadLocalDirectory(parts[0] + '\\');
        } else {
          const separator = localCurrentPath.includes('\\') ? '\\' : '/';
          let newPath = parts.slice(0, -1).join(separator);
          if (isWindowsDrive && !newPath.includes('\\') && !newPath.includes('/')) {
            newPath += separator;
          }
          loadLocalDirectory(newPath);
        }
      }
    });
  }

  const refreshBtn = document.getElementById('sftp-local-btn-refresh');
  if (refreshBtn) {
    const newRefreshBtn = refreshBtn.cloneNode(true);
    refreshBtn.parentNode?.replaceChild(newRefreshBtn, refreshBtn);
    newRefreshBtn.addEventListener('click', () => {
      loadLocalDirectory(localCurrentPath);
    });
  }

  const pathInput = document.getElementById('sftp-local-path') as HTMLInputElement | null;
  if (pathInput) {
    pathInput.onkeydown = (e) => {
      if (e.key === 'Enter') {
        loadLocalDirectory(pathInput.value.trim());
      }
    };
  }
}

function setupQueueListeners(): void {
  const clearBtn = document.getElementById('sftp-queue-clear');
  if (clearBtn) {
    const newClearBtn = clearBtn.cloneNode(true);
    clearBtn.parentNode?.replaceChild(newClearBtn, clearBtn);
    newClearBtn.addEventListener('click', () => {
      sftpTransfers = sftpTransfers.filter(t => t.status === 'active');
      renderSftpQueue();
    });
  }
}
