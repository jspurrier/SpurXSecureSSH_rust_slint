import { state } from "../state";
import { elements } from "../ui/elements";
import { TabInfo } from "../types";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
}

const textEncoder = new TextEncoder();

export function enableTabKeyboard(tab: TabInfo): void {
  if (tab.onDataDisposable) {
    tab.onDataDisposable.dispose();
  }
  tab.onDataDisposable = tab.terminal.onData((data) => {
    if (tab.sessionId && tab.connected) {
      invoke('ssh_write', {
        sessionId: tab.sessionId,
        data: Array.from(textEncoder.encode(data)),
      });
    }
  });
}

export function disableTabKeyboard(tab: TabInfo): void {
  if (tab.onDataDisposable) {
    tab.onDataDisposable.dispose();
    tab.onDataDisposable = undefined;
  }
}

export async function startYmodemSend(): Promise<void> {
  if (!state.activeTabId) {
    alert('No active tab. Please connect to a server first.');
    return;
  }

  const tab = state.tabs.get(state.activeTabId);
  if (!tab || !tab.sessionId || !tab.connected) {
    alert('No active SSH connection in this tab.');
    return;
  }

  const sessionId = tab.sessionId;

  // Let user pick a file
  let filePath: string | null = null;
  try {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const selected = await open({
      multiple: false,
      title: 'Select File to Send via YModem',
    });
    if (!selected) return;
    filePath = selected as string;
  } catch (e) {
    console.error('File dialog error:', e);
    alert('Failed to open file picker: ' + e);
    return;
  }

  // Get filename from path
  const filename = filePath.split(/[/\\]/).pop() || filePath;

  // Lock terminal keyboard input
  disableTabKeyboard(tab);

  // Initialize progress UI overlay
  elements.ymodemOverlay.classList.remove('hidden');
  elements.ymodemFilename.textContent = filename;
  elements.ymodemProgressFill.style.width = '0%';
  elements.ymodemPercentage.textContent = '0%';
  elements.ymodemBytesRatio.textContent = `0 / ${formatBytes(0)}`;
  elements.ymodemSpeed.textContent = '0 KB/s';
  elements.ymodemElapsed.textContent = '0s';
  elements.ymodemStateBadge.textContent = 'Initializing';
  elements.ymodemStateBadge.className = 'ymodem-badge';

  const startTime = Date.now();
  const timerId = setInterval(() => {
    const elapsed = Math.round((Date.now() - startTime) / 1000);
    elements.ymodemElapsed.textContent = `${elapsed}s`;
  }, 1000);

  // Set cancel abort button handler
  elements.ymodemAbortBtn.onclick = async () => {
    try {
      await invoke('ssh_cancel_ymodem', { sessionId });
    } catch (e) {
      console.error('Cancel YModem error:', e);
    }
  };

  // Setup progress / completion / error listeners
  let progressUnlisten: (() => void) | null = null;
  let finishedUnlisten: (() => void) | null = null;
  let errorUnlisten: (() => void) | null = null;

  const cleanup = () => {
    clearInterval(timerId);
    if (progressUnlisten) progressUnlisten();
    if (finishedUnlisten) finishedUnlisten();
    if (errorUnlisten) errorUnlisten();
    
    // Re-enable keyboard
    enableTabKeyboard(tab);
  };

  try {
    progressUnlisten = await listen<any>(`ymodem-progress-${sessionId}`, (event) => {
      const p = event.payload;
      elements.ymodemStateBadge.textContent = p.state;
      elements.ymodemStateBadge.className = 'ymodem-badge sending';
      elements.ymodemFilename.textContent = p.filename;

      const percent = p.total_bytes > 0 ? Math.round((p.bytes_sent / p.total_bytes) * 100) : 0;
      elements.ymodemProgressFill.style.width = `${percent}%`;
      elements.ymodemPercentage.textContent = `${percent}%`;
      elements.ymodemBytesRatio.textContent = `${formatBytes(p.bytes_sent)} / ${formatBytes(p.total_bytes)}`;
      elements.ymodemSpeed.textContent = `${p.speed.toFixed(1)} KB/s`;
    });

    finishedUnlisten = await listen<void>(`ymodem-finished-${sessionId}`, () => {
      cleanup();
      elements.ymodemStateBadge.textContent = 'Finished';
      elements.ymodemStateBadge.className = 'ymodem-badge finished';
      elements.ymodemProgressFill.style.width = '100%';
      elements.ymodemPercentage.textContent = '100%';

      setTimeout(() => {
        elements.ymodemOverlay.classList.add('hidden');
      }, 1500);
    });

    errorUnlisten = await listen<string>(`ymodem-error-${sessionId}`, (event) => {
      cleanup();
      elements.ymodemStateBadge.textContent = 'Error';
      elements.ymodemStateBadge.className = 'ymodem-badge failed';
      alert(`YModem transfer failed: ${event.payload}`);
      
      setTimeout(() => {
        elements.ymodemOverlay.classList.add('hidden');
      }, 2000);
    });

    // Actually trigger backend transfer
    await invoke('ssh_start_ymodem_send', { sessionId, filePath });
  } catch (err) {
    cleanup();
    alert('Failed to initiate YModem transfer: ' + err);
    elements.ymodemOverlay.classList.add('hidden');
  }
}
