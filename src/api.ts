import { invoke } from "@tauri-apps/api/core";

export type AccessMode = "chat" | "plan" | "ask" | "acceptEdits" | "auto" | "full";

export interface RewindPreview {
  changedFiles: number;
  commandsLeftEffects: boolean;
}

export interface RewindOutcome {
  previousLeaf: string | null;
  restoredFiles: number;
  notRestored: string[];
}

export interface ChatSummary {
  id: string;
  title: string;
  source: "local" | "claude_code" | "codex";
  cwd: string | null;
  modelRef: string | null;
  createdAt: number;
  updatedAt: number;
  pinned: boolean;
  accessMode: AccessMode;
  sourcePath: string | null;
}

export interface MemoryFile {
  source: "tarn" | "claude" | "codex";
  title: string;
  path: string;
  text: string;
  wasCut: boolean;
}

export interface OwnMemory {
  shared: string;
  projectText: string | null;
  projectFolder: string | null;
}

export interface Artifact {
  id: string;
  chatId: string;
  messageId: string;
  title: string;
  kind: string;
  path: string;
  size: number;
  createdAt: number;
}

export interface PlanStep {
  text: string;
  status: "pending" | "in_progress" | "done";
}

export interface BackgroundTask {
  id: string;
  chatId: string;
  name: string;
  command: string;
  startedAt: number;
  finishedAt: number | null;
  exitCode: number | null;
  wasStopped: boolean;
}

export interface TerminalView {
  text: string;
  folder: string;
  isRunning: boolean;
}

export interface BrowserView {
  url: string;
  title: string;
  isLoading: boolean;
  activity: string;
}

export interface PanelBounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface FolderEntry {
  name: string;
  relativePath: string;
  isFolder: boolean;
  size: number;
}

export interface ChangedFile {
  path: string;
  relativePath: string;
  isNew: boolean;
  isDeleted: boolean;
  added: number;
  removed: number;
}

export interface DiffLine {
  kind: "same" | "added" | "removed";
  text: string;
}

export interface FileView {
  path: string;
  text: string | null;
  diff: DiffLine[] | null;
  problem: string | null;
}

export interface ContextPart {
  kind: "instruction" | "memory" | "tools" | "summary" | "messages" | "images";
  title: string;
  path: string | null;
  tokens: number;
}

export interface ContextUsage {
  parts: ContextPart[];
  usedTokens: number;
  compactAtTokens: number | null;
  modelLimitTokens: number | null;
  isCompacted: boolean;
  compactModelRef: string;
}

export interface ClaudeCodeDelivery {
  sessionId: string;
  cwd: string;
  filePath: string;
  backupPath: string | null;
  writtenHash: string;
  listedInClient: boolean;
}

export interface ToolCallPart {
  type: "tool_call";
  id?: string;
  name: string;
  input: string;
  result?: string;
  isError?: boolean;
  images?: Attachment[];
  status?: "waiting" | "running" | "done" | "denied";
  risk?: string;
  folder?: string | null;
  artifact?: Artifact;
  textOffset?: number;
}

export interface TextPart {
  type: "text";
  text: string;
}

export type MessagePart = ToolCallPart | TextPart;

export interface Attachment {
  name: string;
  mime: string;
  kind: "text" | "image";
  size: number;
  text?: string;
  dataBase64?: string;
  imageId?: string;
}

export interface Message {
  id: string;
  chatId: string;
  parentId: string | null;
  role: "user" | "assistant";
  content: string;
  parts: MessagePart[];
  thinking: string | null;
  provider: string | null;
  model: string | null;
  inputTokens: number | null;
  outputTokens: number | null;
  cost: number | null;
  status: "done" | "streaming" | "error" | "cancelled" | "interrupted";
  error: string | null;
  note: string | null;
  attachments: Attachment[];
  imported: boolean;
  compareGroup: string | null;
  createdAt: number;
  siblingIds: string[];
  drafting?: string;
}

export interface ThreadPage {
  messages: Message[];
  hasMore: boolean;
}

export interface SearchHit {
  chatId: string;
  messageId: string | null;
  title: string;
  source: ChatSummary["source"];
  snippet: string | null;
  updatedAt: number;
}

export interface ProviderInfo {
  id: string;
  name: string;
  protocol: string;
  baseUrl: string;
  requiresKey: boolean;
  isLocal: boolean;
  isCustom: boolean;
  hasKey: boolean;
  connected: boolean;
  modelCount: number;
}

export interface ModelInfo {
  provider: string;
  id: string;
  name: string;
  maxOutput: number | null;
  maxInput: number | null;
  effortLevels: string[];
}

export interface AddKeyResult {
  providerId: string;
  providerName: string;
  modelCount: number;
}

export interface UsageSummary {
  todayCost: number;
  monthCost: number;
  monthInputTokens: number;
  monthOutputTokens: number;
  budget: number | null;
  byProvider: { provider: string; cost: number; inputTokens: number; outputTokens: number }[];
  unpricedMessages: number;
}

export interface RequestLogEntry {
  id: number;
  createdAt: number;
  host: string;
  provider: string;
  model: string;
  bytesSent: number;
  bytesReceived: number;
  status: string;
  messageCount: number;
  files: string;
}

export interface PriceEntry {
  model: string;
  inputPerMillion: number;
  outputPerMillion: number;
}

export interface ImportSource {
  source: "claude_code" | "codex";
  path: string;
  exists: boolean;
  fileCount: number;
  totalBytes: number;
  importedChats: number;
}

export interface ImportProgress {
  source: string;
  filesDone: number;
  filesTotal: number;
  chatsImported: number;
  finished: boolean;
  failedFiles: number;
  background: boolean;
}

export interface SendOutcome {
  chat: ChatSummary;
  userMessage: Message | null;
  assistantMessages: Message[];
}

export interface TextRun {
  text: string;
  bold?: boolean;
  italic?: boolean;
  underline?: boolean;
  strike?: boolean;
  spoiler?: boolean;
  code?: boolean;
  quote?: boolean;
  link?: string;
}

export interface SimMedia {
  kind: string;
  fileId: string;
  name: string;
  url: string | null;
  hasBytes: boolean;
}

export interface SimButton {
  text: string;
  data: string | null;
  url: string | null;
}

export interface SimMessage {
  id: number;
  fromBot: boolean;
  runs: TextRun[];
  media: SimMedia | null;
  buttons: SimButton[][];
  date: number;
  edited: boolean;
  forwarded: boolean;
}

export interface SimLogEntry {
  serial: number;
  at: number;
  method: string;
  detail: string;
  problem: string | null;
  fromUser: boolean;
}

export interface SimView {
  running: boolean;
  address: string | null;
  lastCallAt: number | null;
  webhook: string | null;
  pending: number;
  messages: SimMessage[];
  keyboard: string[][] | null;
  commands: { command: string; description: string }[];
  typingUntil: number;
  notice: { serial: number; text: string; alert: boolean } | null;
  log: SimLogEntry[];
}

export interface StreamPayload {
  messageId: string;
  kind: "delta" | "thinking" | "parts" | "done" | "failed" | "tool";
  text: string;
  message: Message | null;
}

export interface SendRequest {
  chatId: string | null;
  parentId: string | null;
  text: string;
  attachments: Attachment[];
  modelRefs: string[];
  accessMode: AccessMode | null;
  folder: string | null;
}

export type ToolAnswer = "once" | "chat" | "deny";

export interface Skill {
  id: string;
  name: string;
  description: string;
  source: string;
  folder: string;
  enabled: boolean;
}

export interface Plugin {
  id: string;
  name: string;
  description: string;
  enabled: boolean;
  skillCount: number;
  serverNames: string[];
}

export interface McpServerStatus {
  name: string;
  source: string;
  target: string;
  enabled: boolean;
}

export interface VoiceStatus {
  isReady: boolean;
  isEngineInstalled: boolean;
}

export interface RecognitionRequest {
  wavBase64: string;
  isLive: boolean;
}

export interface VoiceProgress {
  stage: "engine" | "model";
  doneBytes: number;
  totalBytes: number;
}

export const api = {
  showMainWindow: () => invoke<void>("show_main_window"),
  listChats: () => invoke<ChatSummary[]>("list_chats"),
  getThread: (chatId: string, limit: number, beforeId: string | null = null, ensureId: string | null = null) =>
    invoke<ThreadPage>("get_thread", { chatId, limit, beforeId, ensureId }),
  renameChat: (chatId: string, title: string) => invoke<void>("rename_chat", { chatId, title }),
  setChatPinned: (chatId: string, pinned: boolean) => invoke<void>("set_chat_pinned", { chatId, pinned }),
  deleteChat: (chatId: string) => invoke<void>("delete_chat", { chatId }),
  switchBranch: (chatId: string, messageId: string) => invoke<void>("switch_branch", { chatId, messageId }),
  sendMessage: (request: SendRequest) => invoke<SendOutcome>("send_message", { request }),
  regenerateMessage: (messageId: string, modelRef: string) => invoke<SendOutcome>("regenerate_message", { messageId, modelRef }),
  editMessage: (messageId: string, text: string, modelRef: string) => invoke<SendOutcome>("edit_message", { messageId, text, modelRef }),
  cancelGeneration: (messageId: string) => invoke<void>("cancel_generation", { messageId }),
  search: (query: string) => invoke<SearchHit[]>("search", { query }),
  listProviders: () => invoke<ProviderInfo[]>("list_providers"),
  addKey: (key: string, providerId: string | null = null) => invoke<AddKeyResult>("add_key", { key, providerId }),
  deleteKey: (providerId: string) => invoke<void>("delete_key", { providerId }),
  addCustomProvider: (request: { name: string; baseUrl: string; protocol: string; key: string | null }) =>
    invoke<AddKeyResult>("add_custom_provider", { request }),
  deleteCustomProvider: (providerId: string) => invoke<void>("delete_custom_provider", { providerId }),
  refreshModels: () => invoke<Record<string, string>>("refresh_models"),
  listModels: () => invoke<ModelInfo[]>("list_models"),
  readAttachment: (path: string) => invoke<Attachment>("read_attachment", { path }),
  readImage: (imageId: string) => invoke<string>("read_image", { imageId }),
  getSettings: () => invoke<Record<string, string>>("get_settings"),
  setSetting: (key: string, value: string) => invoke<void>("set_setting", { key, value }),
  usageSummary: () => invoke<UsageSummary>("usage_summary"),
  requestLog: () => invoke<RequestLogEntry[]>("request_log"),
  clearRequestLog: () => invoke<void>("clear_request_log"),
  listPrices: () => invoke<PriceEntry[]>("list_prices"),
  setPrice: (model: string, input: number | null, output: number | null) => invoke<void>("set_price", { model, input, output }),
  scanImportSources: () => invoke<ImportSource[]>("scan_import_sources"),
  startImport: (sources: { source: string; path: string }[]) => invoke<void>("start_import", { sources }),
  exportChat: (chatId: string, path: string) => invoke<void>("export_chat", { chatId, path }),
  setChatAccess: (chatId: string, accessMode: AccessMode, folder: string | null) => invoke<ChatSummary>("set_chat_access", { chatId, accessMode, folder }),
  answerToolRequest: (messageId: string, callId: string, answer: ToolAnswer) => invoke<void>("answer_tool_request", { messageId, callId, answer }),
  listSkills: () => invoke<Skill[]>("list_skills"),
  setSkillEnabled: (id: string, enabled: boolean) => invoke<void>("set_skill_enabled", { id, enabled }),
  addSkillFolder: (folder: string) => invoke<number>("add_skill_folder", { folder }),
  listPlugins: () => invoke<Plugin[]>("list_plugins"),
  setPluginEnabled: (id: string, enabled: boolean) => invoke<void>("set_plugin_enabled", { id, enabled }),
  listMcpServers: () => invoke<McpServerStatus[]>("list_mcp_servers"),
  checkMcpServer: (name: string) => invoke<number>("check_mcp_server", { name }),
  addMcpServer: (name: string, target: string) => invoke<void>("add_mcp_server", { name, target }),
  removeMcpServer: (name: string) => invoke<void>("remove_mcp_server", { name }),
  setMcpServerEnabled: (name: string, enabled: boolean) => invoke<void>("set_mcp_server_enabled", { name, enabled }),
  voiceStatus: () => invoke<VoiceStatus>("voice_status"),
  installVoice: () => invoke<VoiceStatus>("install_voice"),
  prepareSpeech: () => invoke<void>("prepare_speech"),
  transcribeAudio: (request: RecognitionRequest) => invoke<string>("transcribe_audio", { request }),
  releaseSpeech: () => invoke<void>("release_speech"),
  forkChat: (chatId: string, messageId: string | null) => invoke<ChatSummary>("fork_chat", { chatId, messageId }),
  syncChat: (chatId: string) => invoke<number>("sync_chat", { chatId }),
  syncAllChats: () => invoke<void>("sync_all_chats"),
  contextUsage: (chatId: string, modelRef: string) => invoke<ContextUsage>("context_usage", { chatId, modelRef }),
  compactChat: (chatId: string, modelRef: string) => invoke<string | null>("compact_chat", { chatId, modelRef }),
  memoryFiles: (chatId: string | null) => invoke<MemoryFile[]>("memory_files", { chatId }),
  ownMemory: (chatId: string | null) => invoke<OwnMemory>("own_memory", { chatId }),
  saveOwnMemory: (place: "shared" | "project", chatId: string | null, text: string) => invoke<void>("save_own_memory", { place, chatId, text }),
  sendToClaudeCode: (chatId: string, replaceOriginal: boolean) => invoke<ClaudeCodeDelivery>("send_to_claude_code", { chatId, replaceOriginal }),
  undoClaudeCodeReplace: (delivery: ClaudeCodeDelivery) => invoke<void>("undo_claude_code_replace", { delivery }),
  rewindPreview: (messageId: string) => invoke<RewindPreview>("rewind_preview", { messageId }),
  rewindChat: (messageId: string, restoreFiles: boolean) => invoke<RewindOutcome>("rewind_chat", { messageId, restoreFiles }),
  undoFileRestore: (chatId: string) => invoke<string[]>("undo_file_restore", { chatId }),
  backgroundTasks: (chatId: string) => invoke<BackgroundTask[]>("background_tasks", { chatId }),
  backgroundOutput: (taskId: string) => invoke<string>("background_output", { taskId }),
  stopBackgroundTask: (taskId: string) => invoke<void>("stop_background_task", { taskId }),
  terminalView: (chatId: string) => invoke<TerminalView>("terminal_view", { chatId }),
  terminalRun: (chatId: string, line: string) => invoke<void>("terminal_run", { chatId, line }),
  terminalStop: (chatId: string) => invoke<void>("terminal_stop", { chatId }),
  terminalClear: (chatId: string) => invoke<void>("terminal_clear", { chatId }),
  browserPlace: (bounds: PanelBounds | null, visible: boolean) => invoke<void>("browser_place", { bounds, visible }),
  browserGo: (target: string) => invoke<void>("browser_go", { target }),
  browserView: () => invoke<BrowserView>("browser_view"),
  listFolder: (chatId: string, relative: string) => invoke<FolderEntry[]>("list_folder", { chatId, relative }),
  changedFiles: (chatId: string) => invoke<ChangedFile[]>("changed_files", { chatId }),
  fileView: (chatId: string, path: string) => invoke<FileView>("file_view", { chatId, path }),
  chatPlan: (chatId: string) => invoke<PlanStep[]>("chat_plan", { chatId }),
  chatArtifacts: (chatId: string) => invoke<Artifact[]>("chat_artifacts", { chatId }),
  artifactText: (artifactId: string) => invoke<string>("artifact_text", { artifactId }),
  saveArtifactCopy: (artifactId: string, target: string) => invoke<void>("save_artifact_copy", { artifactId, target }),
  openArtifactInBrowser: (artifactId: string) => invoke<void>("open_artifact_in_browser", { artifactId }),
  widgetPage: (html: string) => invoke<string>("widget_page", { html }),
  saveWidgetPicture: (target: string, pngBase64: string) => invoke<void>("save_widget_picture", { target, pngBase64 }),
  botSimStart: () => invoke<SimView>("bot_sim_start"),
  botSimView: () => invoke<SimView>("bot_sim_view"),
  botSimSend: (text: string) => invoke<void>("bot_sim_send", { text }),
  botSimSendPhoto: (path: string, caption: string) => invoke<void>("bot_sim_send_photo", { path, caption }),
  botSimPress: (messageId: number, data: string) => invoke<void>("bot_sim_press", { messageId, data }),
  botSimReset: () => invoke<void>("bot_sim_reset"),
  botSimStop: () => invoke<void>("bot_sim_stop"),
};
