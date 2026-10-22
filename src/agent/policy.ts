import type { RiskLevel, ToolContext } from "./types.ts";

export type PolicyDecision =
  | { decision: "allow" }
  | { decision: "confirm"; title: string; details: string; risk: RiskLevel }
  | { decision: "deny"; reason: string };

const ALLOWED_WRITE_ROOTS = [
  "/tmp",
  ".config/local-helper-ai",
  "projects",
  "Projects",
  "workspace",
  "Documents",
  "Документи",
  "Desktop",
  "Робочий стіл",
  "Downloads",
  "Завантаження",
  "Obsidian",
  "local_helper_ai",
];

const SAFE_TEXT_EXTENSIONS = new Set([
  ".txt",
  ".md",
  ".markdown",
  ".log",
  ".csv",
  ".tsv",
  ".json",
]);

function isSafeTextFile(filepath: string): boolean {
  const lower = filepath.toLowerCase().trim();
  for (const ext of SAFE_TEXT_EXTENSIONS) {
    if (lower.endsWith(ext)) {
      return true;
    }
  }
  return false;
}

const READONLY_COMMANDS = new Set([
  "ls",
  "pwd",
  "ps",
  "cat",
  "head",
  "tail",
  "wc",
  "grep",
  "rg",
  "which",
  "whoami",
  "uname",
  "df",
  "du",
  "free",
  "date",
  "uptime",
  "echo",
  "stat",
  "file",
]);

const APP_LAUNCH_COMMANDS = new Set([
  // Браузери та системні URL/файлові відкривачі
  "xdg-open",
  "open",
  "gio",
  "gnome-open",
  "kde-open",
  "sensible-browser",
  "x-www-browser",
  "firefox",
  "google-chrome",
  "google-chrome-stable",
  "chromium",
  "chromium-browser",
  "brave",
  "brave-browser",
  "opera",
  "vivaldi",
  "epiphany",
  "microsoft-edge",
  "microsoft-edge-stable",

  // Редактори коду та нотаток
  "code",
  "vscode",
  "codium",
  "vscodium",
  "obsidian",
  "gedit",
  "kate",
  "mousepad",
  "xed",
  "leafpad",
  "subl",
  "sublime-text",
  "atom",
  "notepadqq",

  // Месенджери та комунікація
  "telegram-desktop",
  "telegram",
  "discord",
  "slack",
  "skype",
  "skypeforlinux",
  "zoom",
  "signal-desktop",
  "element-desktop",
  "thunderbird",

  // Мультимедіа та плеєри
  "spotify",
  "vlc",
  "mpv",
  "rhythmbox",
  "totem",
  "audacious",
  "clementine",

  // Файлові менеджери
  "nautilus",
  "dolphin",
  "thunar",
  "nemo",
  "pcmanfm",
  "caja",

  // Термінали
  "gnome-terminal",
  "konsole",
  "alacritty",
  "kitty",
  "xterm",
  "tilix",
  "terminator",

  // Системні застосунки, утиліти та офіс
  "gnome-calculator",
  "kcalc",
  "calc",
  "libreoffice",
  "soffice",
  "gimp",
  "inkscape",
  "steam",
  "dbeaver",
  "dbeaver-ce",
  "postman",
  "obs",
  "obs-studio",
  "gtk-launch",
  "pavucontrol",
]);

function isSensitivePath(path: string): boolean {
  const normalized = path.replace(/\\/g, "/");
  return (
    normalized.includes(".ssh") ||
    normalized.includes(".gnupg") ||
    normalized.includes(".aws") ||
    normalized.includes("/etc/shadow") ||
    normalized.includes("config.env") ||
    normalized.includes(".bashrc") ||
    normalized.includes(".zshrc") ||
    normalized.includes(".profile") ||
    /(?:^|[\s/'"])\.env(?:$|[\s/'"])/.test(normalized)
  );
}

function isPathInAllowedRoots(
  path: string,
  vaultPath?: string,
  activeWorkspacePath?: string | null
): boolean {
  const normalized = path.replace(/\\/g, "/");
  if (vaultPath && normalized.startsWith(vaultPath.replace(/\\/g, "/"))) {
    return true;
  }
  if (
    activeWorkspacePath &&
    normalized.startsWith(activeWorkspacePath.replace(/\\/g, "/"))
  ) {
    return true;
  }
  return ALLOWED_WRITE_ROOTS.some((root) => normalized.includes(root));
}

/**
 * Оцінює безпеку виклику інструменту.
 */
export function evaluatePolicy(
  toolName: string,
  args: Record<string, unknown>,
  ctx: ToolContext
): PolicyDecision {
  // 1. Tainted-режим: захист від Indirect Prompt Injection
  // Якщо модель уже прочитала зовнішній вміст вебсторінки у цьому ході
  if (ctx.tainted) {
    if (
      toolName === "run_cli_command" ||
      toolName === "create_or_update_file" ||
      toolName === "append_to_obsidian" ||
      toolName === "manage_memory"
    ) {
      const details =
        toolName === "run_cli_command"
          ? String(args.command ?? "")
          : toolName === "create_or_update_file"
          ? String(args.filepath ?? "")
          : toolName === "append_to_obsidian"
          ? String(args.content ?? "")
          : String(args.fact ?? "");

      return {
        decision: "confirm",
        title: "⚠️ Дія після читання вебсторінки (захист від ін'єкцій)",
        details: `Інструмент: ${toolName}\nВміст: ${details}`,
        risk: "write",
      };
    }
  }

  // 2. run_cli_command
  if (toolName === "run_cli_command") {
    const rawCmd = String(args.command ?? "").trim();
    if (!rawCmd) {
      return { decision: "deny", reason: "Порожня команда." };
    }

    // Безумовна заборона (sudo, su, доступ до секретів)
    const lowerCmd = rawCmd.toLowerCase();
    if (
      lowerCmd.startsWith("sudo ") ||
      lowerCmd.includes(" sudo ") ||
      lowerCmd.startsWith("su ") ||
      lowerCmd.includes(" su ") ||
      lowerCmd.includes("pkexec") ||
      isSensitivePath(rawCmd)
    ) {
      return {
        decision: "deny",
        reason:
          "Команда заблокована політикою безпеки (заборонено sudo/su та доступ до секретів).",
      };
    }

    // Перевірка на небезпечні підстановки команд ($(..), `..`)
    if (rawCmd.includes("$(") || rawCmd.includes("`")) {
      return {
        decision: "confirm",
        title: "Запуск команди з підстановкою",
        details: rawCmd,
        risk: "exec",
      };
    }

    // Перевірка підкоманд (ланцюжків через ;, &&, ||, |)
    // Попередньо нормалізуємо перенаправлення дескрипторів (наприклад 2>&1 або &>),
    // щоб & у 2>&1 не сприймався як роздільник команд
    const normalizedCmd = rawCmd
      .replace(/\s*\d*>&[0-9]+\b/g, "")
      .replace(/\s*&>\s*/g, " > ");

    const subcommands = normalizedCmd
      .split(/[;&|]+/)
      .map((s) => s.trim())
      .filter(Boolean);

    let allSubcommandsAllowed = subcommands.length > 0;

    for (const sub of subcommands) {
      // Перевірка перенаправлення виводу у файл (> або >>)
      if (sub.includes(">")) {
        const parts = sub.split(/>+/);
        const dest = parts[parts.length - 1]?.trim().replace(/['"]/g, "") || "";
        const left = parts[0]?.trim() || "";
        const leftTokens = left.split(/\s+/);
        const leftBin = leftTokens[0]?.toLowerCase().split("/").pop() || "";

        // Дозволяємо перенаправлення в /dev/null (типово для запуску апок) або у безпечні текстові файли (.txt, .log)
        if (
          dest === "/dev/null" ||
          dest.startsWith("/dev/null") ||
          ((leftBin === "echo" ||
            leftBin === "printf" ||
            leftBin === "cat" ||
            READONLY_COMMANDS.has(leftBin)) &&
            dest &&
            !isSensitivePath(dest) &&
            (isSafeTextFile(dest) ||
              isPathInAllowedRoots(
                dest,
                ctx.vaultPath,
                ctx.activeWorkspacePath
              )))
        ) {
          // Безпечне перенаправлення
        } else {
          allSubcommandsAllowed = false;
          break;
        }
      }

      const tokens = sub.split(/\s+/);
      let bin = tokens[0]?.toLowerCase() || "";
      let baseBin = bin.split("/").pop() || "";

      // Якщо команда запускається через nohup / setsid
      if (baseBin === "nohup" || baseBin === "setsid") {
        const nextBin = tokens[1]?.toLowerCase().split("/").pop() || "";
        if (nextBin) {
          baseBin = nextBin;
        }
      }

      // Запуск застосунків / десктопних програм
      if (APP_LAUNCH_COMMANDS.has(baseBin)) {
        continue;
      }
      if (baseBin === "gio" && tokens[1]?.toLowerCase() === "open") {
        continue;
      }

      // Git: дозволені лише read-only підкоманди
      if (baseBin === "git") {
        const gitSub = tokens[1]?.toLowerCase();
        const allowedGitSubs = new Set([
          "status",
          "diff",
          "log",
          "branch",
          "show",
          "tag",
          "describe",
          "remote",
          "rev-parse",
        ]);
        if (!gitSub || !allowedGitSubs.has(gitSub)) {
          allSubcommandsAllowed = false;
          break;
        }
        continue;
      }

      // Діагностичні/read-only команди
      if (READONLY_COMMANDS.has(baseBin)) {
        // Якщо find містить небезпечні прапорці
        if (
          baseBin === "find" &&
          (sub.includes("-exec") ||
            sub.includes("-delete") ||
            sub.includes("-ok"))
        ) {
          allSubcommandsAllowed = false;
          break;
        }
        continue;
      }

      // Створення директорій або порожніх файлів у дозволених коренях або безпечних .txt
      if (baseBin === "mkdir" || baseBin === "touch") {
        const target = tokens.slice(1).filter((t) => !t.startsWith("-")).join(" ");
        if (
          target &&
          !isSensitivePath(target) &&
          (isSafeTextFile(target) ||
            isPathInAllowedRoots(
              target,
              ctx.vaultPath,
              ctx.activeWorkspacePath
            ))
        ) {
          continue;
        }
      }

      // Будь-яка інша команда (скрипт, python, rm, apt, kill, etc.)
      allSubcommandsAllowed = false;
      break;
    }

    if (allSubcommandsAllowed) {
      return { decision: "allow" };
    }

    return {
      decision: "confirm",
      title: "Потрібне підтвердження команди",
      details: rawCmd,
      risk: "exec",
    };
  }

  // 3. create_or_update_file
  if (toolName === "create_or_update_file") {
    const rawPath = String(args.filepath ?? args.path ?? "").trim();
    if (isSensitivePath(rawPath)) {
      return {
        decision: "deny",
        reason: "Заборонено запис у системні та секретні директорії.",
      };
    }

    // Створення або редагування текстових файлів (.txt, .md, .log, .csv, .json)
    // чи файлів у дозволених робочих папках не вимагає підтвердження
    if (
      isSafeTextFile(rawPath) ||
      isPathInAllowedRoots(rawPath, ctx.vaultPath, ctx.activeWorkspacePath)
    ) {
      return { decision: "allow" };
    }

    return {
      decision: "confirm",
      title: "Запис виконуваного файлу або поза робочими директоріями",
      details: rawPath,
      risk: "write",
    };
  }

  // 4. read_file_or_dir
  if (toolName === "read_file_or_dir") {
    const rawPath = String(args.target_path ?? args.path ?? "");
    if (isSensitivePath(rawPath)) {
      return {
        decision: "deny",
        reason: "Заборонено читання конфіденційних файлів та ключів.",
      };
    }
    return { decision: "allow" };
  }

  // 5. Інші стандартні інструменти
  return { decision: "allow" };
}

