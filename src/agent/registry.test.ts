import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { createDefaultToolRegistry } from "./registry.ts";

describe("ToolRegistry", () => {
  const registry = createDefaultToolRegistry();

  it("registers all 15 core tools", () => {
    const expected = [
      "append_to_obsidian",
      "manage_obsidian",
      "create_or_update_file",
      "read_file_or_dir",
      "run_cli_command",
      "control_media",
      "control_display",
      "manage_memory",
      "manage_reminders",
      "summarize_webpage",
      "convert_file",
      "open_disk_analyzer",
      "share_file_wifi",
      "clean_downloads",
      "manage_session_presets",
    ];

    for (const name of expected) {
      const tool = registry.get(name);
      assert.ok(tool, `Tool ${name} must be registered`);
      assert.equal(tool.name, name);
      assert.ok(tool.promptRule.length > 0);
      assert.ok(tool.declaration.name === name);
    }
  });

  it("validates append_to_obsidian arguments", () => {
    const tool = registry.get("append_to_obsidian")!;
    const invalid = tool.validateArgs({});
    assert.equal(invalid.ok, false);

    const valid = tool.validateArgs({
      note: "Купити хліб",
      is_todo: true,
      section: "## TO DO",
      subfolder: "null",
      filename: "TODO",
    });
    assert.equal(valid.ok, true);
    if (valid.ok) {
      assert.equal(valid.args.content, "Купити хліб");
      assert.equal(valid.args.as_todo, true);
      assert.equal(valid.args.section, "TO DO");
      assert.equal(valid.args.filename, "TODO.md");
      assert.equal(valid.args.subfolder, null);
    }
  });

  it("formats append_to_obsidian execute content with proper Kanban due date syntax", async () => {
    const tool = registry.get("append_to_obsidian")!;
    let passedArgs: Record<string, unknown> = {};

    const mockCtx = {
      now: new Date("2026-10-23T10:00:00"),
      tainted: false,
      invoke: async <T>(_cmd: string, args?: Record<string, unknown>): Promise<T> => {
        passedArgs = args || {};
        return "Ok" as T;
      },
    };

    const res = await tool.execute(
      {
        content: "Здати архітектурний звіт",
        as_todo: true,
        section: "TO DO",
        filename: "TODO.md",
        due_date: "завтра",
      },
      mockCtx
    );

    assert.equal(res.ok, true);
    assert.equal(passedArgs.content, "Здати архітектурний звіт @{2026-10-24}");
    assert.equal(passedArgs.due_date, "2026-10-24");
    assert.equal(passedArgs.section, "TO DO");
  });

  it("validates and executes manage_obsidian actions correctly", async () => {
    const tool = registry.get("manage_obsidian")!;

    // 1. Validation checks
    assert.equal(tool.validateArgs({ action: "read" }).ok, false, "read requires filename");
    assert.equal(tool.validateArgs({ action: "create" }).ok, false, "create requires content");
    assert.equal(tool.validateArgs({ action: "search" }).ok, false, "search requires query");
    assert.equal(tool.validateArgs({ action: "complete_todo" }).ok, false, "complete_todo requires query");

    const readValid = tool.validateArgs({ action: "read", filename: "lab ASM" });
    assert.equal(readValid.ok, true);
    if (readValid.ok) {
      assert.equal(readValid.args.action, "read");
      assert.equal(readValid.args.filename, "lab ASM.md");
    }

    const searchValid = tool.validateArgs({ action: "search", query: "алгоритми" });
    assert.equal(searchValid.ok, true);
    if (searchValid.ok) {
      assert.equal(searchValid.args.query, "алгоритми");
      assert.equal(searchValid.args.search_content, true);
    }

    // 2. Execution checks
    let invokedCmd = "";
    let invokedArgs: Record<string, unknown> = {};

    const mockCtx = {
      now: new Date("2026-10-23T10:00:00"),
      tainted: false,
      invoke: async <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
        invokedCmd = cmd;
        invokedArgs = args || {};
        if (cmd === "read_obsidian_note") {
          return "# Конспект\nДеталі" as T;
        }
        if (cmd === "search_obsidian_notes") {
          return [
            {
              filename: "TODO.md",
              relative_path: "TODO.md",
              matches: ["- [ ] Мідтерм алгоси"],
            },
          ] as T;
        }
        if (cmd === "complete_obsidian_todo") {
          return "Задачу виконано у 'TODO.md': - [x] Мідтерм алгоси" as T;
        }
        if (cmd === "create_or_update_obsidian_note") {
          return "Нотатку успішно збережено" as T;
        }
        return "Ok" as T;
      },
    };

    // Test read
    const readRes = await tool.execute({ action: "read", filename: "lab ASM.md" }, mockCtx);
    assert.equal(readRes.ok, true);
    assert.equal(invokedCmd, "read_obsidian_note");
    assert.equal(invokedArgs.filename, "lab ASM.md");

    // Test search
    const searchRes = await tool.execute({ action: "search", query: "алгоси" }, mockCtx);
    assert.equal(searchRes.ok, true);
    assert.equal(invokedCmd, "search_obsidian_notes");
    assert.equal(invokedArgs.query, "алгоси");
    assert.ok(searchRes.summary.includes("Мідтерм алгоси"));

    // Test complete_todo
    const completeRes = await tool.execute({ action: "complete_todo", query: "алгоси" }, mockCtx);
    assert.equal(completeRes.ok, true);
    assert.equal(invokedCmd, "complete_obsidian_todo");
    assert.equal(invokedArgs.task_query, "алгоси");
    assert.equal(invokedArgs.filename, "TODO.md");
  });

  it("validates create_or_update_file arguments", () => {
    const tool = registry.get("create_or_update_file")!;
    assert.equal(tool.validateArgs({}).ok, false);

    const valid = tool.validateArgs({
      path: "~/test.txt",
      content: "Hello",
    });
    assert.equal(valid.ok, true);
    if (valid.ok) {
      assert.equal(valid.args.filepath, "~/test.txt");
      assert.equal(valid.args.content, "Hello");
    }
  });

  it("validates run_cli_command arguments", () => {
    const tool = registry.get("run_cli_command")!;
    assert.equal(tool.validateArgs({}).ok, false);

    const valid = tool.validateArgs({
      cmd: "git status",
    });
    assert.equal(valid.ok, true);
    if (valid.ok) {
      assert.equal(valid.args.command, "git status");
    }
  });

  it("executes run_cli_command using activeWorkspacePath when working_dir is omitted", async () => {
    const tool = registry.get("run_cli_command")!;
    let passedWorkingDir: unknown = undefined;

    const mockCtx = {
      now: new Date(),
      tainted: false,
      activeWorkspacePath: "/home/ivan/my-cool-project",
      invoke: async <T>(_cmd: string, args?: Record<string, unknown>): Promise<T> => {
        passedWorkingDir = args?.working_dir;
        return "Exit code: 0\nOn branch main" as T;
      },
    };

    const res = await tool.execute({ command: "git status" }, mockCtx);
    assert.equal(res.ok, true);
    assert.equal(passedWorkingDir, "/home/ivan/my-cool-project");
  });

  it("validates manage_reminders arguments", () => {
    const tool = registry.get("manage_reminders")!;
    const valid = tool.validateArgs({
      action: "create",
      title: "Зустріч",
      seconds: "300",
    });
    assert.equal(valid.ok, true);
    if (valid.ok) {
      assert.equal(valid.args.action, "create");
      assert.equal(valid.args.text, "Зустріч");
      assert.equal(valid.args.delay_seconds, 300);
    }
  });

  it("validates convert_file arguments", () => {
    const tool = registry.get("convert_file")!;
    const invalid = tool.validateArgs({});
    assert.equal(invalid.ok, false);

    const valid = tool.validateArgs({
      input_path: "/home/user/video.mp4",
      target_format: ".GIF",
      custom_output_path: "anim.gif",
    });
    assert.equal(valid.ok, true);
    if (valid.ok) {
      assert.equal(valid.args.input_path, "/home/user/video.mp4");
      assert.equal(valid.args.target_format, "gif");
      assert.equal(valid.args.custom_output_path, "anim.gif");
    }
  });

  it("validates open_disk_analyzer, share_file_wifi, and clean_downloads arguments", () => {
    const diskTool = registry.get("open_disk_analyzer")!;
    const shareTool = registry.get("share_file_wifi")!;
    const cleanTool = registry.get("clean_downloads")!;

    const validDisk = diskTool.validateArgs({ path: "~/Projects" });
    assert.equal(validDisk.ok, true);

    const invalidShare = shareTool.validateArgs({ file_path: "" });
    assert.equal(invalidShare.ok, false);

    const validShare = shareTool.validateArgs({ file_path: "/home/user/doc.pdf" });
    assert.equal(validShare.ok, true);

    const validClean = cleanTool.validateArgs({ execute_cleanup: true });
    assert.equal(validClean.ok, true);

    const presetsTool = registry.get("manage_session_presets");
    assert.ok(presetsTool);
    const validPresets = presetsTool.validateArgs({ action: "open", name: "study" });
    assert.equal(validPresets.ok, true);
    if (validPresets.ok) {
      assert.equal(validPresets.args.action, "open");
      assert.equal(validPresets.args.name, "study");
    }
  });

  it("generates function declarations and prompt rules for all tools", () => {
    const decls = registry.getFunctionDeclarations();
    assert.equal(decls.length, 15);

    const rules = registry.getPromptRules();
    assert.equal(rules.length, 15);
    for (const rule of rules) {
      assert.ok(typeof rule === "string" && rule.length > 5);
    }
  });
});

