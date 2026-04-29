import { state } from "../state";
import { elements } from "../ui/elements";
import { escapeHtml } from "../ui/utils";
import { AiConfig } from "../types";
const { invoke } = window.__TAURI__.core;

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
    // Call backend AI chat function
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
    // Use defaults
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
