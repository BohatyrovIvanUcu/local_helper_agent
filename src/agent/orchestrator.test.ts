import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  runAgentTurn,
  stripToolExecutionNotice,
  detectDirectMediaCommand,
  type ModelClient,
} from "./orchestrator.ts";
import { createDefaultToolRegistry } from "./registry.ts";
import type { ConfirmationRequest } from "./types.ts";

describe("runAgentTurn", () => {
  const registry = createDefaultToolRegistry();

  it("stripToolExecutionNotice removes leading execution badges and brackets", () => {
    assert.equal(
      stripToolExecutionNotice("[Виконано: append_to_obsidian] Записав задачу!"),
      "Записав задачу!"
    );
    assert.equal(
      stripToolExecutionNotice("[append_to_obsidian]\nЗаписав задачу!"),
      "Записав задачу!"
    );
    assert.equal(
      stripToolExecutionNotice("[Executed: cli] [Result: ok] Готово"),
      "Готово"
    );
    assert.equal(
      stripToolExecutionNotice("Звичайний текст без дужок"),
      "Звичайний текст без дужок"
    );
    assert.equal(
      stripToolExecutionNotice(
        '<untrusted_web_content source="https://example.com">\nКонтент сторінки\n</untrusted_web_content>'
      ),
      "Контент сторінки"
    );
  });

  it("handles simple text response without tool calls", async () => {
    let callCount = 0;
    const fakeClient: ModelClient = {
      generateContent: async () => {
        callCount++;
        return {
          text: "Привіт! Чим можу допомогти на будівництві?",
          candidates: [
            {
              content: {
                parts: [{ text: "Привіт! Чим можу допомогти на будівництві?" }],
              },
            },
          ],
        };
      },
    };

    const res = await runAgentTurn({
      modelClient: fakeClient,
      modelName: "gemini-3.5-flash-lite",
      systemInstruction: "System prompt",
      userPrompt: "Привіт",
      toolRegistry: registry,
      invoke: (async <T>() => ({} as T)),
      callbacks: {
        onStateChange: () => {},
      },
    });

    assert.equal(callCount, 1);
    assert.equal(res.text, "Привіт! Чим можу допомогти на будівництві?");
    assert.equal(res.executedTools.length, 0);
  });

  it("executes tool call and makes 2nd LLM call for mascot reply (No directReply)", async () => {
    let callCount = 0;
    const invokedCommands: string[] = [];

    const fakeClient: ModelClient = {
      generateContent: async ({ contents }) => {
        callCount++;
        if (callCount === 1) {
          return {
            candidates: [
              {
                content: {
                  parts: [
                    {
                      functionCall: {
                        name: "control_media",
                        args: { action: "play_pause" },
                      },
                    },
                  ],
                },
              },
            ],
          };
        }
        // Second call with function response
        const lastMsg = contents[contents.length - 1] as { parts?: Array<Record<string, unknown>> };
        assert.ok(lastMsg.parts?.[0]?.functionResponse);
        return {
          text: "Вмикаю музику на повну, шефе! 🎶",
          candidates: [
            {
              content: {
                parts: [{ text: "Вмикаю музику на повну, шефе! 🎶" }],
              },
            },
          ],
        };
      },
    };

    const res = await runAgentTurn({
      modelClient: fakeClient,
      modelName: "gemini-3.5-flash-lite",
      systemInstruction: "System prompt",
      userPrompt: "Увімкни музику",
      toolRegistry: registry,
      invoke: async <T>(cmd: string): Promise<T> => {
        invokedCommands.push(cmd);
        return "Музику перемкнено." as unknown as T;
      },
      callbacks: {
        onStateChange: () => {},
      },
    });

    assert.equal(callCount, 2);
    assert.deepEqual(invokedCommands, ["control_media", "append_conversation_turn"]);
    assert.equal(res.text, "Вмикаю музику на повну, шефе! 🎶");
    assert.deepEqual(res.executedTools, ["control_media"]);
  });

  it("handles missing tool name safely without defaulting to append_to_obsidian", async () => {
    let callCount = 0;
    const fakeClient: ModelClient = {
      generateContent: async ({ contents }) => {
        callCount++;
        if (callCount === 1) {
          return {
            candidates: [
              {
                content: {
                  // Function call without name
                  parts: [{ functionCall: { args: { foo: "bar" } } }],
                },
              },
            ],
          };
        }
        const lastMsg = contents[contents.length - 1] as { parts?: Array<{ functionResponse?: { response?: { ok: boolean } } }> };
        assert.equal(lastMsg.parts?.[0]?.functionResponse?.response?.ok, false);
        return {
          text: "Ой, здається, я забув який інструмент взяти з пояса!",
          candidates: [
            {
              content: {
                parts: [{ text: "Ой, здається, я забув який інструмент взяти з пояса!" }],
              },
            },
          ],
        };
      },
    };

    const res = await runAgentTurn({
      modelClient: fakeClient,
      modelName: "gemini-3.5-flash-lite",
      systemInstruction: "System prompt",
      userPrompt: "Зроби щось",
      toolRegistry: registry,
      invoke: async () => {
        throw new Error("Should not be invoked!");
      },
      callbacks: {
        onStateChange: () => {},
      },
    });

    assert.equal(callCount, 2);
    assert.equal(res.executedTools.length, 0);
  });

  it("requests user confirmation for risky commands and handles rejection", async () => {
    let confirmCalled = false;
    let callCount = 0;

    const fakeClient: ModelClient = {
      generateContent: async ({ contents }) => {
        callCount++;
        if (callCount === 1) {
          return {
            candidates: [
              {
                content: {
                  parts: [
                    {
                      functionCall: {
                        name: "run_cli_command",
                        args: { command: "python3 dangerous.py" },
                      },
                    },
                  ],
                },
              },
            ],
          };
        }
        const lastMsg = contents[contents.length - 1] as { parts?: Array<{ functionResponse?: { response?: { code: string } } }> };
        assert.equal(lastMsg.parts?.[0]?.functionResponse?.response?.code, "user_cancelled");
        return {
          text: "Зрозумів, команду скасовано.",
          candidates: [
            { content: { parts: [{ text: "Зрозумів, команду скасовано." }] } },
          ],
        };
      },
    };

    const res = await runAgentTurn({
      modelClient: fakeClient,
      modelName: "gemini-3.5-flash-lite",
      systemInstruction: "System prompt",
      userPrompt: "Запусти скрипт",
      toolRegistry: registry,
      invoke: async () => {
        throw new Error("Should not invoke when cancelled!");
      },
      callbacks: {
        onStateChange: () => {},
        onConfirmRequest: async (req: ConfirmationRequest) => {
          confirmCalled = true;
          assert.equal(req.toolName, "run_cli_command");
          assert.equal(req.details, "python3 dangerous.py");
          return false; // User clicks Cancel
        },
      },
    });

    assert.equal(confirmCalled, true);
    assert.equal(callCount, 2);
    assert.equal(res.text, "Зрозумів, команду скасовано.");
  });

  it("detectDirectMediaCommand recognizes media control keywords", () => {
    assert.deepEqual(detectDirectMediaCommand("resume"), { action: "play" });
    assert.deepEqual(detectDirectMediaCommand("play!"), { action: "play" });
    assert.deepEqual(detectDirectMediaCommand("продовжи"), { action: "play" });
    assert.deepEqual(detectDirectMediaCommand("пауза"), { action: "pause" });
    assert.deepEqual(detectDirectMediaCommand("pause."), { action: "pause" });
    assert.deepEqual(detectDirectMediaCommand("наступний трек"), { action: "next" });
    assert.deepEqual(detectDirectMediaCommand("next"), { action: "next" });
    assert.deepEqual(detectDirectMediaCommand("попередній трек"), { action: "prev" });
    assert.deepEqual(detectDirectMediaCommand("prev"), { action: "prev" });
    assert.equal(detectDirectMediaCommand("що таке квантова фізика?"), null);
  });

  it("executes direct media command instantly without calling LLM", async () => {
    let llmCalled = false;
    const fakeClient: ModelClient = {
      generateContent: async () => {
        llmCalled = true;
        throw new Error("LLM should not be called for direct media commands!");
      },
    };

    const invokedCommands: Array<{ cmd: string; args: unknown }> = [];
    const res = await runAgentTurn({
      modelClient: fakeClient,
      modelName: "gemini-3.5-flash-lite",
      systemInstruction: "System prompt",
      userPrompt: "наступний трек",
      toolRegistry: registry,
      invoke: async <T>(cmd: string, args: unknown): Promise<T> => {
        invokedCommands.push({ cmd, args });
        return "Увімкнено наступний трек ⏭️" as unknown as T;
      },
      callbacks: {
        onStateChange: () => {},
      },
    });

    assert.equal(llmCalled, false);
    assert.equal(res.text, "Увімкнено наступний трек ⏭️");
    assert.deepEqual(res.executedTools, ["control_media"]);
    assert.equal(invokedCommands[0].cmd, "control_media");
    assert.deepEqual(invokedCommands[0].args, { action: "next", query: null });
  });
});
