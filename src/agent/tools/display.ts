import { Type } from "@google/genai";
import type { ToolOutcome, ToolSpec } from "../types.ts";

export interface DisplayToolArgs {
  action: string;
  value?: number | null;
  enabled?: boolean | null;
  temperature?: number | null;
}

export const displayTool: ToolSpec<DisplayToolArgs> = {
  name: "control_display",
  uiLabel: "Екран & Світло",
  risk: "exec",
  idempotent: false,
  timeoutMs: 10000,
  declaration: {
    name: "control_display",
    description:
      "Керує яскравістю екрана комп'ютера та нічним режимом (теплим світлом / Night Light). Може встановлювати яскравість (0-100%), збільшувати/зменшувати її або вмикати/вимикати тепле світло для захисту очей.",
    parameters: {
      type: Type.OBJECT,
      properties: {
        action: {
          type: Type.STRING,
          description:
            "Дія для керування екраном: 'lock_screen' (блокування системи та пауза музики при відході), 'set_brightness', 'increase_brightness', 'decrease_brightness', 'get_brightness', 'enable_night_light', 'disable_night_light', 'toggle_night_light', 'get_night_light', 'set_night_light_temp'.",
        },
        value: {
          type: Type.INTEGER,
          description:
            "Необов'язковий відсоток яскравості (5-100) або крок зміни яскравості (наприклад, 10 або 30).",
        },
        enabled: {
          type: Type.BOOLEAN,
          description:
            "Необов'язковий прапорець для ввімкнення (true) або вимкнення (false) нічного режиму.",
        },
        temperature: {
          type: Type.INTEGER,
          description:
            "Необов'язкова колірна температура нічного світла в Кельвінах (наприклад, 2700 для теплого світла, 4000 для помірного).",
        },
      },
      required: ["action"],
    },
  },
  promptRule:
    "control_display: керування яскравістю дисплея, нічним режимом (теплим світлом) та блокуванням екрана. Коли користувач просить 'зроби яскравіше/темніше', 'яскравість 50%', 'увімкни нічний режим', 'я відійшов', 'заблокуй екран/комп' — ЗАВЖДИ викликай control_display з відповідною дією.",
  validateArgs: (raw) => {
    const rawAction = raw.action ?? "get_brightness";
    const action = typeof rawAction === "string" ? rawAction.trim() : "get_brightness";
    const value =
      typeof raw.value === "number" && !isNaN(raw.value) ? Math.round(raw.value) : null;
    const enabled = typeof raw.enabled === "boolean" ? raw.enabled : null;
    const temperature =
      typeof raw.temperature === "number" && !isNaN(raw.temperature)
        ? Math.round(raw.temperature)
        : null;

    return {
      ok: true,
      args: { action, value, enabled, temperature },
    };
  },
  execute: async (args, ctx): Promise<ToolOutcome> => {
    try {
      const res = await ctx.invoke<string>("control_display", {
        action: args.action,
        value: args.value,
        enabled: args.enabled,
        temperature: args.temperature,
      });
      return {
        ok: true,
        summary: res,
        data: { result: res },
      };
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      return {
        ok: false,
        code: "execution_error",
        summary: `Помилка керування екраном: ${msg}`,
        data: { error: msg },
      };
    }
  },
};

