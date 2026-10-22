import type { ToolSpec } from "../types.ts";
import { obsidianTool } from "./obsidian.ts";
import { createFileTool, readFileTool } from "./files.ts";
import { cliTool } from "./cli.ts";
import { mediaTool } from "./media.ts";
import { memoryTool } from "./memory.ts";
import { remindersTool } from "./reminders.ts";
import { webTool } from "./web.ts";

export {
  obsidianTool,
  createFileTool,
  readFileTool,
  cliTool,
  mediaTool,
  memoryTool,
  remindersTool,
  webTool,
};

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export const ALL_BUILTIN_TOOLS: ToolSpec<any>[] = [
  obsidianTool,
  createFileTool,
  readFileTool,
  cliTool,
  mediaTool,
  memoryTool,
  remindersTool,
  webTool,
];

