import type { FunctionDeclaration } from "@google/genai";

export type RiskLevel = "read" | "write" | "exec" | "external";

export type ToolErrorCode =
  | "validation_error"
  | "permission_denied"
  | "user_cancelled"
  | "not_found"
  | "timeout"
  | "execution_error"
  | "unknown_tool";

export interface ToolOutcome {
  ok: boolean;
  code?: ToolErrorCode;
  /** Стислий підсумок для UI / системних логів */
  summary: string;
  /** Структуровані дані для functionResponse у Gemini */
  data?: Record<string, unknown>;
  /** Чи містить відповідь зовнішній недовірений контент (наприклад, вебсторінка) */
  untrustedContent?: boolean;
  /** Якщо true, виклик можна безпечно повторити (для ідемпотентних операцій читання) */
  retryable?: boolean;
}

export type InvokeFn = <T = unknown>(
  cmd: string,
  args?: Record<string, unknown>
) => Promise<T>;

export interface ConfirmationRequest {
  toolName: string;
  title: string;
  details: string;
  risk: RiskLevel;
}

export type ConfirmFn = (req: ConfirmationRequest) => Promise<boolean>;

export interface ToolContext {
  invoke: InvokeFn;
  confirm?: ConfirmFn;
  now: Date;
  vaultPath?: string;
  homeDir?: string;
  tainted: boolean;
  activeWorkspacePath?: string | null;
  activeFile?: string | null;
}

export interface ToolSpec<TArgs = Record<string, unknown>> {
  name: string;
  uiLabel: string;
  risk: RiskLevel;
  idempotent: boolean;
  timeoutMs?: number;
  declaration: FunctionDeclaration;
  /** Лаконічне правило (1–2 речення) для системного промпта */
  promptRule: string;
  /** Валідація та нормалізація аргументів від LLM */
  validateArgs: (raw: Record<string, unknown>) =>
    | { ok: true; args: TArgs }
    | { ok: false; error: string };
  execute: (args: TArgs, ctx: ToolContext) => Promise<ToolOutcome>;
}

export type WorkingToolName =
  | "append_to_obsidian"
  | "create_or_update_file"
  | "read_file_or_dir"
  | "run_cli_command"
  | "control_media"
  | "control_display"
  | "manage_memory"
  | "manage_reminders"
  | "summarize_webpage"
  | "convert_file"
  | "open_disk_analyzer"
  | "share_file_wifi"
  | "clean_downloads";

export type MascotState = "IDLE" | "THINKING" | "WORKING" | "SUCCESS" | "ERROR";

export interface AskMascotCallbacks {
  onStateChange: (state: MascotState, tool?: WorkingToolName) => void;
  onBubbleMessage?: (message: string) => void;
  onConfirmRequest?: ConfirmFn;
}

export interface AgentMemoryMessage {
  role: "user" | "model";
  text: string;
  timestamp: string;
  tools_used?: string[];
}

export interface AgentMemory {
  facts: string[];
  recent_messages: AgentMemoryMessage[];
}

export interface AppConfig {
  gemini_api_key: string;
  gemini_model: string;
  obsidian_vault_path: string;
  config_file_path: string;
}
