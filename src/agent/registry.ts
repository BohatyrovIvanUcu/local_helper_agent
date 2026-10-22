import type { FunctionDeclaration } from "@google/genai";
import type { ToolSpec } from "./types.ts";
import { ALL_BUILTIN_TOOLS } from "./tools/index.ts";

export class ToolRegistry {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  private tools: Map<string, ToolSpec<any>>;

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  constructor(initialTools: ToolSpec<any>[] = []) {
    this.tools = new Map();
    for (const tool of initialTools) {
      this.register(tool);
    }
  }

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  public register(tool: ToolSpec<any>): void {
    this.tools.set(tool.name, tool);
  }

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  public get(name: string): ToolSpec<any> | undefined {
    return this.tools.get(name);
  }

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  public getAll(): ToolSpec<any>[] {
    return Array.from(this.tools.values());
  }

  public getFunctionDeclarations(): FunctionDeclaration[] {
    return this.getAll().map((t) => t.declaration);
  }

  public getPromptRules(): string[] {
    return this.getAll().map((t) => t.promptRule);
  }
}

export function createDefaultToolRegistry(): ToolRegistry {
  return new ToolRegistry(ALL_BUILTIN_TOOLS);
}

