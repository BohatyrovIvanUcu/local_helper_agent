import type {
  AskMascotCallbacks,
  InvokeFn,
  ToolContext,
  ToolOutcome,
  WorkingToolName,
} from "./types.ts";
import { ToolRegistry } from "./registry.ts";
import { evaluatePolicy } from "./policy.ts";

export interface ModelResponse {
  text?: string;
  functionCalls?: Array<{
    id?: string;
    name?: string;
    args?: Record<string, unknown>;
  }>;
  candidates?: Array<{ content?: Record<string, unknown> }>;
}

export interface ModelClient {
  generateContent: (params: {
    model: string;
    contents: Array<Record<string, unknown>>;
    config: Record<string, unknown>;
  }) => Promise<ModelResponse>;
}

export function stripToolExecutionNotice(text: string): string {
  if (!text) return "";
  let cleaned = text.trim();
  while (/^\[[^\]]+\]\s*/.test(cleaned)) {
    cleaned = cleaned.replace(/^\[[^\]]+\]\s*/, "").trim();
  }
  // Захист: системні XML-теги пісочниці ніколи не повинні з'являтися у хмарці маскота
  cleaned = cleaned.replace(/<\/?untrusted_web_content[^>]*>/gi, "").trim();
  return cleaned;
}

export function detectDirectMediaCommand(
  prompt: string
): { action: string; query?: string | null } | null {
  const clean = prompt.trim().toLowerCase().replace(/[!.?,;]+$/g, "").trim();

  if (
    [
      "пауза",
      "pause",
      "стоп",
      "stop",
      "зупини",
      "зупинити",
      "постав на паузу",
      "паузу",
      "на паузу",
      "зупини музику",
      "stop music",
    ].includes(clean)
  ) {
    return { action: "pause" };
  }

  if (
    [
      "resume",
      "продовжи",
      "продовжити",
      "продовжуй",
      "грай",
      "грати",
      "play",
      "плей",
      "віднови",
      "відновити",
      "старт",
      "продовжи музику",
      "продовжити музику",
      "грай далі",
    ].includes(clean)
  ) {
    return { action: "play" };
  }

  if (
    [
      "наступний",
      "наступний трек",
      "наступна",
      "наступна пісня",
      "наступну",
      "next",
      "next track",
      "скіп",
      "skip",
      "далі",
      "переключи",
      "перемкни",
      "включи наступний",
      "включи наступну",
    ].includes(clean)
  ) {
    return { action: "next" };
  }

  if (
    [
      "попередній",
      "попередній трек",
      "попередня",
      "попередня пісня",
      "попередню",
      "prev",
      "previous",
      "previous track",
      "назад",
      "включи попередній",
      "включи попередню",
    ].includes(clean)
  ) {
    return { action: "prev" };
  }

  return null;
}

export interface RunTurnOptions {
  modelClient: ModelClient;
  modelName: string;
  systemInstruction: string;
  userPrompt: string;
  attachments?: Array<Record<string, unknown>>;
  conversationHistory?: Array<Record<string, unknown>>;
  toolRegistry: ToolRegistry;
  invoke: InvokeFn;
  callbacks: AskMascotCallbacks;
  vaultPath?: string;
  activeWorkspacePath?: string | null;
  activeFile?: string | null;
  maxIterations?: number;
}

export interface TurnResult {
  text: string;
  executedTools: string[];
}

export async function runAgentTurn(options: RunTurnOptions): Promise<TurnResult> {
  const {
    modelClient,
    modelName,
    systemInstruction,
    userPrompt,
    attachments = [],
    conversationHistory = [],
    toolRegistry,
    invoke,
    callbacks,
    vaultPath,
    maxIterations = 5,
  } = options;

  // Пряме миттєве виконання медіа-команд (resume, pause, next, prev тощо) без затримки на LLM
  const directMedia = detectDirectMediaCommand(userPrompt);
  if (directMedia && attachments.length === 0) {
    callbacks.onStateChange("WORKING", "control_media");
    try {
      const summary = await invoke<string>("control_media", {
        action: directMedia.action,
        query: directMedia.query ?? null,
      });

      if (userPrompt.trim() && summary) {
        try {
          await invoke("append_conversation_turn", {
            user_text: userPrompt.trim(),
            model_text: summary,
            userText: userPrompt.trim(),
            modelText: summary,
          });
        } catch {
          // ignore memory error
        }
      }

      return {
        text: summary,
        executedTools: ["control_media"],
      };
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      return {
        text: `Помилка керування медіа: ${msg}`,
        executedTools: ["control_media"],
      };
    }
  }

  const ctx: ToolContext = {
    invoke,
    confirm: callbacks.onConfirmRequest,
    now: new Date(),
    vaultPath,
    tainted: false,
    activeWorkspacePath: options.activeWorkspacePath,
    activeFile: options.activeFile,
  };

  const executedToolsLog: string[] = [];

  // Формуємо початкові contents
  const contents: Array<Record<string, unknown>> = [...conversationHistory];

  const userParts: Array<Record<string, unknown>> = [];
  for (const att of attachments) {
    userParts.push(att);
  }
  if (userPrompt.trim()) {
    userParts.push({ text: userPrompt });
  }

  contents.push({
    role: "user",
    parts: userParts,
  });

  callbacks.onStateChange("THINKING");

  const functionDeclarations = toolRegistry.getFunctionDeclarations();
  let lastToolSummary: string | undefined;

  for (let iteration = 0; iteration < maxIterations; iteration++) {
    let response: ModelResponse;

    if (iteration > 0 && lastToolSummary) {
      // Для повторних кроків (генерація персонажної відповіді після виконання інструментів):
      // якщо Gemini затримується довше таймауту, повертаємо готовий результат виконаного інструменту.
      // Для читання/саммарі вебсторінок даємо достатньо часу (14 с) для якісного аналізу тексту.
      const isWebSummary = executedToolsLog.includes("summarize_webpage");
      const fallbackTimeoutMs = isWebSummary ? 14000 : 7000;
      const fallbackSummary = stripToolExecutionNotice(lastToolSummary);
      let timer: ReturnType<typeof setTimeout> | undefined;
      const timeoutPromise = new Promise<ModelResponse>((resolve) => {
        timer = setTimeout(() => {
          resolve({ text: fallbackSummary });
        }, fallbackTimeoutMs);
      });

      try {
        response = await Promise.race([
          modelClient.generateContent({
            model: modelName,
            contents,
            config: {
              systemInstruction: {
                parts: [{ text: systemInstruction }],
              },
              tools: [{ functionDeclarations }],
              temperature: 0.2,
            },
          }),
          timeoutPromise,
        ]);
      } finally {
        if (timer) clearTimeout(timer);
      }
    } else {
      response = await modelClient.generateContent({
        model: modelName,
        contents,
        config: {
          systemInstruction: {
            parts: [{ text: systemInstruction }],
          },
          tools: [{ functionDeclarations }],
          temperature: 0.2,
        },
      });
    }

    const candidate = response.candidates?.[0]?.content as
      | { parts?: Array<Record<string, unknown>> }
      | undefined;
    const parts = candidate?.parts || [];

    const functionCalls: Array<{
      id?: string;
      name?: string;
      args?: Record<string, unknown>;
    }> = [];

    let textResponse = "";

    for (const part of parts) {
      if (part.functionCall) {
        functionCalls.push(
          part.functionCall as {
            id?: string;
            name?: string;
            args?: Record<string, unknown>;
          }
        );
      }
      if (typeof part.text === "string") {
        textResponse += part.text;
      }
    }

    // Якщо модель не викликає жодного інструменту — отримано фінальну текстову відповідь
    if (functionCalls.length === 0) {
      const rawText = textResponse.trim() || response.text?.trim() || "";
      const finalText = stripToolExecutionNotice(rawText);

      // Зберігаємо діалог у пам'ять
      if (userPrompt.trim() && finalText) {
        try {
          await invoke("append_conversation_turn", {
            user_text: userPrompt.trim(),
            model_text: finalText,
            userText: userPrompt.trim(),
            modelText: finalText,
          });
        } catch {
          // Помилка запису пам'яті не блокує видачу відповіді
        }
      }

      return {
        text: finalText,
        executedTools: executedToolsLog,
      };
    }

    // Додаємо виклики моделі в історію ходу
    contents.push({
      role: "model",
      parts: candidate?.parts || functionCalls.map((fc) => ({ functionCall: fc })),
    });

    const responseParts: Array<Record<string, unknown>> = [];

    for (const call of functionCalls) {
      if (!call.name) {
        responseParts.push({
          functionResponse: {
            id: call.id,
            name: "unknown_tool",
            response: {
              ok: false,
              code: "unknown_tool",
              result: "Помилка: модель не вказала назву інструменту.",
            },
          },
        });
        continue;
      }

      const toolSpec = toolRegistry.get(call.name);
      if (!toolSpec) {
        responseParts.push({
          functionResponse: {
            id: call.id,
            name: call.name,
            response: {
              ok: false,
              code: "unknown_tool",
              result: `Невідомий інструмент: ${call.name}`,
            },
          },
        });
        continue;
      }

      const rawArgs = call.args || {};
      const validation = toolSpec.validateArgs(rawArgs);
      if (!validation.ok) {
        responseParts.push({
          functionResponse: {
            id: call.id,
            name: toolSpec.name,
            response: {
              ok: false,
              code: "validation_error",
              result: validation.error,
            },
          },
        });
        continue;
      }

      // Перевірка політики безпеки
      const policyDecision = evaluatePolicy(toolSpec.name, validation.args, ctx);

      if (policyDecision.decision === "deny") {
        responseParts.push({
          functionResponse: {
            id: call.id,
            name: toolSpec.name,
            response: {
              ok: false,
              code: "permission_denied",
              result: policyDecision.reason,
            },
          },
        });
        continue;
      }

      if (policyDecision.decision === "confirm") {
        if (!ctx.confirm) {
          responseParts.push({
            functionResponse: {
              id: call.id,
              name: toolSpec.name,
              response: {
                ok: false,
                code: "permission_denied",
                result:
                  "Потрібне підтвердження користувача, але інтерфейс підтвердження недоступний.",
              },
            },
          });
          continue;
        }

        const approved = await ctx.confirm({
          toolName: toolSpec.name,
          title: policyDecision.title,
          details: policyDecision.details,
          risk: policyDecision.risk,
        });

        if (!approved) {
          responseParts.push({
            functionResponse: {
              id: call.id,
              name: toolSpec.name,
              response: {
                ok: false,
                code: "user_cancelled",
                result: "Дію скасовано користувачем.",
              },
            },
          });
          continue;
        }
      }

      // Виконання інструменту
      callbacks.onStateChange("WORKING", toolSpec.name as WorkingToolName);

      const timeoutMs = toolSpec.timeoutMs ?? 15000;
      let outcome: ToolOutcome;

      try {
        let timer: ReturnType<typeof setTimeout> | undefined;
        const timeoutPromise = new Promise<ToolOutcome>((resolve) => {
          timer = setTimeout(() => {
            resolve({
              ok: false,
              code: "timeout",
              summary: `Таймаут виконання ${toolSpec.name} (${timeoutMs} мс).`,
            });
          }, timeoutMs);
        });

        try {
          outcome = await Promise.race([
            toolSpec.execute(validation.args, ctx),
            timeoutPromise,
          ]);
        } finally {
          if (timer) clearTimeout(timer);
        }
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        outcome = {
          ok: false,
          code: "execution_error",
          summary: `Помилка виконання ${toolSpec.name}: ${msg}`,
        };
      }

      executedToolsLog.push(toolSpec.name);
      lastToolSummary = outcome.summary;

      if (outcome.untrustedContent) {
        ctx.tainted = true;
      }

      responseParts.push({
        functionResponse: {
          id: call.id,
          name: toolSpec.name,
          response: {
            ok: outcome.ok,
            code: outcome.code ?? (outcome.ok ? "ok" : "execution_error"),
            result: outcome.summary,
            ...(outcome.data ? { data: outcome.data } : {}),
          },
        },
      });
    }

    // Передаємо результати інструментів назад у модель для генерації персонажної відповіді
    contents.push({
      role: "user",
      parts: responseParts,
    });

    callbacks.onStateChange("THINKING", undefined);
  }

  return {
    text: "Дію виконано, але досягнуто ліміт кроків оркестратора.",
    executedTools: executedToolsLog,
  };
}

