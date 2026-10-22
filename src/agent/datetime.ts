/**
 * Детерміновані утиліти для розбору відносних дат та часу у локальному часовому поясі користувача.
 */

function pad2(n: number): string {
  return n < 10 ? `0${n}` : `${n}`;
}

export function formatLocalDate(d: Date): string {
  const year = d.getFullYear();
  const month = pad2(d.getMonth() + 1);
  const day = pad2(d.getDate());
  return `${year}-${month}-${day}`;
}

export function formatLocalDateTime(d: Date): string {
  const datePart = formatLocalDate(d);
  const hours = pad2(d.getHours());
  const minutes = pad2(d.getMinutes());
  const seconds = pad2(d.getSeconds());
  return `${datePart}T${hours}:${minutes}:${seconds}`;
}

const UK_DAYS_OF_WEEK: Record<string, number> = {
  // 1 = Понеділок, ..., 7 = Неділя
  "понеділок": 1,
  "понеділка": 1,
  "вівторок": 2,
  "вівторка": 2,
  "середа": 3,
  "середу": 3,
  "середи": 3,
  "четвер": 4,
  "четверга": 4,
  "п'ятниця": 5,
  "п'ятницю": 5,
  "п'ятниці": 5,
  "пятниця": 5,
  "пятницю": 5,
  "пятниці": 5,
  "субота": 6,
  "суботу": 6,
  "суботи": 6,
  "неділя": 7,
  "неділю": 7,
  "неділі": 7,
};

const EN_DAYS_OF_WEEK: Record<string, number> = {
  "monday": 1,
  "mon": 1,
  "tuesday": 2,
  "tue": 2,
  "wednesday": 3,
  "wed": 3,
  "thursday": 4,
  "thu": 4,
  "friday": 5,
  "fri": 5,
  "saturday": 6,
  "sat": 6,
  "sunday": 7,
  "sun": 7,
};

function getIsoDayOfWeek(d: Date): number {
  // getDay(): 0 = Sun, 1 = Mon, ..., 6 = Sat
  const day = d.getDay();
  return day === 0 ? 7 : day;
}

/**
 * Перетворює текстовий опис дати (відносний або календарний) у формат 'YYYY-MM-DD'.
 */
export function resolveRelativeDate(input: string, now: Date): string {
  const raw = input.trim();
  if (!raw) {
    return formatLocalDate(now);
  }

  // Якщо дата вже у форматі YYYY-MM-DD
  if (/^\d{4}-\d{2}-\d{2}$/.test(raw)) {
    return raw;
  }

  // Формат DD.MM.YYYY
  const dotMatch = /^(\d{1,2})\.(\d{1,2})\.(\d{4})$/.exec(raw);
  if (dotMatch) {
    const [, d, m, y] = dotMatch;
    return `${y}-${pad2(Number(m))}-${pad2(Number(d))}`;
  }

  const clean = raw
    .toLowerCase()
    .replace(/^(в|у|on|next|наступного|наступної|наступний)\s+/i, "")
    .trim();

  if (clean === "today" || clean === "сьогодні") {
    return formatLocalDate(now);
  }

  if (clean === "tomorrow" || clean === "завтра") {
    const next = new Date(now.getTime());
    next.setDate(next.getDate() + 1);
    return formatLocalDate(next);
  }

  if (
    clean === "day_after_tomorrow" ||
    clean === "післязавтра" ||
    clean === "після завтра"
  ) {
    const next = new Date(now.getTime());
    next.setDate(next.getDate() + 2);
    return formatLocalDate(next);
  }

  if (clean === "next_week" || clean === "тижня" || clean === "тиждень") {
    const next = new Date(now.getTime());
    next.setDate(next.getDate() + 7);
    return formatLocalDate(next);
  }

  // через N днів / in N days
  const inDaysMatch =
    /^(?:через|in)\s+(\d+)\s*(?:дн|днів|дня|день|days|day)?$/i.exec(raw.trim());
  if (inDaysMatch) {
    const days = parseInt(inDaysMatch[1], 10);
    const next = new Date(now.getTime());
    next.setDate(next.getDate() + days);
    return formatLocalDate(next);
  }

  // Перевірка днів тижня
  const targetDay = UK_DAYS_OF_WEEK[clean] ?? EN_DAYS_OF_WEEK[clean];
  if (targetDay !== undefined) {
    const currentDay = getIsoDayOfWeek(now);
    let diff = targetDay - currentDay;
    if (diff <= 0) {
      // Якщо день збігається із сьогодні або вже минув на цьому тижні,
      // вказуємо наступний такий день через тиждень (+7)
      diff += 7;
    }
    const next = new Date(now.getTime());
    next.setDate(next.getDate() + diff);
    return formatLocalDate(next);
  }

  return raw;
}

/**
 * Нормалізує час або тривалість нагадування у локальний ISO-рядок 'YYYY-MM-DDTHH:mm:ss'.
 */
export function resolveReminderTime(
  params: { remindAt?: string | null; durationMinutes?: number | null },
  now: Date
): { isoLocal: string | null; durationMinutes?: number } {
  if (params.durationMinutes && params.durationMinutes > 0) {
    const target = new Date(now.getTime() + params.durationMinutes * 60_000);
    return {
      isoLocal: formatLocalDateTime(target),
      durationMinutes: params.durationMinutes,
    };
  }

  const raw = params.remindAt?.trim();
  if (!raw) {
    return { isoLocal: null };
  }

  // Повний ISO формат з датою і часом
  if (/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(?::\d{2})?$/.test(raw)) {
    const normalized = raw.length === 16 ? `${raw}:00` : raw;
    return { isoLocal: normalized };
  }

  // Тільки час: HH:mm або HH:mm:ss
  const timeOnlyMatch = /^(\d{1,2})[:;](\d{2})(?:[:;](\d{2}))?$/.exec(raw);
  if (timeOnlyMatch) {
    const hours = parseInt(timeOnlyMatch[1], 10);
    const minutes = parseInt(timeOnlyMatch[2], 10);
    const seconds = timeOnlyMatch[3] ? parseInt(timeOnlyMatch[3], 10) : 0;

    const candidate = new Date(now.getTime());
    candidate.setHours(hours, minutes, seconds, 0);

    // Якщо цей час сьогодні вже минув — ставимо на завтра
    if (candidate.getTime() <= now.getTime()) {
      candidate.setDate(candidate.getDate() + 1);
    }
    return { isoLocal: formatLocalDateTime(candidate) };
  }

  // Комбінація відносної дати та часу (наприклад: "завтра 09:00", "середа 18:30", "tomorrow 15:00")
  const comboMatch = /^(.+?)\s+(\d{1,2}[:;]\d{2}(?:[:;]\d{2})?)$/.exec(raw);
  if (comboMatch) {
    const datePart = resolveRelativeDate(comboMatch[1], now);
    const timeRaw = comboMatch[2].replace(";", ":");
    const [h, m, s] = timeRaw.split(":");
    const timeNormalized = `${pad2(Number(h))}:${pad2(Number(m))}:${pad2(Number(s || 0))}`;
    return { isoLocal: `${datePart}T${timeNormalized}` };
  }

  return { isoLocal: raw };
}

