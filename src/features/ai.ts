import { state } from "../state";
import { elements } from "../ui/elements";
import { escapeHtml } from "../ui/utils";
import { AiConfig } from "../types";
const { invoke } = window.__TAURI__.core;

let autocompletePopup: HTMLDivElement | null = null;
let currentSelectedIndex = 0;
let suggestionList: Array<{ cmd: string; desc: string }> = [];
let detectedContextText = '';

export function toggleAiPanel(): void {
    state.aiPanelOpen = !state.aiPanelOpen;
    elements.aiPanel.classList.toggle('collapsed', !state.aiPanelOpen);
    elements.aiToggleBtn.classList.toggle('active', state.aiPanelOpen);
    if (state.aiPanelOpen) {
      updatePrivacyBanner();
    }

    setTimeout(() => {
      const tab = state.activeTabId ? state.tabs.get(state.activeTabId) : null;
      if (tab) {
        tab.fitAddon.fit();
      }
    }, 300);
}

export function renderAiMessages(): void {
    if (state.aiMessages.length === 0) {
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
    for (const msg of state.aiMessages) {
      // Convert markdown code blocks to HTML with copy & run buttons
      let content = escapeHtml(msg.content);

      content = content.replace(/```(\w*)\n([\s\S]*?)```/g, (_match, _lang, code) => {
        const codeId = 'code-' + Math.random().toString(36).substr(2, 9);
        return `<div class="code-block-wrapper" style="position: relative;">
              <button class="code-run-btn" data-code-id="${codeId}" title="Run in active terminal" style="position: absolute; top: 8px; right: 42px; width: 28px; height: 28px; padding: 0; display: flex; align-items: center; justify-content: center; background: var(--bg-tertiary); border: 1px solid var(--border-primary); border-radius: 4px; color: var(--text-muted); cursor: pointer; opacity: 0.7; transition: all var(--transition-fast);">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14">
                  <polygon points="5 3 19 12 5 21 5 3"/>
                </svg>
              </button>
              <button class="code-copy-btn" data-code-id="${codeId}" title="Copy to clipboard">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <rect x="9" y="9" width="13" height="13" rx="2" ry="2"/>
                  <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>
                </svg>
              </button>
              <pre><code id="${codeId}">${code.trim()}</code></pre>
            </div>`;
      });

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

    // Attach copy button listeners
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

    // Attach run button listeners
    document.querySelectorAll('.code-run-btn').forEach(btn => {
      btn.addEventListener('click', () => {
        const codeId = btn.getAttribute('data-code-id');
        const codeEl = document.getElementById(codeId!);
        if (codeEl) {
          const command = codeEl.textContent || '';
          runCommandInActiveTab(command);
        }
      });
    });
}

export function showAiTyping(): void {
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

export function hideAiTyping(): void {
    document.getElementById('ai-typing')?.remove();
}

export function showAiError(message: string): void {
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

export async function sendAiMessage(userMessage: string): Promise<void> {
    if (!userMessage.trim()) return;
    state.aiMessages.push({ role: 'user', content: userMessage });
    renderAiMessages();
    elements.aiInput.value = '';
    elements.aiSendBtn.disabled = true;
    showAiTyping();
    try {
      const response = await invoke<string>('ai_chat', {
        provider: state.aiProvider,
        messages: state.aiMessages,
      });

      hideAiTyping();
      state.aiMessages.push({ role: 'assistant', content: response });
      renderAiMessages();
    } catch (error) {
      hideAiTyping();
      showAiError(error instanceof Error ? error.message : 'Failed to get AI response. Check your API key settings.');
    } finally {
      elements.aiSendBtn.disabled = false;
    }
}

export async function showAiSettingsModal(): Promise<void> {
    try {
      const config = await invoke<AiConfig>('get_ai_config');
      elements.aiGeminiKey.value = config.gemini_api_key || '';
      elements.aiGrokKey.value = config.grok_api_key || '';
      elements.aiOllamaHost.value = config.ollama_host || 'http://localhost:11434';
      elements.aiOllamaModel.value = config.ollama_model || 'llama3.2';
    } catch (e) {
      elements.aiGeminiKey.value = '';
      elements.aiGrokKey.value = '';
      elements.aiOllamaHost.value = 'http://localhost:11434';
      elements.aiOllamaModel.value = 'llama3.2';
    }

    elements.aiSettingsModal.classList.add('show');
}

export function hideAiSettingsModal(): void {
    elements.aiSettingsModal.classList.remove('show');
    elements.aiTestGemini.classList.remove('success', 'error');
    elements.aiTestGrok.classList.remove('success', 'error');
    elements.aiTestOllama.classList.remove('success', 'error');
}

export async function saveAiSettings(): Promise<void> {
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

export function updatePrivacyBanner(): void {
    const banner = document.getElementById('ai-privacy-banner');
    if (!banner) return;
    const isCloud = state.aiProvider === 'gemini' || state.aiProvider === 'grok';
    banner.classList.toggle('hidden', !isCloud);
}

export function getTerminalContent(): string {
    const tab = state.activeTabId ? state.tabs.get(state.activeTabId) : null;
    if (!tab) return '';
    const buffer = tab.terminal.buffer.active;
    const lines: string[] = [];
    for (let i = 0; i < buffer.length; i++) {
      const line = buffer.getLine(i);
      if (line) {
        lines.push(line.translateToString(true));
      }
    }

    return lines.slice(-100).join('\n').trim();
}

export async function analyzeTerminal(): Promise<void> {
  const buffer = getTerminalContent();
  if (!buffer) {
    alert('Terminal buffer is empty or no active session.');
    return;
  }

  if (!state.aiPanelOpen) {
    toggleAiPanel();
  }

  const prompt = `Analyze the following terminal buffer for any errors, warnings, or failures. If you find an error, explain it and provide the exact commands to fix it. If everything looks correct, state so.\n\nTerminal Buffer:\n\`\`\`\n${buffer}\n\`\`\``;
  await sendAiMessage(prompt);
}

export function runCommandInActiveTab(cmd: string): void {
  if (!state.activeTabId) {
    alert('No active tab. Please connect to a server first.');
    return;
  }
  const tab = state.tabs.get(state.activeTabId);
  if (!tab || !tab.sessionId || !tab.connected) {
    alert('No active SSH connection in this tab.');
    return;
  }

  const encoder = new TextEncoder();
  const data = Array.from(encoder.encode(cmd.trim() + '\n'));
  invoke('ssh_write', {
    sessionId: tab.sessionId,
    data: data,
  }).then(() => {
    setTimeout(() => {
      if (tab.terminal) tab.terminal.focus();
    }, 50);
  }).catch((e) => {
    console.error('Failed to execute command in terminal:', e);
  });
}

export async function triggerSmartAutocomplete(tab: any): Promise<void> {
  removeAutocompletePopup();

  const buffer = tab.terminal.buffer.active;
  
  // Extract first 100 lines (which usually contain the SSH welcome banner/MOTD/OS name)
  const firstLines: string[] = [];
  const bannerLimit = Math.min(100, buffer.length);
  for (let i = 0; i < bannerLimit; i++) {
    const line = buffer.getLine(i);
    if (line) firstLines.push(line.translateToString(true));
  }
  const loginBanner = firstLines.join('\n').trim();

  // Extract last 500 lines for current context (helps capture previous interface command listings, IP configurations, show run blocks, or ? help printouts)
  const lastLines: string[] = [];
  const startIdx = Math.max(0, buffer.length - 500);
  for (let i = startIdx; i < buffer.length; i++) {
    const line = buffer.getLine(i);
    if (line) lastLines.push(line.translateToString(true));
  }
  const context = lastLines.join('\n').trim();

  const cursorX = buffer.cursorX;
  const cursorY = buffer.cursorY;
  
  let left = 100;
  let top = 200;
  try {
    const core = tab.terminal._core;
    const cellWidth = core._renderService.dimensions.actualCellWidth || 9;
    const cellHeight = core._renderService.dimensions.actualCellHeight || 18;
    left = cursorX * cellWidth + 40;
    top = cursorY * cellHeight + 20;
  } catch (e) {
    console.error('Failed to compute cursor position:', e);
  }

  autocompletePopup = document.createElement('div');
  autocompletePopup.className = 'autocomplete-popup';
  autocompletePopup.id = 'autocomplete-popup';
  autocompletePopup.style.left = `${left}px`;
  autocompletePopup.style.top = `${top}px`;
  autocompletePopup.innerHTML = '<div style="padding: 8px 12px; color: var(--text-muted);">Detecting platform & suggestions...</div>';
  
  const terminalWrapper = tab.container || document.body;
  terminalWrapper.appendChild(autocompletePopup);

  try {
    const prompt = `You are a shell autocomplete helper for network equipment (Cisco IOS/IOS-XR/IOS-XE, Nokia SR OS, Cisco ASR, etc.) and Linux/Unix servers.
We have captured two sections of the terminal buffer: the initial welcome banner (to help identify the platform/OS) and the recent command context.

Welcome/Login Banner:
"""
${loginBanner}
"""

Recent Context & Prompt:
"""
${context}
"""

Analyze both to auto-detect:
1. The platform type (e.g., "Cisco IOS-XR", "Nokia SR OS", "Linux bash", "Cisco ASR")
2. The current command context/mode (e.g., config mode, privileged exec, bash)

Suggest 3-5 completions for the current command line.

CRITICAL INSTANCE ANALYSIS:
- Scan the "Recent Context & Prompt" for any real physical/logical interface names (e.g. "GigabitEthernet0/0/0/1", "Bundle-Ether10", "Loopback0", "1/1/c1/1"), VLAN IDs, or VRF names that were printed by past command outputs (like "show ip interface brief", "show interfaces description", "? Help options", etc.).
- Prioritize using these EXACT real interface names/parameters when completing commands rather than guessing generic placeholder names.

Return ONLY a valid JSON object matching the schema below. No explanation, no markdown formatting, no code block wrappers.
JSON schema:
{
  "detected_device": "Detected Platform Name & Mode (e.g., Cisco IOS-XR Config)",
  "suggestions": [
    {"cmd": "completed command line", "desc": "short description"}
  ]
}
`;

    const res = await invoke<string>('ai_chat', {
      provider: state.aiProvider,
      messages: [{ role: 'user', content: prompt }]
    });

    let cleanJson = res.trim();
    if (cleanJson.startsWith('```')) {
      const match = cleanJson.match(/```(?:json)?\s*([\s\S]*?)\s*```/);
      if (match) cleanJson = match[1];
    }
    const data = JSON.parse(cleanJson);
    if (data && typeof data === 'object' && !Array.isArray(data)) {
      detectedContextText = data.detected_device || 'Auto-Detected Context';
      suggestionList = data.suggestions || [];
    } else if (Array.isArray(data)) {
      detectedContextText = 'Auto-Detected Context';
      suggestionList = data;
    } else {
      detectedContextText = 'Auto-Detected Context';
      suggestionList = [];
    }
  } catch (err) {
    console.error('AI autocomplete failed, using fallback:', err);
    detectedContextText = 'Linux / Bash (Fallback)';
    suggestionList = [
      { cmd: 'ls -lh', desc: 'List files with human-readable sizes' },
      { cmd: 'df -h', desc: 'Show disk space usage' },
      { cmd: 'free -m', desc: 'Show free and used memory in MB' },
      { cmd: 'systemctl status', desc: 'Show systemd service status' },
      { cmd: 'ps aux | grep', desc: 'Search for running processes' }
    ];
  }

  if (suggestionList.length === 0) {
    autocompletePopup.innerHTML = '<div style="padding: 8px 12px; color: var(--text-muted);">No suggestions found</div>';
    setTimeout(removeAutocompletePopup, 1500);
    return;
  }

  currentSelectedIndex = 0;
  renderAutocompleteItems(tab);
}

function renderAutocompleteItems(tab: any): void {
  if (!autocompletePopup) return;

  const headerHtml = `<div class="autocomplete-header">${escapeHtml(detectedContextText)}</div>`;
  const itemsHtml = suggestionList.map((item, idx) => `
    <div class="autocomplete-item ${idx === currentSelectedIndex ? 'selected' : ''}" data-index="${idx}">
      <span class="cmd">${escapeHtml(item.cmd)}</span>
      <span class="desc">${escapeHtml(item.desc)}</span>
    </div>
  `).join('');

  autocompletePopup.innerHTML = headerHtml + itemsHtml;

  autocompletePopup.querySelectorAll('.autocomplete-item').forEach(itemEl => {
    itemEl.addEventListener('click', () => {
      const idx = parseInt((itemEl as HTMLElement).dataset.index || '0');
      insertSuggestion(tab, suggestionList[idx].cmd);
    });
  });

  document.addEventListener('keydown', handlePopupKeydown);
}

function handlePopupKeydown(e: KeyboardEvent): void {
  if (!autocompletePopup) return;

  const tab = state.activeTabId ? state.tabs.get(state.activeTabId) : null;
  if (!tab) {
    removeAutocompletePopup();
    return;
  }

  if (e.key === 'ArrowDown') {
    e.preventDefault();
    currentSelectedIndex = (currentSelectedIndex + 1) % suggestionList.length;
    renderAutocompleteItems(tab);
  } else if (e.key === 'ArrowUp') {
    e.preventDefault();
    currentSelectedIndex = (currentSelectedIndex - 1 + suggestionList.length) % suggestionList.length;
    renderAutocompleteItems(tab);
  } else if (e.key === 'Enter') {
    e.preventDefault();
    insertSuggestion(tab, suggestionList[currentSelectedIndex].cmd);
  } else if (e.key === 'Escape') {
    e.preventDefault();
    removeAutocompletePopup();
  }
}

function insertSuggestion(tab: any, cmd: string): void {
  removeAutocompletePopup();
  if (tab.sessionId && tab.connected) {
    const buffer = tab.terminal.buffer.active;
    const currentLine = buffer.getLine(buffer.baseY + buffer.cursorY)?.translateToString(true) || '';
    
    // Find longest suffix of currentLine that matches prefix of cmd
    let overlap = '';
    for (let i = 0; i < currentLine.length; i++) {
      const suffix = currentLine.substring(i);
      if (cmd.startsWith(suffix)) {
        overlap = suffix;
        break;
      }
    }
    
    const remainingCmd = cmd.substring(overlap.length);
    if (remainingCmd) {
      const encoder = new TextEncoder();
      invoke('ssh_write', {
        sessionId: tab.sessionId,
        data: Array.from(encoder.encode(remainingCmd)),
      }).catch(e => console.error('Failed to write suggestion:', e));
    }
  }

  // Restore focus to terminal
  setTimeout(() => {
    tab.terminal.focus();
  }, 50);
}

export function removeAutocompletePopup(): void {
  if (autocompletePopup) {
    autocompletePopup.remove();
    autocompletePopup = null;
  }
  document.removeEventListener('keydown', handlePopupKeydown);

  // Return focus to active tab terminal
  if (state.activeTabId) {
    const tab = state.tabs.get(state.activeTabId);
    if (tab?.terminal) {
      tab.terminal.focus();
    }
  }
}

document.addEventListener('click', (e) => {
  if (autocompletePopup && !autocompletePopup.contains(e.target as Node)) {
    removeAutocompletePopup();
  }
});
