import type {
  AskMascotCallbacks,
  InvokeFn,
  ToolContext,
  ToolOutcome,
  WorkingToolName,
} from "./types.ts";
import { ToolRegistry } from "./registry.ts";
import { evaluatePolicy } from "./policy.ts";

export interface ModelResponse {
  text?: string;
  functionCalls?: Array<{
    id?: string;
    name?: string;
    args?: Record<string, unknown>;
  }>;
  candidates?: Array<{ content?: Record<string, unknown> }>;
}

export interface ModelClient {
  generateContent: (params: {
    model: string;
    contents: Array<Record<string, unknown>>;
    config: Record<string, unknown>;
  }) => Promise<ModelResponse>;
}

export function stripToolExecutionNotice(text: string): string {
  if (!text) return "";
  let cleaned = text.trim();
  while (/^\[[^\]]+\]\s*/.test(cleaned)) {
    cleaned = cleaned.replace(/^\[[^\]]+\]\s*/, "").trim();
  }
  // Захист: системні XML-теги пісочниці ніколи не повинні з'являтися у хмарці маскота
  cleaned = cleaned.replace(/<\/?untrusted_web_content[^>]*>/gi, "").trim();
  return cleaned;
}

export function stripConversationalNoise(text: string): string {
  // Колапс емоційно розтягнутих літер: "офффни" -> "офни", "стоооп" -> "стоп", "врууубай" -> "врубай"
  const collapsed = text.replace(/([\p{L}])\1{2,}/giu, "$1");
  const noisePattern =
    /(?:^|[^\p{L}\p{N}])(?:будь\s*ласка|будь-ласка|пж|пліз|please|сер|шеф|бро|друг|маскот|давай|ану|швидко|вже|ну|типу|можеш|можна|міг\s*би|хотілося\s*б|до\s*біса|к\s*бісу|нафіг|блін|нахрін|чорт)(?=[^\p{L}\p{N}]|$)/giu;
  let res = collapsed;
  while (noisePattern.test(res)) {
    res = res.replace(noisePattern, " ");
  }
  return res.replace(/\s+/g, " ").trim();
}

export function detectDirectMediaCommand(
  prompt: string
): { action: string; query?: string | null } | null {
  const raw = prompt.trim().toLowerCase().replace(/[!.?,;]+$/g, "").trim();
  // Очищення від типових розмовних часток та звертань ("сер", "будь ласка", "пж", "шеф", "маскот" тощо)
  const clean = stripConversationalNoise(raw);

  // 1. Пауза / зупинка / вимкнення музики або вимога тиші
  if (
    /^(?:пауза|pause|stop|стоп|паузу|на\s+паузу|зупини|стоп\s+музика|зупинити|офни|офф|off|вимкни|виключи|вируби|потуши|затуши|погаси|заглуши|приглуши)$/iu.test(
      clean
    ) ||
    // Дія паузи з музикою: (потушити/вимкнути/виключити/вирубити/зупинити/заглушити/приглушити/офнути/поставити на паузу) + (музику/трек/пісню/плеєр)
    /(?:потуш\p{L}*|затуш\p{L}*|погас\p{L}*|вимкн\p{L}*|виключ\p{L}*|вируб\p{L}*|заглуш\p{L}*|приглуш\p{L}*|зупин\p{L}*|стоп\p{L}*|оф\p{L}*|пауз\p{L}*|pause|stop|off)\s+(?:музик\p{L}*|трек\p{L}*|пісн\p{L}*|плеєр\p{L}*)/iu.test(
      clean
    ) ||
    // Музику на паузу / музику вимкни / музику потуши / музику офни
    /(?:музик\p{L}*|трек\p{L}*|пісн\p{L}*)\s+(?:на\s+)?(?:пауз\p{L}*|стоп\p{L}*|оф\p{L}*|вимкн\p{L}*|виключ\p{L}*|вируб\p{L}*|потуш\p{L}*|затуш\p{L}*|погас\p{L}*|зупин\p{L}*)/iu.test(
      clean
    ) ||
    // Потреба в тиші: (треба|хочу|зроби|дай|організуй)\s+(тиші/тишу/тишину)
    /(?:треба|хочу|зроби|дай|організуй)\s+(?:тиш\p{L}*)/iu.test(clean)
  ) {
    return { action: "pause" };
  }

  // 2. Відтворення / запуск музики
  if (
    /^(?:play|плей|грай|віднови|відновити|продовжи|продовжити|продовжуй|resume|старт|start|музику|музика|музичка|музичко)$/iu.test(
      clean
    ) ||
    /(?:включ\p{L}*|увімкн\p{L}*|запуст\p{L}*|вруб\p{L}*|постав\p{L}*|грай\p{L}*|продовж\p{L}*|віднов\p{L}*|дай|підкин\p{L}*|хочу)\s+(?:музик\p{L}*|трек\p{L}*|пісн\p{L}*|плейлист\p{L}*|щось послухати)/iu.test(
      clean
    ) ||
    /(?:музик\p{L}*|трек\p{L}*|пісн\p{L}*)\s+(?:включ\p{L}*|увімкн\p{L}*|запуст\p{L}*|вруб\p{L}*|постав\p{L}*|грай\p{L}*|віднов\p{L}*|продовж\p{L}*|дай)/iu.test(
      clean
    )
  ) {
    return { action: "play" };
  }

  // 2.1 Улюблені треки / плейлист лайків: "мої лайки", "включи те що я люблю слухати", "включи мої треки", "лайкнуті"
  if (
    /(?:лайк\p{L}*|улюблен\p{L}*|те\s+що\s+я\s+люблю|мої\s+треки|мої\s+пісні|моя\s+музика|що\s+я\s+слухаю)/iu.test(
      clean
    )
  ) {
    return { action: "play", query: "liked" };
  }

  // 2.2 Запуск конкретної пісні: "включи пісню <назва>", "увімкни трек <назва>", "play <назва>"
  const playQueryMatch =
    clean.match(
      /^(?:включи|увімкни|запусти|вруби|врубай|постав|play|дай|підкинь)\s+(?:пісню|трек|музику|виконавця|альбом|радіо)\s+(.+)$/i
    ) || clean.match(/^play\s+(.+)$/i);
  if (playQueryMatch) {
    const q = playQueryMatch[1].trim();
    if (
      q &&
      ![
        "музику",
        "музика",
        "музики",
        "музичко",
        "щось",
        "плей",
        "play",
      ].includes(q)
    ) {
      return { action: "play", query: q };
    }
  }

  // 3. Наступний трек (Next / Skip)
  if (
    /^(?:next|next\s+track|next\s+song|скіп|skip|skip\s+track|skip\s+song|далі|наступн\p{L}*|переключ\p{L}*|перемкн\p{L}*|пропусти|перегорни|скип\p{L}*)$/iu.test(
      clean
    ) ||
    /(?:наступн\p{L}*|інш\p{L}*)\s+(?:трек\p{L}*|пісн\p{L}*|музик\p{L}*)/iu.test(
      clean
    ) ||
    /(?:включ\p{L}*|увімкн\p{L}*|постав\p{L}*|дай|давай|переключ\p{L}*|перемкн\p{L}*|скіпн\p{L}*|скипн\p{L}*)\s+(?:наступн\p{L}*|інш\p{L}*|далі)/iu.test(
      clean
    ) ||
    /(?:переключ\p{L}*|перемкн\p{L}*|скіпн\p{L}*|скипн\p{L}*)\s+(?:трек\p{L}*|пісн\p{L}*)/iu.test(
      clean
    ) ||
    /^(?:next|skip)\s+(?:track|song|music)$/iu.test(clean)
  ) {
    return { action: "next" };
  }

  // 4. Попередній трек (Prev / Back)
  if (
    /^(?:prev|previous|previous\s+track|previous\s+song|prev\s+track|prev\s+song|назад|попередн\p{L}*)$/iu.test(
      clean
    ) ||
    /(?:попередн\p{L}*|минул\p{L}*)\s+(?:трек\p{L}*|пісн\p{L}*|музик\p{L}*)/iu.test(
      clean
    ) ||
    /(?:включ\p{L}*|увімкн\p{L}*|постав\p{L}*|поверни|верни|переключ\p{L}*|перемкн\p{L}*)\s+(?:попередн\p{L}*|назад)/iu.test(
      clean
    ) ||
    /^(?:prev|previous)\s+(?:track|song|music)$/iu.test(clean)
  ) {
    return { action: "prev" };
  }

  // 5. Встановлення точного відсотка гучності (наприклад: "гучність 0", "звук 50%", "зроби звук 30%")
  const volPctMatch =
    clean.match(/(?:гучн\S*|звук\S*|volume)\s*(?:на|до|у|в)?\s*(\d{1,3})\s*%?/iu) ||
    clean.match(/(?:постав|зроби|встанови|зменш|збільш)\s*(?:гучн\S*|звук\S*)?(?:\s+(?:на|до))?\s+(\d{1,3})\s*%?/iu);
  if (volPctMatch) {
    const val = parseInt(volPctMatch[1], 10);
    if (!isNaN(val) && val >= 0 && val <= 100) {
      return { action: "set_volume", query: String(val) };
    }
  }

  // 6. Зменшення гучності (тихіше)
  if (
    /(?:тихіш|тиішх|притиш|quieter|volume\s+down)/iu.test(clean) ||
    /(?:зменш|знизь|приглуш)\s+(?:звук|гучн)/iu.test(clean)
  ) {
    return { action: "volume_down" };
  }

  // 7. Збільшення гучності (голосніше / гучніше)
  if (
    /(?:голосніш|гучніш|louder|volume\s+up)/iu.test(clean) ||
    /(?:додай|підніми|підвищ|збільш)\s+(?:звук|гучн)/iu.test(clean)
  ) {
    return { action: "volume_up" };
  }

  // 8. Вимкнення звуку (Mute)
  if (
    /(?:вимкн\p{L}*|виключ\p{L}*|заглуш\p{L}*)\s+звук/iu.test(clean) ||
    /^(?:без\s+звуку|мут|mute|повна\s+тиша)$/iu.test(clean)
  ) {
    return { action: "mute" };
  }

  // 9. Увімкнення звуку (Unmute)
  if (
    /(?:увімкн\p{L}*|включ\p{L}*|поверн\p{L}*|верн\p{L}*)\s+звук/iu.test(clean) ||
    /^(?:анмут|unmute)$/iu.test(clean)
  ) {
    return { action: "unmute" };
  }

  // 10. Запит поточної гучності
  if (
    [
      "яка гучність",
      "яка зараз гучність",
      "поточна гучність",
      "рівень звуку",
      "рівень гучності",
    ].includes(clean)
  ) {
    return { action: "get_volume" };
  }

  return null;
}

export interface DirectDisplayCommand {
  action: string;
  value?: number | null;
  enabled?: boolean | null;
  temperature?: number | null;
}

export function detectDirectDisplayCommand(prompt: string): DirectDisplayCommand | null {
  const raw = prompt.trim().toLowerCase().replace(/[!.?,;]+$/g, "").trim();
  const clean = stripConversationalNoise(raw);

  // 0. Блокування екрана / Kill Switch при відході ("я відійшов", "відійшов", "заблокуй екран", "lock", "kill switch")
  if (
    /^(?:я\s+)?(?:відійшов|відходжу|пішов|відлучусь|відлучився)$/iu.test(clean) ||
    /^(?:заблокуй|блокуй)\s+(?:екран|комп|пк|систему|робочий\s+стіл|сесію)$/iu.test(clean) ||
    /^(?:lock|lock\s+screen|kill\s*switch|кілсвіч|кіл\s*свіч|блокуй|заблокуй)$/iu.test(clean) ||
    /(?:заблокуй|блокуй)\s+(?:екран|комп|пк)/iu.test(clean)
  ) {
    return { action: "lock_screen" };
  }

  // 1. Нічний режим / тепле світло
  if (
    /^(?:night\s+light|night\s+light\s+on)$/iu.test(clean) ||
    /(?:включ\p{L}*|увімкн\p{L}*|постав\p{L}*|зроби|дай)\s+(?:нічн\p{L}*|тепл\p{L}*|жовт\p{L}*)\s+(?:режим\p{L}*|світл\p{L}*|екран\p{L}*)/iu.test(
      clean
    ) ||
    /^(?:нічний\s+режим|нічне\s+світло|тепле\s+світло|теплий\s+режим|теплий\s+екран|жовтий\s+екран|жовте\s+світло)$/iu.test(
      clean
    )
  ) {
    return { action: "enable_night_light" };
  }

  if (
    /^(?:night\s+light\s+off)$/iu.test(clean) ||
    /(?:вимкн\p{L}*|виключ\p{L}*|погас\p{L}*|потуш\p{L}*|скин\p{L}*)\s+(?:нічн\p{L}*|тепл\p{L}*|жовт\p{L}*)\s+(?:режим\p{L}*|світл\p{L}*|екран\p{L}*)/iu.test(
      clean
    ) ||
    /^(?:холодний\s+екран|холодне\s+світло|денне\s+світло|звичайне\s+світло|нейтральний\s+екран)$/iu.test(
      clean
    )
  ) {
    return { action: "disable_night_light" };
  }

  if (
    /(?:перемкн\p{L}*|переключ\p{L}*|тогл)\s+(?:нічн\p{L}*|тепл\p{L}*)/iu.test(clean)
  ) {
    return { action: "toggle_night_light" };
  }

  if (
    /(?:який\s+режим|статус\s+нічн\p{L}*|чи\s+(?:увімкнен|включен)\p{L}*\s+нічн\p{L}*)/iu.test(clean)
  ) {
    return { action: "get_night_light" };
  }

  // 2. Точний відсоток яскравості
  const pctMatch =
    clean.match(/(?:яскрав\S*|чскрав\S*|екран\S*|brightness)\s*(?:на|до|у|в)?\s*(\d{1,3})\s*%?/iu) ||
    clean.match(/(?:постав|зроби|встанови|зменш|збільш|викрути)\s*(?:яскрав\S*|чскрав\S*|екран\S*)?\s*(?:на|до)?\s*(\d{1,3})\s*%?/iu);
  if (pctMatch) {
    const val = parseInt(pctMatch[1], 10);
    if (!isNaN(val) && val >= 0 && val <= 100) {
      return { action: "set_brightness", value: Math.max(5, val) };
    }
  }

  // 3. Збільшення / зменшення яскравості
  if (
    /^(?:яскравіше|світліше|brighter)$/iu.test(clean) ||
    /(?:додай|підніми|підвищ|збільш)\s+(?:яскрав\S*|чскрав\S*|світл\S*)/iu.test(clean) ||
    /(?:зроби\s+)?(?:екран\p{L}*\s+)?(?:яскравіш\p{L}*|світліш\p{L}*)/iu.test(clean)
  ) {
    return { action: "increase_brightness", value: 10 };
  }

  if (
    /^(?:темніше|dimmer)$/iu.test(clean) ||
    /(?:зменш|знизь|приглуши?)\s+(?:яскрав\S*|чскрав\S*|світл\S*)/iu.test(clean) ||
    /(?:зроби\s+)?(?:екран\p{L}*\s+)?(?:темніш\p{L}*|приглуш\p{L}*)/iu.test(clean)
  ) {
    return { action: "decrease_brightness", value: 10 };
  }

  // 4. Запит поточної яскравості
  if (
    [
      "яка яскравість",
      "яка зараз яскравість",
      "поточна яскравість",
      "яскравість екрана",
      "яка яскравість екрана",
    ].includes(clean)
  ) {
    return { action: "get_brightness" };
  }

  return null;
}

export interface AttachedFileMeta {
  path: string;
  name: string;
  size?: number;
  isDirectory?: boolean;
}

export interface DirectConvertCommand {
  inputPath: string;
  targetFormat: string;
  customOutputPath?: string | null;
}

export function detectDirectConvertCommand(
  prompt: string,
  attachedPath?: string | null
): DirectConvertCommand | null {
  if (!attachedPath) return null;
  const raw = prompt.trim().toLowerCase().replace(/[!.?,;]+$/g, "").trim();
  const clean = stripConversationalNoise(raw);

  // 1. Специфічні фрази: "зроби гіфку", "гіфка", "gif", "витягни аудіо/звук", "заархівуй"
  if (
    /^(?:зроби\s+)?(?:гіфку|гифку|гіф|гиф|gif|анімацію)$/iu.test(clean) ||
    /(?:конвертуй|перетвори|зроби|to)\s+(?:це\s+)?(?:в|у|to)?\s*(?:гіфку|гифку|гіф|gif)/iu.test(
      clean
    )
  ) {
    return { inputPath: attachedPath, targetFormat: "gif" };
  }

  if (
    /(?:витягни|дістань|витяг|екстракт)\s+(?:аудіо|звук|доріжку|пісню)/iu.test(clean) ||
    /^(?:аудіо|звук|доріжка)$/iu.test(clean)
  ) {
    return { inputPath: attachedPath, targetFormat: "mp3" };
  }

  if (
    /^(?:заархівуй|запакуй|архів|зроби\s+архів|в\s+архів|у\s+архів)$/iu.test(clean) ||
    /(?:запакуй|заархівуй|архівуй)\s+(?:в|у)?\s*(?:zip|архів)/iu.test(clean)
  ) {
    return { inputPath: attachedPath, targetFormat: "zip" };
  }

  // Розархівація / розпакування: "розархівуй", "розпакуй", "видобудь", "unzip", "extract"
  if (
    /^(?:розархівуй|розархівувати|розпакуй|розпакувати|видобудь|видобути|unzip|extract|unarchive)$/iu.test(
      clean
    ) ||
    /(?:розархівуй|розархівувати|розпакуй|розпакувати|видобудь|видобути|unzip|extract)\s+(?:це\s+|архів\s+|файл\s+|папку\s+)?/iu.test(
      clean
    ) ||
    /(?:витягни|дістань)\s+(?:це\s+)?(?:з|із)\s+архів/iu.test(clean) ||
    /^(?:в\s+папку|у\s+папку)$/iu.test(clean)
  ) {
    return { inputPath: attachedPath, targetFormat: "unzip" };
  }

  // Специфічні фрази для перетворення в текст: "переведи в текст", "зроби txt", "в текст", "витягни текст"
  if (
    /^(?:текст|txt|text|у\s+текст|в\s+текст)$/iu.test(clean) ||
    /(?:переведи|конвертуй|перетвори|витягни|збережи|дістань|екстракт)\s+(?:це\s+)?(?:в|у|to)?\s*(?:текст|текстовий|txt|text)/iu.test(
      clean
    ) ||
    /(?:в|у|to)\s+(?:текст|txt|text)$/iu.test(clean)
  ) {
    return { inputPath: attachedPath, targetFormat: "txt" };
  }

  // Специфічні фрази для перетворення в Markdown (через Microsoft MarkItDown): "переведи в markdown", "зроби md", "в markdown"
  if (
    /^(?:markdown|маркдаун|md|у\s+markdown|в\s+markdown|у\s+маркдаун|в\s+маркдаун|у\s+md|в\s+md)$/iu.test(
      clean
    ) ||
    /(?:переведи|конвертуй|перетвори|експортуй|збережи|дістань|convert)\s+(?:це\s+)?(?:в|у|to)?\s*(?:markdown|маркдаун|md)/iu.test(
      clean
    ) ||
    /(?:в|у|to)\s+(?:markdown|маркдаун|md)$/iu.test(clean)
  ) {
    return { inputPath: attachedPath, targetFormat: "md" };
  }

  // 2. Шаблони "конвертуй у [формат]", "перетвори в [формат]", "зроби [формат]", "в [формат]", "to [format]"
  const match =
    /^(?:конвертуй|перетвори|зроби|трансформуй|збережи|convert)\s+(?:це\s+|його\s+|файл\s+|відео\s+|папку\s+)?(?:в|у|to|як|as)\s+([a-zA-Z0-9.]+)/iu.exec(
      clean
    ) ||
    /^(?:в|у|to)\s+([a-zA-Z0-9.]+)/iu.exec(clean) ||
    /^(?:зроби|створи|make)\s+([a-zA-Z0-9.]+)/iu.exec(clean);

  if (match && match[1]) {
    const rawTarget = match[1].replace(/^\.+/, "").toLowerCase();
    const validFormats = [
      "gif",
      "mp3",
      "wav",
      "mp4",
      "mkv",
      "webm",
      "avi",
      "mov",
      "flac",
      "ogg",
      "aac",
      "m4a",
      "webp",
      "png",
      "jpg",
      "jpeg",
      "ico",
      "bmp",
      "pdf",
      "txt",
      "text",
      "md",
      "markdown",
      "zip",
      "tar.gz",
      "tar",
      "html",
      "unzip",
      "extract",
      "unarchive",
    ];
    if (validFormats.includes(rawTarget)) {
      const finalTarget =
        rawTarget === "text"
          ? "txt"
          : rawTarget === "markdown"
          ? "md"
          : rawTarget === "extract" || rawTarget === "unarchive"
          ? "unzip"
          : rawTarget;
      return { inputPath: attachedPath, targetFormat: finalTarget };
    }
  }

  return null;
}

export function detectDirectDiskCommand(
  prompt: string
): { path?: string } | null {
  const raw = prompt.trim().toLowerCase().replace(/[!.?,;]+$/g, "").trim();
  const clean = stripConversationalNoise(raw);

  if (
    /^(?:що\s+забило\s+диск|що\s+забило\s+пам['ʼ]?ять|покажи\s+диск|візуалізатор\s+диска|wiztree|аналізатор\s+диска|карта\s+диска|де\s+місце\s+на\s+диску|дисковий\s+простір|що\s+з\s+диском)$/iu.test(
      clean
    ) ||
    /(?:відкрий|покажи|запусти|перевір)\s+(?:wiztree|аналізатор\s+диск|візуалізатор\s+диск|карту\s+диск|диск)/iu.test(
      clean
    )
  ) {
    return { path: "~" };
  }
  return null;
}

export function detectDirectShareCommand(
  prompt: string,
  attachedPath?: string | null
): { filePath: string } | null {
  const raw = prompt.trim().toLowerCase().replace(/[!.?,;]+$/g, "").trim();
  const clean = stripConversationalNoise(raw);

  // Словник дій передачі (кинь, скинь, закинь, перекинь, роздай тощо)
  const shareActions =
    "(?:кинь|кинути|закинь|закинути|скинь|скинути|перекинь|перекинути|кидай|скидай|перекидай|відправ|відправити|надішли|надіслати|поділись|поділися|поділитися|роздай|роздати|share)";
  // Словник цілей (телефон, смартфон, мобілка, вайфай, qr)
  const shareDestinations =
    "(?:на\\s+телефон|на\\s+смартфон|на\\s+мобілку|на\\s+мобільний|на\\s+трубку|по\\s+вайфаю|по\\s+wi-?fi|wifi\\s+share|через\\s+qr|по\\s+qr|qr|телефон|смартфон)";

  const isShareIntent =
    new RegExp(
      `^(?:${shareActions}\\s+${shareDestinations}|${shareActions}|${shareDestinations})$`,
      "iu"
    ).test(clean) ||
    new RegExp(
      `${shareActions}\\s+(?:це\\s+|файл\\s+|папку\\s+)?(?:${shareDestinations})`,
      "iu"
    ).test(clean) ||
    new RegExp(
      `(?:${shareDestinations})\\s+(?:це\\s+|файл\\s+|папку\\s+)?(?:${shareActions})`,
      "iu"
    ).test(clean);

  if (!isShareIntent) return null;

  if (attachedPath) {
    return { filePath: attachedPath };
  }

  // Якщо файл не прикріплено через DnD, спробуємо знайти шлях у тексті запиту
  const pathMatch = prompt.match(/(?:^|\s)(~?\/[^\s]+|[a-zA-Z]:\\[^\s]+)/);
  if (pathMatch) {
    return { filePath: pathMatch[1].trim() };
  }

  return null;
}

export function detectDirectDownloadsCommand(
  prompt: string
): { action: "analyze" } | null {
  const raw = prompt.trim().toLowerCase().replace(/[!.?,;]+$/g, "").trim();
  const clean = stripConversationalNoise(raw);

  if (
    /^(?:прибери\s+завантаження|прибрати\s+завантаження|почисти\s+завантаження|очисти\s+завантаження|прибери\s+downloads|очисти\s+downloads|наведи\s+лад\s+у\s+завантаженнях|порядок\s+в\s+downloads|розумне\s+прибирання)$/iu.test(
      clean
    ) ||
    /(?:прибери|почисти|очисти|наведи\s+лад)\s+(?:в\s+|у\s+)?(?:завантаженнях|downloads)/iu.test(
      clean
    )
  ) {
    return { action: "analyze" };
  }
  return null;
}

export interface DirectPresetCommand {
  action: "open" | "save" | "close" | "list";
  name?: string;
}

export function detectDirectPresetCommand(
  prompt: string
): DirectPresetCommand | null {
  const raw = prompt.trim().toLowerCase().replace(/[!.?,;]+$/g, "").trim();
  const clean = stripConversationalNoise(raw);

  // 1. Перегляд списку збережених пресетів
  if (
    /^(?:які\s+(?:в\s+мене\s+)?є\s+пресети|покажи\s+(?:мої\s+)?пресети|мої\s+пресети|список\s+(?:моїх\s+)?пресетів|список\s+стеків|покажи\s+стеки|які\s+стеки)$/iu.test(
      clean
    )
  ) {
    return { action: "list" };
  }

  // 2. Збереження поточного стеку ("збережи поточний стек як [назва]", "збережи сесію як [назва]")
  const saveMatch =
    clean.match(
      /(?:збережи|зафіксуй|запам'ятай)\s+(?:поточний\s+|відкриті\s+)?(?:стек|сесію|вкладки|пресет)(?:\s+(?:як|під\s+назвою|з\s+назвою))?\s+([\p{L}\p{N}_-]+)/iu
    ) ||
    clean.match(/^(?:збережи|зафіксуй)\s+пресет\s+([\p{L}\p{N}_-]+)/iu);
  if (saveMatch && saveMatch[1]) {
    return { action: "save", name: saveMatch[1].trim() };
  }

  // 3. Закриття стеку ("закрий усе для навчання", "закрий робочий стек", "закрий стек [назва]")
  const closeMatch = clean.match(
    /^закрий\s+(?:усе\s+для\s+|все\s+для\s+|стек\s+|пресет\s+|сесію\s+)?([\p{L}\p{N}_-]+)/iu
  );
  if (closeMatch && closeMatch[1]) {
    const rawTarget = closeMatch[1].trim();
    if (["навчання", "уроки", "пар", "пари", "study"].includes(rawTarget)) {
      return { action: "close", name: "study" };
    }
    if (["роботи", "роботу", "робочий", "робота", "work"].includes(rawTarget)) {
      return { action: "close", name: "work" };
    }
    return { action: "close", name: rawTarget };
  }

  // 4. Пряме відкриття навчання / пар / уроків
  if (
    /^(?:відкрий\s+)?(?:стек\s+|пресет\s+)?(?:навчання|пари|уроки|універ|study)$/iu.test(
      clean
    ) ||
    /^(?:відкрий\s+навчання|стек\s+навчання|відкрий\s+стек\s+навчання|пресет\s+навчання)$/iu.test(
      clean
    )
  ) {
    return { action: "open", name: "study" };
  }

  // 5. Пряме відкриття роботи / робочого стеку
  if (
    /^(?:відкрий\s+)?(?:робочий\s+стек|стек\s+розробки|роботу|робота|work)$/iu.test(
      clean
    ) ||
    /^(?:відкрий\s+роботу|відкрий\s+робочий\s+стек|пресет\s+робота|стек\s+робота)$/iu.test(
      clean
    )
  ) {
    return { action: "open", name: "work" };
  }

  // 6. Загальне відкриття пресету за назвою ("відкрий пресет [назва]", "запусти пресет [назва]")
  const openMatch = clean.match(
    /^(?:відкрий|запусти|підніми)\s+(?:пресет|стек|сесію)\s+([\p{L}\p{N}_-]+)/iu
  );
  if (openMatch && openMatch[1]) {
    return { action: "open", name: openMatch[1].trim() };
  }

  return null;
}

export interface RunTurnOptions {
  modelClient: ModelClient;
  modelName: string;
  systemInstruction: string;
  userPrompt: string;
  attachments?: Array<Record<string, unknown>>;
  attachedFile?: AttachedFileMeta | null;
  conversationHistory?: Array<Record<string, unknown>>;
  toolRegistry: ToolRegistry;
  invoke: InvokeFn;
  callbacks: AskMascotCallbacks;
  vaultPath?: string;
  activeWorkspacePath?: string | null;
  activeFile?: string | null;
  maxIterations?: number;
}

export interface TurnResult {
  text: string;
  executedTools: string[];
}

export async function runAgentTurn(options: RunTurnOptions): Promise<TurnResult> {
  const {
    modelClient,
    modelName,
    systemInstruction,
    userPrompt,
    attachments = [],
    conversationHistory = [],
    toolRegistry,
    invoke,
    callbacks,
    vaultPath,
    maxIterations = 5,
  } = options;

  // Пряме миттєве виконання медіа-команд (resume, pause, next, prev тощо) без затримки на LLM
  const directMedia = detectDirectMediaCommand(userPrompt);
  if (directMedia && attachments.length === 0) {
    callbacks.onStateChange("WORKING", "control_media");
    try {
      const summary = await invoke<string>("control_media", {
        action: directMedia.action,
        query: directMedia.query ?? null,
      });

      if (userPrompt.trim() && summary) {
        try {
          await invoke("append_conversation_turn", {
            user_text: userPrompt.trim(),
            model_text: summary,
            tools_used: [`control_media (${directMedia.action})`],
            userText: userPrompt.trim(),
            modelText: summary,
            toolsUsed: [`control_media (${directMedia.action})`],
          });
        } catch {
          // ignore memory error
        }
      }

      return {
        text: summary,
        executedTools: ["control_media"],
      };
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      return {
        text: `Помилка керування медіа: ${msg}`,
        executedTools: ["control_media"],
      };
    }
  }

  // Пряме миттєве виконання команд яскравості та нічного режиму без затримки на LLM (<30 мс)
  const directDisplay = detectDirectDisplayCommand(userPrompt);
  if (directDisplay && attachments.length === 0) {
    callbacks.onStateChange("WORKING", "control_display");
    try {
      const summary = await invoke<string>("control_display", {
        action: directDisplay.action,
        value: directDisplay.value ?? null,
        enabled: directDisplay.enabled ?? null,
        temperature: directDisplay.temperature ?? null,
      });

      if (userPrompt.trim() && summary) {
        try {
          await invoke("append_conversation_turn", {
            user_text: userPrompt.trim(),
            model_text: summary,
            tools_used: [`control_display (${directDisplay.action})`],
            userText: userPrompt.trim(),
            modelText: summary,
            toolsUsed: [`control_display (${directDisplay.action})`],
          });
        } catch {
          // ignore memory error
        }
      }

      return {
        text: summary,
        executedTools: ["control_display"],
      };
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      return {
        text: `Помилка керування дисплеєм: ${msg}`,
        executedTools: ["control_display"],
      };
    }
  }

  // Пряме миттєве виконання команд конвертації файлів (у gif, в mp3, запакуй в zip тощо)
  const directConvert = detectDirectConvertCommand(userPrompt, options.attachedFile?.path);
  if (directConvert) {
    callbacks.onStateChange("WORKING", "convert_file");
    try {
      interface BackendConversionResult {
        success: boolean;
        input_path: string;
        output_path: string;
        output_filename: string;
        output_size_bytes: number;
        engine_used: string;
        message: string;
      }
      const res = await invoke<BackendConversionResult>("convert_file", {
        input_path: directConvert.inputPath,
        target_format: directConvert.targetFormat,
        custom_output_path: directConvert.customOutputPath ?? null,
      });

      const sizeKb = (res.output_size_bytes / 1024).toFixed(1);
      const ext = directConvert.targetFormat.toLowerCase();
      const isExtract = ext === "unzip" || ext === "extract" || ext === "unarchive";
      const badge = isExtract
        ? "📂"
        : ["mp3", "wav", "flac", "ogg", "aac", "m4a", "opus"].includes(ext)
        ? "🎵"
        : ["mp4", "webm", "mkv", "avi", "mov", "gif"].includes(ext)
        ? "🎬"
        : ["zip", "tar", "tar.gz"].includes(ext)
        ? "📦"
        : ["png", "jpg", "jpeg", "webp", "bmp", "ico"].includes(ext)
        ? "🖼️"
        : "📄";

      const encodedPath = encodeURIComponent(res.output_path);
      const summary = isExtract
        ? `Готово! Архів розпаковано у папку «${res.output_filename}» (${sizeKb} KB) 📂 [Відкрити папку](action://open?path=${encodedPath}) • [У файловому менеджері](action://reveal?path=${encodedPath})`
        : `Готово! Створено ${res.output_filename} (${sizeKb} KB) ${badge} [Відкрити файл](action://open?path=${encodedPath}) • [У папці](action://reveal?path=${encodedPath})`;

      if (userPrompt.trim() && summary) {
        try {
          await invoke("append_conversation_turn", {
            user_text: userPrompt.trim(),
            model_text: summary,
            tools_used: [`convert_file (${directConvert.targetFormat})`],
            userText: userPrompt.trim(),
            modelText: summary,
            toolsUsed: [`convert_file (${directConvert.targetFormat})`],
          });
        } catch {
          // ignore memory error
        }
      }

      return {
        text: summary,
        executedTools: ["convert_file"],
      };
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      return {
        text: `Помилка конвертації: ${msg}`,
        executedTools: ["convert_file"],
      };
    }
  }

  // Пряме відкриття візуалізатора диска WizTree без затримки на LLM
  const directDisk = detectDirectDiskCommand(userPrompt);
  if (directDisk && attachments.length === 0) {
    callbacks.onStateChange("WORKING", "open_disk_analyzer");
    try {
      await invoke("open_disk_analyzer_window", {
        path: directDisk.path || "~",
      });
      callbacks.onStateChange("SUCCESS");
      const summary =
        "Відкрив візуалізатор дискового простору WizTree! 🗺️ Тут ви можете побачити дерево папок, розширення та очистити важкі папки.";

      return {
        text: summary,
        executedTools: ["open_disk_analyzer"],
      };
    } catch (err) {
      callbacks.onStateChange("ERROR");
      const msg = err instanceof Error ? err.message : String(err);
      return {
        text: `Помилка відкриття WizTree: ${msg}`,
        executedTools: ["open_disk_analyzer"],
      };
    }
  }

  // Пряма роздача файлу/папки по Wi-Fi при наявності прикріплення
  const directShare = detectDirectShareCommand(
    userPrompt,
    options.attachedFile?.path
  );
  if (directShare) {
    callbacks.onStateChange("WORKING", "share_file_wifi");
    try {
      interface ShareSessionResponse {
        share_url: string;
        file_name: string;
        file_size: number;
        is_dir: boolean;
      }
      const session = await invoke<ShareSessionResponse>("start_file_share", {
        filePath: directShare.filePath,
      });
      await invoke("open_share_window", {
        filePath: directShare.filePath,
      });
      callbacks.onStateChange("SUCCESS");
      const isHttps = session.share_url.startsWith("https://");
      const title = isHttps ? "Запустив захищену HTTPS-роздачу" : "Запустив Wi-Fi роздачу";
      const summary = `${title} для «${session.file_name}»! 📲\nURL: ${session.share_url}\nВідкрито вікно з QR-кодом для легкого сканування телефоном.`;

      return {
        text: summary,
        executedTools: ["share_file_wifi"],
      };
    } catch (err) {
      callbacks.onStateChange("ERROR");
      const msg = err instanceof Error ? err.message : String(err);
      return {
        text: `Помилка запуску роздачі: ${msg}`,
        executedTools: ["share_file_wifi"],
      };
    }
  }

  // Пряме миттєве виконання команд робочих сесій/пресетів (<30 мс)
  const directPreset = detectDirectPresetCommand(userPrompt);
  if (directPreset && attachments.length === 0) {
    callbacks.onStateChange("WORKING", "manage_session_presets");
    try {
      const summary = await invoke<string>("manage_session_presets", {
        action: directPreset.action,
        name: directPreset.name ?? null,
      });

      if (userPrompt.trim() && summary) {
        try {
          await invoke("append_conversation_turn", {
            user_text: userPrompt.trim(),
            model_text: summary,
            tools_used: [`manage_session_presets (${directPreset.action})`],
            userText: userPrompt.trim(),
            modelText: summary,
            toolsUsed: [`manage_session_presets (${directPreset.action})`],
          });
        } catch {
          // ignore memory error
        }
      }

      callbacks.onStateChange("SUCCESS");
      return {
        text: summary,
        executedTools: ["manage_session_presets"],
      };
    } catch (err) {
      callbacks.onStateChange("ERROR");
      const msg = err instanceof Error ? err.message : String(err);
      return {
        text: `Помилка керування робочим стеком: ${msg}`,
        executedTools: ["manage_session_presets"],
      };
    }
  }

  const ctx: ToolContext = {
    invoke,
    confirm: callbacks.onConfirmRequest,
    now: new Date(),
    vaultPath,
    tainted: false,
    activeWorkspacePath: options.activeWorkspacePath,
    activeFile: options.activeFile,
  };

  const executedToolsLog: string[] = [];
  const executedToolsDetails: string[] = [];

  // Формуємо початкові contents
  const contents: Array<Record<string, unknown>> = [...conversationHistory];

  const userParts: Array<Record<string, unknown>> = [];
  for (const att of attachments) {
    userParts.push(att);
  }
  let promptText = userPrompt.trim();
  if (options.attachedFile) {
    let filePreview = "";
    const ext = options.attachedFile.name.split(".").pop()?.toLowerCase();
    const textLikeExts = [
      "txt",
      "md",
      "markdown",
      "csv",
      "json",
      "log",
      "pdf",
      "docx",
      "doc",
      "xlsx",
      "xls",
      "pptx",
      "ppt",
      "html",
      "htm",
      "xml",
    ];
    if (!options.attachedFile.isDirectory && ext && textLikeExts.includes(ext)) {
      try {
        const fileContent = await invoke<string>("read_file_or_dir", {
          target_path: options.attachedFile.path,
        });
        if (
          fileContent &&
          !fileContent.startsWith("Не вдалося") &&
          !fileContent.startsWith("Доступ заборонено")
        ) {
          filePreview = `\n\n[Текстовий вміст прикріпленого файлу "${options.attachedFile.name}"]:\n"""\n${fileContent.slice(0, 10000)}\n"""`;
        }
      } catch {
        // Залишаємо базовий опис шляху
      }
    }
    const fileDesc = `[Прикріплений локальний файл/папка користувача: "${options.attachedFile.name}" (шлях: ${options.attachedFile.path}${
      options.attachedFile.size ? `, розмір: ${options.attachedFile.size} байтів` : ""
    }${options.attachedFile.isDirectory ? ", це папка/директорія" : ""})]${filePreview}`;
    promptText = promptText ? `${fileDesc}\n\n${promptText}` : fileDesc;
  }
  if (promptText) {
    userParts.push({ text: promptText });
  }

  contents.push({
    role: "user",
    parts: userParts,
  });

  callbacks.onStateChange("THINKING");

  const functionDeclarations = toolRegistry.getFunctionDeclarations();
  let lastToolSummary: string | undefined;

  for (let iteration = 0; iteration < maxIterations; iteration++) {
    let response: ModelResponse;

    if (iteration > 0 && lastToolSummary) {
      // Для повторних кроків (генерація персонажної відповіді після виконання інструментів):
      // якщо Gemini затримується довше таймауту, повертаємо готовий результат виконаного інструменту.
      // Для читання/саммарі вебсторінок даємо достатньо часу (14 с) для якісного аналізу тексту.
      const isWebSummary = executedToolsLog.includes("summarize_webpage");
      const fallbackTimeoutMs = isWebSummary ? 14000 : 7000;
      const fallbackSummary = stripToolExecutionNotice(lastToolSummary);
      let timer: ReturnType<typeof setTimeout> | undefined;
      const timeoutPromise = new Promise<ModelResponse>((resolve) => {
        timer = setTimeout(() => {
          resolve({ text: fallbackSummary });
        }, fallbackTimeoutMs);
      });

      try {
        response = await Promise.race([
          modelClient.generateContent({
            model: modelName,
            contents,
            config: {
              systemInstruction: {
                parts: [{ text: systemInstruction }],
              },
              tools: [{ functionDeclarations }],
              temperature: 0.2,
            },
          }),
          timeoutPromise,
        ]);
      } finally {
        if (timer) clearTimeout(timer);
      }
    } else {
      response = await modelClient.generateContent({
        model: modelName,
        contents,
        config: {
          systemInstruction: {
            parts: [{ text: systemInstruction }],
          },
          tools: [{ functionDeclarations }],
          temperature: 0.2,
        },
      });
    }

    const candidate = response.candidates?.[0]?.content as
      | { parts?: Array<Record<string, unknown>> }
      | undefined;
    const parts = candidate?.parts || [];

    const functionCalls: Array<{
      id?: string;
      name?: string;
      args?: Record<string, unknown>;
    }> = [];

    let textResponse = "";

    for (const part of parts) {
      if (part.functionCall) {
        functionCalls.push(
          part.functionCall as {
            id?: string;
            name?: string;
            args?: Record<string, unknown>;
          }
        );
      }
      if (typeof part.text === "string") {
        textResponse += part.text;
      }
    }

    // Якщо модель не викликає жодного інструменту — отримано фінальну текстову відповідь
    if (functionCalls.length === 0) {
      const rawText = textResponse.trim() || response.text?.trim() || "";
      const finalText = stripToolExecutionNotice(rawText);

      // Зберігаємо діалог у пам'ять разом зі списком реально виконаних інструментів для прозорого дебагу
      if (userPrompt.trim() && finalText) {
        try {
          await invoke("append_conversation_turn", {
            user_text: userPrompt.trim(),
            model_text: finalText,
            tools_used: executedToolsDetails,
            userText: userPrompt.trim(),
            modelText: finalText,
            toolsUsed: executedToolsDetails,
          });
        } catch {
          // Помилка запису пам'яті не блокує видачу відповіді
        }
      }

      return {
        text: finalText,
        executedTools: executedToolsLog,
      };
    }

    // Додаємо виклики моделі в історію ходу
    contents.push({
      role: "model",
      parts: candidate?.parts || functionCalls.map((fc) => ({ functionCall: fc })),
    });

    const responseParts: Array<Record<string, unknown>> = [];

    for (const call of functionCalls) {
      if (!call.name) {
        responseParts.push({
          functionResponse: {
            id: call.id,
            name: "unknown_tool",
            response: {
              ok: false,
              code: "unknown_tool",
              result: "Помилка: модель не вказала назву інструменту.",
            },
          },
        });
        continue;
      }

      const toolSpec = toolRegistry.get(call.name);
      if (!toolSpec) {
        responseParts.push({
          functionResponse: {
            id: call.id,
            name: call.name,
            response: {
              ok: false,
              code: "unknown_tool",
              result: `Невідомий інструмент: ${call.name}`,
            },
          },
        });
        continue;
      }

      const rawArgs = call.args || {};
      const validation = toolSpec.validateArgs(rawArgs);
      if (!validation.ok) {
        responseParts.push({
          functionResponse: {
            id: call.id,
            name: toolSpec.name,
            response: {
              ok: false,
              code: "validation_error",
              result: validation.error,
            },
          },
        });
        continue;
      }

      // Перевірка політики безпеки
      const policyDecision = evaluatePolicy(toolSpec.name, validation.args, ctx);

      if (policyDecision.decision === "deny") {
        responseParts.push({
          functionResponse: {
            id: call.id,
            name: toolSpec.name,
            response: {
              ok: false,
              code: "permission_denied",
              result: policyDecision.reason,
            },
          },
        });
        continue;
      }

      if (policyDecision.decision === "confirm") {
        if (!ctx.confirm) {
          responseParts.push({
            functionResponse: {
              id: call.id,
              name: toolSpec.name,
              response: {
                ok: false,
                code: "permission_denied",
                result:
                  "Потрібне підтвердження користувача, але інтерфейс підтвердження недоступний.",
              },
            },
          });
          continue;
        }

        const approved = await ctx.confirm({
          toolName: toolSpec.name,
          title: policyDecision.title,
          details: policyDecision.details,
          risk: policyDecision.risk,
        });

        if (!approved) {
          responseParts.push({
            functionResponse: {
              id: call.id,
              name: toolSpec.name,
              response: {
                ok: false,
                code: "user_cancelled",
                result: "Дію скасовано користувачем.",
              },
            },
          });
          continue;
        }
      }

      // Виконання інструменту
      callbacks.onStateChange("WORKING", toolSpec.name as WorkingToolName);

      const timeoutMs = toolSpec.timeoutMs ?? 15000;
      let outcome: ToolOutcome;

      try {
        let timer: ReturnType<typeof setTimeout> | undefined;
        const timeoutPromise = new Promise<ToolOutcome>((resolve) => {
          timer = setTimeout(() => {
            resolve({
              ok: false,
              code: "timeout",
              summary: `Таймаут виконання ${toolSpec.name} (${timeoutMs} мс).`,
            });
          }, timeoutMs);
        });

        try {
          outcome = await Promise.race([
            toolSpec.execute(validation.args, ctx),
            timeoutPromise,
          ]);
        } finally {
          if (timer) clearTimeout(timer);
        }
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        outcome = {
          ok: false,
          code: "execution_error",
          summary: `Помилка виконання ${toolSpec.name}: ${msg}`,
        };
      }

      executedToolsLog.push(toolSpec.name);
      const actionParam =
        validation.args &&
        typeof validation.args === "object" &&
        "action" in validation.args
          ? ` (${String((validation.args as Record<string, unknown>).action)})`
          : "";
      executedToolsDetails.push(`${toolSpec.name}${actionParam}`);
      lastToolSummary = outcome.summary;

      console.log(`🛠️ [Tool Exec] ${toolSpec.name}${actionParam}:`, validation.args);
      console.log(`📌 [Tool Outcome] ${toolSpec.name} (ok=${outcome.ok}):`, outcome.summary);

      if (outcome.untrustedContent) {
        ctx.tainted = true;
      }

      responseParts.push({
        functionResponse: {
          id: call.id,
          name: toolSpec.name,
          response: {
            ok: outcome.ok,
            code: outcome.code ?? (outcome.ok ? "ok" : "execution_error"),
            result: outcome.summary,
            ...(outcome.data ? { data: outcome.data } : {}),
          },
        },
      });
    }

    // Передаємо результати інструментів назад у модель для генерації персонажної відповіді
    contents.push({
      role: "user",
      parts: responseParts,
    });

    callbacks.onStateChange("THINKING", undefined);
  }

  return {
    text: "Дію виконано, але досягнуто ліміт кроків оркестратора.",
    executedTools: executedToolsLog,
  };
}

