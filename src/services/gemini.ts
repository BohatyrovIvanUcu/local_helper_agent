import { GoogleGenAI, type FunctionDeclaration } from "@google/genai";
import { invoke } from "@tauri-apps/api/core";
import { createDefaultToolRegistry } from "../agent/registry.ts";
import { composeSystemPrompt } from "../agent/prompt.ts";
import { runAgentTurn, type ModelClient } from "../agent/orchestrator.ts";
import type {
  MascotState,
  WorkingToolName,
  AskMascotCallbacks,
  ConfirmationRequest,
  AppConfig,
  AgentMemory,
  AgentMemoryMessage,
} from "../agent/types.ts";

export type {
  MascotState,
  WorkingToolName,
  AskMascotCallbacks,
  ConfirmationRequest,
  AppConfig,
  AgentMemory,
  AgentMemoryMessage as MemoryMessage,
};

export interface MediaAttachment {
  mimeType: string;
  base64Data: string;
  name?: string;
  previewUrl?: string;
}

export function formatGeminiError(err: unknown): string {
  const rawMsg = err instanceof Error ? err.message : String(err ?? "");
  const lower = rawMsg.toLowerCase();

  // 1. Помилка 503 або високе навантаження на сервери Google Gemini
  if (
    lower.includes("503") ||
    lower.includes("overloaded") ||
    lower.includes("high demand") ||
    lower.includes("service unavailable") ||
    lower.includes("temporarily unavailable") ||
    lower.includes("resource unavailable")
  ) {
    return "⏳ Сервери Gemini зараз під високим навантаженням (високий попит на модель).\nБудь ласка, зачекай кілька секунд і повтори запит!";
  }

  // 2. Ліміт запитів (429 Rate limit / Quota)
  if (
    lower.includes("429") ||
    lower.includes("resource_exhausted") ||
    lower.includes("quota") ||
    lower.includes("too many requests") ||
    lower.includes("rate limit")
  ) {
    return "🚦 Вичерпано ліміт запитів Gemini API (Rate limit / Quota).\nБудь ласка, зачекай хвилину або скористайся іншим ключем API.";
  }

  // 3. Недійсний або відсутній API-ключ
  if (
    lower.includes("401") ||
    lower.includes("403") ||
    lower.includes("api_key_invalid") ||
    lower.includes("api key not valid") ||
    lower.includes("unauthenticated") ||
    lower.includes("permission_denied") ||
    (lower.includes("not found") && lower.includes("key"))
  ) {
    return "🔑 Проблема з API-ключем Gemini.\nБудь ласка, перевір або онови ключ у терміналі:\nlocal-helper-ai addkey <ТВІЙ_КЛЮЧ>";
  }

  // 4. Мережеві помилки
  if (
    lower.includes("fetch failed") ||
    lower.includes("networkerror") ||
    lower.includes("failed to fetch") ||
    lower.includes("econnrefused") ||
    lower.includes("etimedout") ||
    lower.includes("connection reset")
  ) {
    return "🌐 Проблема зі з'єднанням з мережею.\nПеревір доступ до інтернету та повтори спробу.";
  }

  // Очищення загального технічного сміття
  const cleaned = rawMsg
    .replace(/^\[GoogleGenAI Error\]:\s*/i, "")
    .replace(/^\[GoogleGenerativeAI Error\]:\s*/i, "")
    .replace(/^Error:\s*/i, "")
    .trim();

  return cleaned || "Сталася непередбачена помилка. Спробуй ще раз.";
}

export async function getAppConfig(): Promise<AppConfig> {
  try {
    return await invoke<AppConfig>("get_app_config");
  } catch {
    return {
      gemini_api_key:
        (import.meta.env.DEV ? import.meta.env.VITE_GEMINI_API_KEY?.trim() : "") ||
        "",
      gemini_model:
        import.meta.env.VITE_GEMINI_MODEL?.trim() || "gemini-3.5-flash-lite",
      obsidian_vault_path: "",
      config_file_path: "",
    };
  }
}

export async function getAgentMemory(): Promise<AgentMemory> {
  try {
    return await invoke<AgentMemory>("get_agent_memory");
  } catch {
    return { facts: [], recent_messages: [] };
  }
}

export const defaultToolRegistry = createDefaultToolRegistry();

export const toolDeclarations: FunctionDeclaration[] =
  defaultToolRegistry.getFunctionDeclarations();

export async function askMascot(
  userPrompt: string,
  callbacks: AskMascotCallbacks,
  attachments: MediaAttachment[] = []
): Promise<string> {
  interface ActiveWorkspaceRaw {
    workspace_path?: string | null;
    active_file?: string | null;
    source?: string;
  }

  const [config, memory, activeWs] = await Promise.all([
    getAppConfig(),
    getAgentMemory(),
    invoke<ActiveWorkspaceRaw>("get_active_workspace").catch((err) => {
      console.warn("Failed to get active workspace:", err);
      return null;
    }),
  ]);

  const apiKey =
    config.gemini_api_key?.trim() ||
    (import.meta.env.DEV ? import.meta.env.VITE_GEMINI_API_KEY?.trim() : "") ||
    "";

  if (!apiKey) {
    throw new Error(
      "🔑 Не знайдено Gemini API ключ!\nБудь ласка, введіть у терміналі:\nlocal-helper-ai addkey <ВАШ_КЛЮЧ>"
    );
  }

  const effectiveModel =
    config.gemini_model?.trim() ||
    import.meta.env.VITE_GEMINI_MODEL?.trim() ||
    "gemini-3.5-flash-lite";

  const ai = new GoogleGenAI({ apiKey });

  const modelClient: ModelClient = {
    generateContent: async (params) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const response = await ai.models.generateContent(params as any);
      return {
        text: response.text ?? undefined,
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        candidates: response.candidates as any,
      };
    },
  };

  const rawAttachments = attachments.map((item) => ({
    inlineData: {
      mimeType: item.mimeType,
      data: item.base64Data,
    },
  }));

  const trimmedPrompt = userPrompt.trim();
  const hasAudio = attachments.some((a) => a.mimeType.startsWith("audio/"));
  const hasImage = attachments.some((a) => a.mimeType.startsWith("image/"));

  const effectiveText =
    trimmedPrompt ||
    (hasAudio && hasImage
      ? "Прослухай голосову інструкцію, проаналізуй прикріплене зображення та виконай потрібну дію."
      : hasAudio
      ? "Уважно прослухай голосове повідомлення користувача українською мовою та виконай запитану дію (наприклад, запиши задачу чи ідею в Obsidian або дай лаконічну відповідь)."
      : hasImage
      ? "Проаналізуй це зображення. Якщо на ньому задачі, дедлайни чи ідеї — додай їх у відповідну секцію Obsidian, інакше коротко поясни суть."
      : "");

  const systemInstruction = composeSystemPrompt({
    vaultPath: config.obsidian_vault_path,
    memoryFacts: memory.facts,
    userQuery: effectiveText,
    toolRegistry: defaultToolRegistry,
    activeWorkspace: activeWs
      ? {
          workspacePath: activeWs.workspace_path,
          activeFile: activeWs.active_file,
          source: activeWs.source,
        }
      : null,
  });

  const conversationHistory = memory.recent_messages
    .filter((msg) => {
      // Захист від галюцинацій: якщо стара репліка маскота стверджує про дію з музикою/звуком,
      // але tools_used порожній — виключаємо її з історії, щоб модель не копіювала відповіді текстом
      if (
        msg.role === "model" &&
        (!msg.tools_used || msg.tools_used.length === 0)
      ) {
        const lower = msg.text.toLowerCase();
        if (
          lower.includes("музик") ||
          lower.includes("пауз") ||
          lower.includes("вирубаю") ||
          lower.includes("вимикаю") ||
          lower.includes("запускаю") ||
          lower.includes("тиша в ефірі") ||
          lower.includes("наступн") ||
          lower.includes("попередн") ||
          lower.includes("яскравіст") ||
          lower.includes("нічний режим") ||
          lower.includes("тепле світло") ||
          lower.includes("теплий екран") ||
          lower.includes("нагадуван") ||
          lower.includes("таймер") ||
          lower.includes("записав у todo") ||
          lower.includes("додав у obsidian")
        ) {
          return false;
        }
      }
      return true;
    })
    .map((msg) => {
      // Очищаємо застарілі префікси на кшталт [Виконано: ...], щоб модель не копіювала їх у відповідь
      const cleanText =
        msg.role === "model"
          ? msg.text.replace(/^\[Виконано:[^\]]+\]\s*/i, "").trim()
          : msg.text;

      return {
        role: msg.role === "model" ? "model" : "user",
        parts: [{ text: cleanText }],
      };
    });

  const result = await runAgentTurn({
    modelClient,
    modelName: effectiveModel,
    systemInstruction,
    userPrompt: effectiveText,
    attachments: rawAttachments,
    conversationHistory,
    toolRegistry: defaultToolRegistry,
    invoke,
    callbacks,
    vaultPath: config.obsidian_vault_path,
    activeWorkspacePath: activeWs?.workspace_path,
    activeFile: activeWs?.active_file,
  });

  return result.text;
}
