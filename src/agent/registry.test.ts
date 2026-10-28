import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { createDefaultToolRegistry } from "./registry.ts";

describe("ToolRegistry", () => {
  const registry = createDefaultToolRegistry();

  it("registers all 13 core tools", () => {
    const expected = [
      "append_to_obsidian",
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
  });

  it("generates function declarations and prompt rules for all tools", () => {
    const decls = registry.getFunctionDeclarations();
    assert.equal(decls.length, 13);

    const rules = registry.getPromptRules();
    assert.equal(rules.length, 13);
    for (const rule of rules) {
      assert.ok(typeof rule === "string" && rule.length > 5);
    }
  });
});

