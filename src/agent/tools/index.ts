import type { ToolSpec } from "../types.ts";
import { obsidianTool, manageObsidianTool } from "./obsidian.ts";
import { createFileTool, readFileTool } from "./files.ts";
import { cliTool } from "./cli.ts";
import { mediaTool } from "./media.ts";
import { memoryTool } from "./memory.ts";
import { remindersTool } from "./reminders.ts";
import { webTool } from "./web.ts";
import { displayTool } from "./display.ts";
import { convertFileTool } from "./converter.ts";
import { openDiskAnalyzerTool } from "./disk.ts";
import { shareFileWifiTool } from "./share.ts";
import { cleanDownloadsTool } from "./downloads.ts";
import { presetsTool } from "./presets.ts";

export {
  obsidianTool,
  manageObsidianTool,
  createFileTool,
  readFileTool,
  cliTool,
  mediaTool,
  memoryTool,
  remindersTool,
  webTool,
  displayTool,
  convertFileTool,
  openDiskAnalyzerTool,
  shareFileWifiTool,
  cleanDownloadsTool,
  presetsTool,
};

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export const ALL_BUILTIN_TOOLS: ToolSpec<any>[] = [
  obsidianTool,
  manageObsidianTool,
  createFileTool,
  readFileTool,
  cliTool,
  mediaTool,
  memoryTool,
  remindersTool,
  webTool,
  displayTool,
  convertFileTool,
  openDiskAnalyzerTool,
  shareFileWifiTool,
  cleanDownloadsTool,
  presetsTool,
];

