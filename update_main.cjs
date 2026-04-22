const fs = require('fs');
const path = require('path');

const filePath = path.join(__dirname, 'src', 'main.ts');
let content = fs.readFileSync(filePath, 'utf8');

// 1. SavedSession Interface
content = content.replace(
  '  log_directory?: string;\n}',
  '  log_directory?: string;\n  use_global_credentials?: boolean;\n}'
);

// 2. AppSettings Interface
content = content.replace(
  '  keepalive_interval: number;',
  '  keepalive_interval: number;\n  global_username: string | null;'
);

// 3. elements structure
content = content.replace(
  "sessionModalTitle: document.getElementById('session-modal-title') as HTMLHeadingElement,",
  "sessionModalTitle: document.getElementById('session-modal-title') as HTMLHeadingElement,\n  sessionUseGlobal: document.getElementById('session-use-global') as HTMLInputElement,"
);

content = content.replace(
  "importPreview: document.getElementById('import-preview') as HTMLDivElement,",
  "importPreview: document.getElementById('import-preview') as HTMLDivElement,\n  importMasterPassword: document.getElementById('import-master-password') as HTMLInputElement,\n  importMasterPasswordGroup: document.getElementById('import-master-password-group') as HTMLDivElement,"
);

content = content.replace(
  "exportSessionCount: document.getElementById('export-session-count') as HTMLSpanElement,",
  "exportSessionCount: document.getElementById('export-session-count') as HTMLSpanElement,\n  exportMasterPassword: document.getElementById('export-master-password') as HTMLInputElement,\n  exportMasterPasswordGroup: document.getElementById('export-master-password-group') as HTMLDivElement,"
);

// 4. showAddSessionModal
content = content.replace(
  "elements.sessionLogDirectory.value = '';",
  "elements.sessionLogDirectory.value = '';\n  elements.sessionUseGlobal.checked = false;\n  document.getElementById('session-username-group')!.style.display = 'block';\n  document.getElementById('session-password-group')!.style.display = 'block';"
);

// 5. showEditSessionModal
content = content.replace(
  "elements.sessionLogDirectory.value = session.log_directory || '';",
  `elements.sessionLogDirectory.value = session.log_directory || '';
  elements.sessionUseGlobal.checked = session.use_global_credentials || false;
  if(session.use_global_credentials) {
    document.getElementById('session-username-group')!.style.display = 'none';
    document.getElementById('session-password-group')!.style.display = 'none';
  } else {
    document.getElementById('session-username-group')!.style.display = 'block';
    document.getElementById('session-password-group')!.style.display = 'block';
  }`
);

// 6. saveSessionModal
content = content.replace(
  "log_directory,",
  "log_directory,\n    use_global_credentials: elements.sessionUseGlobal.checked,"
);

// 7. populateSettingsForm
content = content.replace(
  "elements.settingsDownloadDir.value = currentSettings.download_directory || '';",
  `elements.settingsDownloadDir.value = currentSettings.download_directory || '';
  
  const globalUser = document.getElementById('settings-global-username') as HTMLInputElement;
  if (globalUser) globalUser.value = currentSettings.global_username || '';
  
  const globalPwd = document.getElementById('settings-global-password') as HTMLInputElement;
  if (globalPwd) {
    globalPwd.value = '';
    invoke<string | null>('get_global_password').then(pwd => {
      if (pwd) globalPwd.value = pwd;
    }).catch(console.error);
  }`
);

// 8. saveAppSettings
content = content.replace(
  "download_directory: elements.settingsDownloadDir.value || null,",
  `download_directory: elements.settingsDownloadDir.value || null,\n    global_username: (document.getElementById('settings-global-username') as HTMLInputElement)?.value || null,`
);

content = content.replace(
  "currentSettings = settings;",
  `currentSettings = settings;
    
    const globalPwdInput = document.getElementById('settings-global-password') as HTMLInputElement;
    if (globalPwdInput && globalPwdInput.value) {
      await invoke('save_global_password', { password: globalPwdInput.value });
    } else if (globalPwdInput && !globalPwdInput.value) {
      await invoke('delete_global_password');
    }`
);

// 9. showLoginModal
content = content.replace(
  `  elements.loginUsername.value = session.username || '';
  elements.loginPassword.value = '';

  // Try to load password securely
  try {
    const password = await invoke<string | null>('get_session_password', { sessionId: session.id });
    if (password) {
      elements.loginPassword.value = password;
    }
  } catch (e) {
    console.error('Failed to load session password:', e);
  }`,
  `  if (session.use_global_credentials) {
    elements.loginUsername.value = currentSettings?.global_username || '';
    elements.loginPassword.value = '';
    try {
      const gPwd = await invoke<string | null>('get_global_password');
      if (gPwd) elements.loginPassword.value = gPwd;
    } catch(e) { console.error(e); }
  } else {
    elements.loginUsername.value = session.username || '';
    elements.loginPassword.value = '';
    try {
      const password = await invoke<string | null>('get_session_password', { sessionId: session.id });
      if (password) {
        elements.loginPassword.value = password;
      }
    } catch (e) {
      console.error('Failed to load session password:', e);
    }
  }`
);

// 10. setupEventListeners (add sessionUseGlobal listener)
content = content.replace(
  "document.getElementById('btn-session-save')?.addEventListener('click', () => {",
  `elements.sessionUseGlobal.addEventListener('change', (e) => {
    const checked = (e.target as HTMLInputElement).checked;
    if (checked) {
      document.getElementById('session-username-group')!.style.display = 'none';
      document.getElementById('session-password-group')!.style.display = 'none';
    } else {
      document.getElementById('session-username-group')!.style.display = 'block';
      document.getElementById('session-password-group')!.style.display = 'block';
    }
  });\n\n  document.getElementById('btn-session-save')?.addEventListener('click', () => {`
);

// 11. setupImportListeners Format toggles & import logic
content = content.replace(
  `// File Menu: Import Sessions`,
  `  elements.importFormat.addEventListener('change', () => {
    const format = elements.importFormat.value;
    if (format === 'spurx_encrypted') {
      elements.importMasterPasswordGroup.style.display = 'block';
      elements.importMasterPasswordGroup.classList.remove('hidden');
    } else {
      elements.importMasterPasswordGroup.style.display = 'none';
      elements.importMasterPasswordGroup.classList.add('hidden');
    }
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
  });\n\n  // File Menu: Import Sessions`
);

content = content.replace(
  "pendingImportSessions = await invoke<SavedSession[]>('import_sessions', { path: filePath, format });",
  `const pwd = elements.importMasterPassword.value || null;
    pendingImportSessions = await invoke<SavedSession[]>('import_sessions', { path: filePath, format, password: pwd });`
);

content = content.replace(
  `await invoke('export_sessions', {
          path: filePath,
          format,
          sessions: savedSessions
        });`,
  `await invoke('export_sessions', {
          path: filePath,
          format,
          sessions: savedSessions,
          password: elements.exportMasterPassword.value || null
        });`
);

content = content.replace(
  `const ext = format === 'securecrt' ? 'xml' : 'reg';`,
  `let ext = format === 'securecrt' ? 'xml' : 'reg';
      if (format === 'spurx_encrypted') ext = 'spurx';
      if (format === 'spurx_plaintext') ext = 'json';`
);

content = content.replace(
  `const name = format === 'securecrt' ? 'SecureCRT XML' : 'PuTTY Registry';`,
  `let name = format === 'securecrt' ? 'SecureCRT XML' : 'PuTTY Registry';
      if (format === 'spurx_encrypted') name = 'SpurX Encrypted';
      if (format === 'spurx_plaintext') name = 'SpurX JSON';`
);

fs.writeFileSync(filePath, content);
console.log('Update complete.');
