import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  runAgentTurn,
  stripToolExecutionNotice,
  detectDirectMediaCommand,
  detectDirectDisplayCommand,
  detectDirectConvertCommand,
  detectDirectDiskCommand,
  detectDirectShareCommand,
  detectDirectDownloadsCommand,
  detectDirectPresetCommand,
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
      userPrompt: "Знайди та увімкни мені трек для фокусу",
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
    assert.deepEqual(detectDirectMediaCommand("next song"), { action: "next" });
    assert.deepEqual(detectDirectMediaCommand("skip song"), { action: "next" });
    assert.deepEqual(detectDirectMediaCommand("скіпни трек"), { action: "next" });
    assert.deepEqual(detectDirectMediaCommand("перемкни пісню"), { action: "next" });
    assert.deepEqual(detectDirectMediaCommand("попередній трек"), { action: "prev" });
    assert.deepEqual(detectDirectMediaCommand("prev"), { action: "prev" });
    assert.deepEqual(detectDirectMediaCommand("prev song"), { action: "prev" });
    assert.deepEqual(detectDirectMediaCommand("тихіше"), { action: "volume_down" });
    assert.deepEqual(detectDirectMediaCommand("тиішхе"), { action: "volume_down" });
    assert.deepEqual(detectDirectMediaCommand("зроби гучніше"), { action: "volume_up" });
    assert.deepEqual(detectDirectMediaCommand("зробигучнітсь на 0"), {
      action: "set_volume",
      query: "0",
    });
    assert.deepEqual(detectDirectMediaCommand("зроби гучніість на 0"), {
      action: "set_volume",
      query: "0",
    });
    assert.deepEqual(detectDirectMediaCommand("гучність 50%"), {
      action: "set_volume",
      query: "50",
    });
    assert.deepEqual(detectDirectMediaCommand("вимкни звук"), { action: "mute" });
    assert.deepEqual(detectDirectMediaCommand("музику сер"), { action: "play" });
    assert.deepEqual(detectDirectMediaCommand("включи музику будь ласка"), { action: "play" });
    assert.deepEqual(detectDirectMediaCommand("пж дай музики"), { action: "play" });
    assert.deepEqual(detectDirectMediaCommand("хочу музику"), { action: "play" });
    assert.deepEqual(detectDirectMediaCommand("офни музику пж"), { action: "pause" });
    assert.deepEqual(detectDirectMediaCommand("можеш потушити музику треба тиші"), { action: "pause" });
    assert.deepEqual(detectDirectMediaCommand("потуши музику"), { action: "pause" });
    assert.deepEqual(detectDirectMediaCommand("треба тиші"), { action: "pause" });
    assert.deepEqual(detectDirectMediaCommand("хочу тиші"), { action: "pause" });
    assert.deepEqual(detectDirectMediaCommand("офффни музику до біса"), { action: "pause" });
    assert.deepEqual(detectDirectMediaCommand("потуши музику нафіг"), { action: "pause" });
    assert.deepEqual(detectDirectMediaCommand("стоооп"), { action: "pause" });
    assert.deepEqual(detectDirectMediaCommand("врууубай музику блін"), { action: "play" });
    assert.deepEqual(detectDirectMediaCommand("включи те що я люблю слухати"), {
      action: "play",
      query: "liked",
    });
    assert.deepEqual(detectDirectMediaCommand("включи мої треки"), {
      action: "play",
      query: "liked",
    });
    assert.deepEqual(detectDirectMediaCommand("мої лайкнуті"), {
      action: "play",
      query: "liked",
    });
    assert.equal(detectDirectMediaCommand("запусти скрипт"), null);
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

  it("detectDirectDisplayCommand recognizes display and night light keywords", () => {
    assert.deepEqual(detectDirectDisplayCommand("увімкни нічний режим"), {
      action: "enable_night_light",
    });
    assert.deepEqual(detectDirectDisplayCommand("тепле світло"), {
      action: "enable_night_light",
    });
    assert.deepEqual(detectDirectDisplayCommand("вимкни нічний режим"), {
      action: "disable_night_light",
    });
    assert.deepEqual(detectDirectDisplayCommand("зроби яскравіше"), {
      action: "increase_brightness",
      value: 10,
    });
    assert.deepEqual(detectDirectDisplayCommand("темніше"), {
      action: "decrease_brightness",
      value: 10,
    });
    assert.deepEqual(detectDirectDisplayCommand("яскравість 50%"), {
      action: "set_brightness",
      value: 50,
    });
    assert.deepEqual(detectDirectDisplayCommand("зменш яскравість до 30%"), {
      action: "set_brightness",
      value: 30,
    });
    assert.deepEqual(detectDirectDisplayCommand("зроби теплий екран"), {
      action: "enable_night_light",
    });
    assert.deepEqual(detectDirectDisplayCommand("холодний екран"), {
      action: "disable_night_light",
    });
    assert.deepEqual(detectDirectDisplayCommand("чскравіть на 70"), {
      action: "set_brightness",
      value: 70,
    });
    assert.deepEqual(detectDirectDisplayCommand("зроби екран темніше"), {
      action: "decrease_brightness",
      value: 10,
    });
    assert.deepEqual(detectDirectDisplayCommand("зроби екран світліше"), {
      action: "increase_brightness",
      value: 10,
    });
    assert.deepEqual(detectDirectDisplayCommand("я відійшов"), {
      action: "lock_screen",
    });
    assert.deepEqual(detectDirectDisplayCommand("відійшов"), {
      action: "lock_screen",
    });
    assert.deepEqual(detectDirectDisplayCommand("заблокуй екран"), {
      action: "lock_screen",
    });
    assert.deepEqual(detectDirectDisplayCommand("lock screen"), {
      action: "lock_screen",
    });
    assert.deepEqual(detectDirectDisplayCommand("kill switch"), {
      action: "lock_screen",
    });
    assert.deepEqual(detectDirectDisplayCommand("я пішов"), {
      action: "lock_screen",
    });
    assert.equal(detectDirectDisplayCommand("розкажи анекдот"), null);
  });

  it("executes direct display command instantly without calling LLM", async () => {
    let llmCalled = false;
    const fakeClient: ModelClient = {
      generateContent: async () => {
        llmCalled = true;
        throw new Error("LLM should not be called for direct display commands!");
      },
    };

    const invokedCommands: Array<{ cmd: string; args: unknown }> = [];
    const res = await runAgentTurn({
      modelClient: fakeClient,
      modelName: "gemini-3.5-flash-lite",
      systemInstruction: "System prompt",
      userPrompt: "зменш яскравість до 30%",
      toolRegistry: registry,
      invoke: async <T>(cmd: string, args: unknown): Promise<T> => {
        invokedCommands.push({ cmd, args });
        return "Встановив яскравість на 30% 🔆" as unknown as T;
      },
      callbacks: {
        onStateChange: () => {},
      },
    });

    assert.equal(llmCalled, false);
    assert.equal(res.text, "Встановив яскравість на 30% 🔆");
    assert.deepEqual(res.executedTools, ["control_display"]);
    assert.equal(invokedCommands[0].cmd, "control_display");
    assert.deepEqual(invokedCommands[0].args, {
      action: "set_brightness",
      value: 30,
      enabled: null,
      temperature: null,
    });
  });

  it("detectDirectConvertCommand recognizes file conversion requests", () => {
    assert.deepEqual(
      detectDirectConvertCommand("зроби gif", "/home/user/video.mp4"),
      {
        inputPath: "/home/user/video.mp4",
        targetFormat: "gif",
      }
    );
    assert.deepEqual(
      detectDirectConvertCommand("конвертуй у mp3", "/home/user/audio.m4a"),
      {
        inputPath: "/home/user/audio.m4a",
        targetFormat: "mp3",
      }
    );
    assert.deepEqual(
      detectDirectConvertCommand("запакуй в zip", "/home/user/my_folder"),
      {
        inputPath: "/home/user/my_folder",
        targetFormat: "zip",
      }
    );
    assert.deepEqual(
      detectDirectConvertCommand("в webp", "/home/user/photo.png"),
      {
        inputPath: "/home/user/photo.png",
        targetFormat: "webp",
      }
    );
    assert.deepEqual(
      detectDirectConvertCommand("переведи в текст", "/home/user/document.pdf"),
      {
        inputPath: "/home/user/document.pdf",
        targetFormat: "txt",
      }
    );
    assert.deepEqual(
      detectDirectConvertCommand("зроби txt", "/home/user/notes.pdf"),
      {
        inputPath: "/home/user/notes.pdf",
        targetFormat: "txt",
      }
    );
    assert.deepEqual(
      detectDirectConvertCommand("переведи в markdown", "/home/user/table.xlsx"),
      {
        inputPath: "/home/user/table.xlsx",
        targetFormat: "md",
      }
    );
    assert.deepEqual(
      detectDirectConvertCommand("зроби md", "/home/user/doc.docx"),
      {
        inputPath: "/home/user/doc.docx",
        targetFormat: "md",
      }
    );
    assert.deepEqual(
      detectDirectConvertCommand("розархівуй", "/home/user/archive.zip"),
      {
        inputPath: "/home/user/archive.zip",
        targetFormat: "unzip",
      }
    );
    assert.deepEqual(
      detectDirectConvertCommand("розпакуй цей архів", "/home/user/bundle.tar.gz"),
      {
        inputPath: "/home/user/bundle.tar.gz",
        targetFormat: "unzip",
      }
    );
    assert.deepEqual(
      detectDirectConvertCommand("видобудь", "/home/user/files.7z"),
      {
        inputPath: "/home/user/files.7z",
        targetFormat: "unzip",
      }
    );
    assert.equal(detectDirectConvertCommand("що в цьому файлі?", "/home/user/doc.pdf"), null);
    assert.equal(detectDirectConvertCommand("зроби gif", null), null);
  });

  it("executes direct convert command instantly without calling LLM", async () => {
    let llmCalled = false;
    const fakeClient: ModelClient = {
      generateContent: async () => {
        llmCalled = true;
        throw new Error("LLM should not be called for direct convert commands!");
      },
    };

    const invokedCommands: Array<{ cmd: string; args: unknown }> = [];
    const res = await runAgentTurn({
      modelClient: fakeClient,
      modelName: "gemini-3.5-flash-lite",
      systemInstruction: "System prompt",
      userPrompt: "зроби gif",
      attachedFile: {
        path: "/home/user/sample.mp4",
        name: "sample.mp4",
      },
      toolRegistry: registry,
      invoke: async <T>(cmd: string, args: unknown): Promise<T> => {
        invokedCommands.push({ cmd, args });
        return {
          success: true,
          input_path: "/home/user/sample.mp4",
          output_path: "/home/user/sample.gif",
          output_filename: "sample.gif",
          output_size_bytes: 512000,
          engine_used: "FFmpeg (HQ Palettegen)",
          message: "Успішно створено",
        } as unknown as T;
      },
      callbacks: {
        onStateChange: () => {},
      },
    });

    assert.equal(llmCalled, false);
    assert.ok(res.text.includes("sample.gif"));
    assert.deepEqual(res.executedTools, ["convert_file"]);
    assert.equal(invokedCommands[0].cmd, "convert_file");
    assert.deepEqual(invokedCommands[0].args, {
      input_path: "/home/user/sample.mp4",
      target_format: "gif",
      custom_output_path: null,
    });
  });

  it("detectDirectDiskCommand, detectDirectShareCommand, and detectDirectDownloadsCommand recognize requests", () => {
    assert.deepEqual(detectDirectDiskCommand("що забило диск"), { path: "~" });
    assert.deepEqual(detectDirectDiskCommand("відкрий wiztree"), { path: "~" });
    assert.deepEqual(detectDirectDiskCommand("візуалізатор диска"), { path: "~" });
    assert.equal(detectDirectDiskCommand("привіт маскот"), null);

    assert.deepEqual(
      detectDirectShareCommand("скинь на телефон", "/home/user/file.pdf"),
      { filePath: "/home/user/file.pdf" }
    );
    assert.deepEqual(
      detectDirectShareCommand("кинь на телефон", "/home/user/file.pdf"),
      { filePath: "/home/user/file.pdf" }
    );
    assert.deepEqual(
      detectDirectShareCommand("закинь на телефон", "/home/user/file.pdf"),
      { filePath: "/home/user/file.pdf" }
    );
    assert.deepEqual(
      detectDirectShareCommand("поділись по вайфаю", "/home/user/photos"),
      { filePath: "/home/user/photos" }
    );
    assert.deepEqual(
      detectDirectShareCommand("кинь на телефон /home/user/doc.pdf", null),
      { filePath: "/home/user/doc.pdf" }
    );
    assert.equal(detectDirectShareCommand("скинь на телефон", null), null);

    assert.deepEqual(detectDirectDownloadsCommand("прибери завантаження"), {
      action: "analyze",
    });
    assert.deepEqual(detectDirectDownloadsCommand("очисти downloads"), {
      action: "analyze",
    });
    assert.equal(detectDirectDownloadsCommand("що чути"), null);
  });

  it("executes direct disk and share commands instantly without calling LLM", async () => {
    let llmCalled = false;
    const dummyClient: ModelClient = {
      generateContent: async () => {
        llmCalled = true;
        throw new Error("Should not call LLM");
      },
    };

    const invokedCommands: Array<{ cmd: string; args: unknown }> = [];

    // Тест швидкого відкриття WizTree
    const resDisk = await runAgentTurn({
      modelClient: dummyClient,
      modelName: "test-model",
      systemInstruction: "System prompt",
      userPrompt: "що забило диск",
      toolRegistry: registry,
      invoke: async <T>(cmd: string, args: unknown): Promise<T> => {
        invokedCommands.push({ cmd, args });
        return {} as unknown as T;
      },
      callbacks: {
        onStateChange: () => {},
      },
    });

    assert.equal(llmCalled, false);
    assert.deepEqual(resDisk.executedTools, ["open_disk_analyzer"]);
    assert.ok(resDisk.text.includes("WizTree"));

    // Тест швидкої роздачі по Wi-Fi
    const resShare = await runAgentTurn({
      modelClient: dummyClient,
      modelName: "test-model",
      systemInstruction: "System prompt",
      userPrompt: "скинь на телефон",
      attachedFile: {
        path: "/home/user/book.pdf",
        name: "book.pdf",
      },
      toolRegistry: registry,
      invoke: async <T>(cmd: string, args: unknown): Promise<T> => {
        invokedCommands.push({ cmd, args });
        if (cmd === "start_file_share") {
          return {
            share_url: "http://192.168.1.50:1422/",
            file_name: "book.pdf",
            file_size: 10240,
            is_dir: false,
          } as unknown as T;
        }
        return {} as unknown as T;
      },
      callbacks: {
        onStateChange: () => {},
      },
    });

    assert.equal(llmCalled, false);
    assert.deepEqual(resShare.executedTools, ["share_file_wifi"]);
    assert.ok(resShare.text.includes("book.pdf"));
    assert.ok(resShare.text.includes("http://192.168.1.50:1422/"));
  });

  it("detectDirectPresetCommand correctly parses open, save, close, list commands", () => {
    assert.deepEqual(detectDirectPresetCommand("відкрий навчання"), {
      action: "open",
      name: "study",
    });
    assert.deepEqual(detectDirectPresetCommand("стек навчання"), {
      action: "open",
      name: "study",
    });
    assert.deepEqual(detectDirectPresetCommand("навчання"), {
      action: "open",
      name: "study",
    });
    assert.deepEqual(detectDirectPresetCommand("відкрий робочий стек"), {
      action: "open",
      name: "work",
    });
    assert.deepEqual(detectDirectPresetCommand("робочий стек"), {
      action: "open",
      name: "work",
    });
    assert.deepEqual(detectDirectPresetCommand("відкрий пресет дизайн"), {
      action: "open",
      name: "дизайн",
    });

    assert.deepEqual(
      detectDirectPresetCommand("збережи поточний стек як робота"),
      {
        action: "save",
        name: "робота",
      }
    );
    assert.deepEqual(detectDirectPresetCommand("збережи сесію як крипта"), {
      action: "save",
      name: "крипта",
    });

    assert.deepEqual(detectDirectPresetCommand("закрий усе для навчання"), {
      action: "close",
      name: "study",
    });
    assert.deepEqual(detectDirectPresetCommand("закрий робочий стек"), {
      action: "close",
      name: "work",
    });

    assert.deepEqual(detectDirectPresetCommand("покажи мої пресети"), {
      action: "list",
    });
    assert.deepEqual(detectDirectPresetCommand("які є пресети"), {
      action: "list",
    });
  });

  it("runs fast-path for session presets without calling LLM", async () => {
    let llmCalled = false;
    const fakeClient: ModelClient = {
      generateContent: async () => {
        llmCalled = true;
        return { text: "LLM output" };
      },
    };

    const invokedCommands: Array<{ cmd: string; args: unknown }> = [];
    const res = await runAgentTurn({
      modelClient: fakeClient,
      modelName: "test-model",
      systemInstruction: "System prompt",
      userPrompt: "відкрий навчання",
      toolRegistry: registry,
      invoke: async <T>(cmd: string, args: unknown): Promise<T> => {
        invokedCommands.push({ cmd, args });
        if (cmd === "manage_session_presets") {
          return "Стек «Навчання» успішно відкрито (LMS UCU, MS Teams та VS Code) 🚀" as unknown as T;
        }
        return {} as unknown as T;
      },
      callbacks: {
        onStateChange: () => {},
      },
    });

    assert.equal(llmCalled, false);
    assert.deepEqual(res.executedTools, ["manage_session_presets"]);
    assert.ok(res.text.includes("Навчання"));
    assert.ok(
      invokedCommands.some(
        (c) =>
          c.cmd === "manage_session_presets" &&
          (c.args as Record<string, unknown>).action === "open" &&
          (c.args as Record<string, unknown>).name === "study"
      )
    );
  });
});
