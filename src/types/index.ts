import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';

export interface ConnectRequest {
  host: string;
  port: number;
  username: string;
  password: string;
  log_directory?: string;
  use_global_credentials?: boolean;
}

export interface ConnectResponse {
  session_id: string;
  success: boolean;
  error?: string;
}

export interface SavedSession {
  id: string;
  name: string;
  host: string;
  port: number;
  folder?: string;
  username?: string;
  log_directory?: string;
  use_global_credentials?: boolean;
}

export interface QuickCommand {
  id: string;
  name: string;
  command: string;
  folder: string;
  icon?: string;
  description?: string;
  example?: string;
}

export interface SftpFileEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
  modified?: number;
  permissions?: number;
}

export interface TabInfo {
  id: string;
  sessionId?: string;
  title: string;
  terminal: Terminal;
  fitAddon: FitAddon;
  connected: boolean;
  unlisteners: Array<() => void>;
  connectionParams: ConnectRequest | null;
  container: HTMLDivElement;
  onDataDisposable?: { dispose: () => void };
  groupId?: string;
}

export interface TabGroup {
  id: string;
  name: string;
  colorIndex: number;
  collapsed: boolean;
  tabIds: string[];
}

export interface AiConfig {
  gemini_api_key: string | null;
  grok_api_key: string | null;
  ollama_host: string;
  ollama_model: string;
}

export interface KnownHostEntry {
  name: string;
  key_type: string;
  fingerprint: string;
}
