import { state } from "../state";
import { escapeHtml, showModal, closeModal, formatFileSize } from "../ui/utils";
import { SftpFileEntry } from "../types";

const { invoke } = window.__TAURI__.core;

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
      password: state.sftpConnectionParams.password,
    });

    // Show modal and load root directory
    const sftpModal = document.getElementById('modal-sftp');
    if (sftpModal) {
      showModal(sftpModal);
      state.sftpCurrentPath = '/home/' + state.sftpConnectionParams.username;
      await loadSftpDirectory(state.sftpCurrentPath);
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

    state.sftpSelectedFile = null;
    state.sftpConnectionParams = null;
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
    });
    } catch (e) {
    console.error('SFTP list failed:', e);
    fileList.innerHTML = `<div class="sftp-empty" style="color: var(--accent-danger);">Error: ${e}</div>`;
    }
}

export function updateSftpButtons(): void {
    const downloadBtn = document.getElementById('sftp-btn-download') as HTMLButtonElement;
    const deleteBtn = document.getElementById('sftp-btn-delete') as HTMLButtonElement;
    if (downloadBtn) downloadBtn.disabled = !state.sftpSelectedFile || state.sftpSelectedFile.is_dir;
    if (deleteBtn) deleteBtn.disabled = !state.sftpSelectedFile;
}

export async function uploadSftpFile(): Promise<void> {
    if (!state.sftpSessionId) return;
    const { open } = await import('@tauri-apps/plugin-dialog');
    const file = await open({ multiple: false });
    if (!file) return;
    try {
    const remotePath = state.sftpCurrentPath + '/' + (file as string).split(/[/\\]/).pop();
    await invoke('sftp_upload', {
      sessionId: state.sftpSessionId,
      localPath: file as string,
      remotePath: remotePath,
    });
    await loadSftpDirectory(state.sftpCurrentPath);
    } catch (e) {
    console.error('SFTP upload failed:', e);
    alert('Upload failed: ' + e);
    }
}

export async function downloadSftpFile(): Promise<void> {
    if (!state.sftpSessionId || !state.sftpSelectedFile || state.sftpSelectedFile.is_dir) return;
    const { save } = await import('@tauri-apps/plugin-dialog');
    const file = await save({ defaultPath: state.sftpSelectedFile.name });
    if (!file) return;
    try {
    await invoke('sftp_download', {
      sessionId: state.sftpSessionId,
      remotePath: state.sftpSelectedFile.path,
      localPath: file,
    });
    alert('Download complete!');
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
    if (!state.sftpSessionId || !state.sftpSelectedFile) return;
    if (!confirm(`Delete "${state.sftpSelectedFile.name}"?`)) return;
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
