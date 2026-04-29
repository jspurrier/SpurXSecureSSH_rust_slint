export let elements = {
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
      sessionLogDirectory: document.getElementById('session-log-directory') as HTMLInputElement,
      sessionModalTitle: document.getElementById('session-modal-title') as HTMLHeadingElement,
      sessionUseGlobal: document.getElementById('session-use-global') as HTMLInputElement,

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
      importMasterPassword: document.getElementById('import-master-password') as HTMLInputElement,

      // Password Prompt Modal
      passwordPromptModal: document.getElementById('modal-password-prompt') as HTMLDivElement,
      passwordPromptInput: document.getElementById('pwd-prompt-input') as HTMLInputElement,
      passwordPromptTitle: document.getElementById('pwd-prompt-title') as HTMLHeadingElement,
      passwordPromptDescription: document.getElementById('pwd-prompt-description') as HTMLParagraphElement,
      passwordPromptConfirm: document.getElementById('btn-pwd-prompt-confirm') as HTMLButtonElement,
      passwordPromptCancel: document.getElementById('btn-pwd-prompt-cancel') as HTMLButtonElement,
      passwordPromptClose: document.getElementById('pwd-prompt-close') as HTMLButtonElement,
      importMasterPasswordGroup: document.getElementById('import-master-password-group') as HTMLDivElement,

      // Export Sessions Modal
      exportModal: document.getElementById('modal-export-sessions') as HTMLDivElement,
      exportFormat: document.getElementById('export-format') as HTMLSelectElement,
      exportSessionCount: document.getElementById('export-session-count') as HTMLSpanElement,
      exportCommandCount: document.getElementById('export-command-count') as HTMLSpanElement,
      exportIncludeSessions: document.getElementById('export-include-sessions') as HTMLInputElement,
      exportIncludeCommands: document.getElementById('export-include-commands') as HTMLInputElement,
      exportSummaryCount: document.getElementById('export-summary-count') as HTMLSpanElement,
      exportMasterPassword: document.getElementById('export-master-password') as HTMLInputElement,
      exportMasterPasswordGroup: document.getElementById('export-master-password-group') as HTMLDivElement,

      // Import Conflict Modal
      conflictModal: document.getElementById('modal-import-conflict') as HTMLDivElement,
      conflictMessage: document.getElementById('conflict-message') as HTMLDivElement,
      conflictHost: document.getElementById('conflict-host') as HTMLElement,
      conflictApplyAll: document.getElementById('conflict-apply-all') as HTMLInputElement,

      // Host Keys Manager
      hostKeysSearch: document.getElementById('host-keys-search') as HTMLInputElement,
      hostKeysList: document.getElementById('host-keys-list') as HTMLDivElement,
      btnRefreshKeys: document.getElementById('btn-refresh-keys') as HTMLButtonElement,
    };
