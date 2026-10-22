import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { runAgentTurn, stripToolExecutionNotice, type ModelClient } from "./orchestrator.ts";
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
});
