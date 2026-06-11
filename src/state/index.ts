
import { TabInfo, TabGroup, SavedSession, QuickCommand, SftpFileEntry, ConnectRequest } from '../types';

export const state = {
    tabs: new Map<string, TabInfo>(),
    tabGroups: new Map<string, TabGroup>(),
    activeTabId: null as string | null,
    savedSessions: [] as SavedSession[],
    quickCommands: [] as QuickCommand[],
    
    editingSessionId: null as string | null,
    editingCommandId: null as string | null,
    
    contextMenuTargetSessionId: null as string | null,
    contextMenuTargetCommandId: null as string | null,
    contextMenuTargetFolder: null as string | null,
    contextMenuType: null as 'session' | 'command' | 'folder' | null,
    moveSessionId: null as string | null,
    moveCommandId: null as string | null,
    moveContext: 'session' as 'session' | 'command',
    loginSession: null as SavedSession | null,
    contextMenuTargetTabId: null as string | null,
    
    commandFolders: ['General'] as string[],
    sessionFolders: ['General'] as string[],
    expandedSessionFolders: new Set<string>(),
    expandedCommandFolders: new Set<string>(),
    folderContext: 'command' as 'session' | 'command',
    tooltipHideTimer: null as number | null,
    
    sftpSessionId: null as string | null,
    sftpCurrentPath: '/',
    sftpSelectedFile: null as SftpFileEntry | null,
    sftpConnectionParams: null as ConnectRequest | null,
    
    aiPanelOpen: false,
    aiMessages: [] as Array<{ role: 'user' | 'assistant'; content: string }>,
    aiProvider: 'gemini' as 'gemini' | 'grok' | 'ollama',
    masterPassword: null as string | null,
};
    