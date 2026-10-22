import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { composeSystemPrompt } from "./prompt.ts";
import { createDefaultToolRegistry } from "./registry.ts";

describe("composeSystemPrompt", () => {
  const registry = createDefaultToolRegistry();
  const fixedDate = new Date(2026, 9, 21, 15, 30, 0); // Wednesday, Oct 21, 2026

  it("contains persona, Ukrainian language instruction and time info", () => {
    const prompt = composeSystemPrompt({
      now: fixedDate,
      vaultPath: "~/Vault/MyNotes",
      memoryFacts: ["Користувача звати Іван", "Любить слухати рок"],
      toolRegistry: registry,
    });

    assert.ok(prompt.includes("будівельник у жовтій касці"));
    assert.ok(prompt.includes("Спілкуйся українською мовою"));
    assert.ok(prompt.includes("2026-10-21 15:30:00"));
    assert.ok(prompt.includes("Середа"));
    assert.ok(prompt.includes("~/Vault/MyNotes"));
  });

  it("includes all registered tool rules", () => {
    const prompt = composeSystemPrompt({
      now: fixedDate,
      toolRegistry: registry,
    });

    assert.ok(prompt.includes("append_to_obsidian:"));
    assert.ok(prompt.includes("create_or_update_file:"));
    assert.ok(prompt.includes("read_file_or_dir:"));
    assert.ok(prompt.includes("run_cli_command:"));
    assert.ok(prompt.includes("control_media:"));
    assert.ok(prompt.includes("manage_memory:"));
    assert.ok(prompt.includes("manage_reminders:"));
    assert.ok(prompt.includes("summarize_webpage:"));
  });

  it("includes untrusted web content guard", () => {
    const prompt = composeSystemPrompt({
      now: fixedDate,
      toolRegistry: registry,
    });

    assert.ok(prompt.includes("<untrusted_web_content>"));
  });

  it("limits memory facts to 15 items and prioritizes relevant facts", () => {
    const facts = Array.from({ length: 30 }, (_, i) => `Факт №${i + 1} про проєкт`);
    facts[0] = "Спеціальний факт про астрономію та телескопи";

    const prompt = composeSystemPrompt({
      now: fixedDate,
      memoryFacts: facts,
      userQuery: "Покажи мені телескопи",
      toolRegistry: registry,
    });

    // Перший факт підтягнувся завдяки збігу з query "телескопи"
    assert.ok(prompt.includes("астрономію та телескопи"));
    assert.ok(prompt.includes("Факт №30"));
  });

  it("includes active VS Code workspace and active file when detected", () => {
    const prompt = composeSystemPrompt({
      now: fixedDate,
      toolRegistry: registry,
      activeWorkspace: {
        workspacePath: "/home/ivan/local_helper_ai",
        activeFile: "/home/ivan/Desktop/pedro_cabral.md",
      },
    });

    assert.ok(prompt.includes("/home/ivan/local_helper_ai"));
    assert.ok(prompt.includes("/home/ivan/Desktop/pedro_cabral.md"));
    assert.ok(prompt.includes("орієнтуйся на активний проект VS Code як робочу папку"));
  });
});
