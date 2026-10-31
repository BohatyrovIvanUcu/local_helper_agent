import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { evaluatePolicy } from "./policy.ts";
import type { ToolContext } from "./types.ts";

describe("SafetyPolicy", () => {
  const dummyCtx: ToolContext = {
    invoke: async <T>() => ({} as T),
    now: new Date(),
    vaultPath: "/home/ivan/Obsidian/Ivan",
    tainted: false,
  };

  it("denies sudo, su, and access to secrets in cli", () => {
    const resSudo = evaluatePolicy("run_cli_command", { command: "sudo apt update" }, dummyCtx);
    assert.equal(resSudo.decision, "deny");

    const resSsh = evaluatePolicy("run_cli_command", { command: "cat ~/.ssh/id_rsa" }, dummyCtx);
    assert.equal(resSsh.decision, "deny");

    const resConfig = evaluatePolicy("run_cli_command", { command: "grep KEY config.env" }, dummyCtx);
    assert.equal(resConfig.decision, "deny");

    const resDotEnv = evaluatePolicy("run_cli_command", { command: "cat .env" }, dummyCtx);
    assert.equal(resDotEnv.decision, "deny");
  });

  it("allows safe read-only and browser commands without confirmation", () => {
    assert.equal(
      evaluatePolicy("run_cli_command", { command: "git status -s" }, dummyCtx).decision,
      "allow"
    );
    assert.equal(
      evaluatePolicy("run_cli_command", { command: "ls -la /tmp" }, dummyCtx).decision,
      "allow"
    );
    assert.equal(
      evaluatePolicy("run_cli_command", { command: "xdg-open https://google.com" }, dummyCtx).decision,
      "allow"
    );
    assert.equal(
      evaluatePolicy("run_cli_command", { command: "open https://google.com" }, dummyCtx).decision,
      "allow"
    );
    assert.equal(
      evaluatePolicy("run_cli_command", { command: "gio open https://google.com" }, dummyCtx).decision,
      "allow"
    );
    assert.equal(
      evaluatePolicy("run_cli_command", { command: "firefox https://youtube.com" }, dummyCtx).decision,
      "allow"
    );
    assert.equal(
      evaluatePolicy("run_cli_command", { command: "mkdir -p ~/projects/my-app" }, dummyCtx).decision,
      "allow"
    );
    assert.equal(
      evaluatePolicy("run_cli_command", { command: "pdftotext -layout /tmp/doc.pdf -" }, dummyCtx).decision,
      "allow"
    );
  });

  it("requires confirmation for script execution and destructive commands", () => {
    const resGitPush = evaluatePolicy("run_cli_command", { command: "git push origin main" }, dummyCtx);
    assert.equal(resGitPush.decision, "confirm");

    const resPy = evaluatePolicy("run_cli_command", { command: "python3 script.py" }, dummyCtx);
    assert.equal(resPy.decision, "confirm");

    const resBash = evaluatePolicy("run_cli_command", { command: "./build.sh" }, dummyCtx);
    assert.equal(resBash.decision, "confirm");

    const resRm = evaluatePolicy("run_cli_command", { command: "rm -rf /tmp/test" }, dummyCtx);
    assert.equal(resRm.decision, "confirm");

    const resRedirect = evaluatePolicy("run_cli_command", { command: "echo 'bad' > /etc/hosts" }, dummyCtx);
    assert.equal(resRedirect.decision, "confirm");
  });

  it("requires confirmation for all actions during a tainted turn", () => {
    const taintedCtx: ToolContext = {
      ...dummyCtx,
      tainted: true,
    };

    // Even an allowlisted command requires confirmation if tainted
    const resCli = evaluatePolicy("run_cli_command", { command: "ls -la" }, taintedCtx);
    assert.equal(resCli.decision, "confirm");
    if (resCli.decision === "confirm") {
      assert.ok(resCli.title.includes("вебсторінки"));
    }

    const resFile = evaluatePolicy(
      "create_or_update_file",
      { filepath: "~/projects/test.txt", content: "data" },
      taintedCtx
    );
    assert.equal(resFile.decision, "confirm");

    const resNote = evaluatePolicy(
      "append_to_obsidian",
      { content: "suspicious note" },
      taintedCtx
    );
    assert.equal(resNote.decision, "confirm");

    const resManageObsidian = evaluatePolicy(
      "manage_obsidian",
      { action: "create", filename: "Injected.md", content: "malicious" },
      taintedCtx
    );
    assert.equal(resManageObsidian.decision, "confirm");
  });

  it("allows safe desktop applications and detached launches without confirmation", () => {
    assert.equal(
      evaluatePolicy("run_cli_command", { command: "code ." }, dummyCtx).decision,
      "allow"
    );
    assert.equal(
      evaluatePolicy("run_cli_command", { command: "obsidian &" }, dummyCtx).decision,
      "allow"
    );
    assert.equal(
      evaluatePolicy("run_cli_command", { command: "telegram-desktop &" }, dummyCtx).decision,
      "allow"
    );
    assert.equal(
      evaluatePolicy("run_cli_command", { command: "nohup gnome-calculator > /dev/null 2>&1 &" }, dummyCtx).decision,
      "allow"
    );
    assert.equal(
      evaluatePolicy("run_cli_command", { command: "vlc music.mp3 &" }, dummyCtx).decision,
      "allow"
    );
  });

  it("allows safe audio and volume control commands without confirmation", () => {
    assert.equal(
      evaluatePolicy("run_cli_command", { command: "pactl set-sink-volume @DEFAULT_SINK@ +5%" }, dummyCtx).decision,
      "allow"
    );
    assert.equal(
      evaluatePolicy("run_cli_command", { command: "pactl set-sink-mute @DEFAULT_SINK@ toggle" }, dummyCtx).decision,
      "allow"
    );
    assert.equal(
      evaluatePolicy("run_cli_command", { command: "wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%+" }, dummyCtx).decision,
      "allow"
    );
    assert.equal(
      evaluatePolicy("run_cli_command", { command: "amixer set Master 10%+" }, dummyCtx).decision,
      "allow"
    );
    assert.equal(
      evaluatePolicy("run_cli_command", { command: "pamixer -i 5" }, dummyCtx).decision,
      "allow"
    );
    assert.equal(
      evaluatePolicy("run_cli_command", { command: "osascript -e 'set volume output volume 50'" }, dummyCtx).decision,
      "allow"
    );
  });

  it("allows safe display and night light control commands without confirmation", () => {
    assert.equal(
      evaluatePolicy("run_cli_command", { command: "brightnessctl set 50%" }, dummyCtx).decision,
      "allow"
    );
    assert.equal(
      evaluatePolicy("run_cli_command", { command: "light -S 70" }, dummyCtx).decision,
      "allow"
    );
    assert.equal(
      evaluatePolicy("run_cli_command", { command: "gsettings set org.gnome.settings-daemon.plugins.color night-light-enabled true" }, dummyCtx).decision,
      "allow"
    );
    assert.equal(
      evaluatePolicy("run_cli_command", { command: "gsettings get org.gnome.settings-daemon.plugins.color night-light-temperature" }, dummyCtx).decision,
      "allow"
    );
    assert.equal(
      evaluatePolicy("run_cli_command", { command: "busctl --user get-property org.gnome.Mutter.DisplayConfig /org/gnome/Mutter/DisplayConfig org.gnome.Mutter.DisplayConfig Backlight" }, dummyCtx).decision,
      "allow"
    );
    assert.equal(
      evaluatePolicy("control_display", { action: "set_brightness", value: 40 }, dummyCtx).decision,
      "allow"
    );
  });

  it("allows creating and editing safe text files (.txt, .md, .log, .json, .csv) anywhere non-sensitive", () => {
    assert.equal(
      evaluatePolicy("create_or_update_file", { filepath: "test.txt" }, dummyCtx).decision,
      "allow"
    );
    assert.equal(
      evaluatePolicy("create_or_update_file", { filepath: "some_folder/notes.md" }, dummyCtx).decision,
      "allow"
    );
    assert.equal(
      evaluatePolicy("create_or_update_file", { filepath: "~/Desktop/report.txt" }, dummyCtx).decision,
      "allow"
    );
    assert.equal(
      evaluatePolicy("create_or_update_file", { filepath: "/tmp/debug.log" }, dummyCtx).decision,
      "allow"
    );
  });

  it("handles file creation paths (allowed roots vs foreign vs sensitive)", () => {
    assert.equal(
      evaluatePolicy("create_or_update_file", { filepath: "~/projects/app/main.py" }, dummyCtx).decision,
      "allow"
    );
    assert.equal(
      evaluatePolicy("create_or_update_file", { filepath: "/etc/cron.d/job" }, dummyCtx).decision,
      "confirm"
    );
    assert.equal(
      evaluatePolicy("create_or_update_file", { filepath: "~/.ssh/authorized_keys" }, dummyCtx).decision,
      "deny"
    );
  });
});
