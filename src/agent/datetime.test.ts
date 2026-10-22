import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  resolveRelativeDate,
  resolveReminderTime,
  formatLocalDate,
  formatLocalDateTime,
} from "./datetime.ts";

describe("datetime utils", () => {
  // Фіксована дата: Середа, 2026-10-21 14:00:00 (місяць 9 в JS = жовтень)
  const baseWednesday = new Date(2026, 9, 21, 14, 0, 0);

  it("1. returns today's local date", () => {
    assert.equal(resolveRelativeDate("сьогодні", baseWednesday), "2026-10-21");
    assert.equal(resolveRelativeDate("today", baseWednesday), "2026-10-21");
  });

  it("2. resolves tomorrow", () => {
    assert.equal(resolveRelativeDate("завтра", baseWednesday), "2026-10-22");
    assert.equal(resolveRelativeDate("tomorrow", baseWednesday), "2026-10-22");
  });

  it("3. resolves day after tomorrow", () => {
    assert.equal(resolveRelativeDate("післязавтра", baseWednesday), "2026-10-23");
    assert.equal(resolveRelativeDate("day_after_tomorrow", baseWednesday), "2026-10-23");
  });

  it("4. preserves already formatted YYYY-MM-DD", () => {
    assert.equal(resolveRelativeDate("2026-12-31", baseWednesday), "2026-12-31");
  });

  it("5. parses DD.MM.YYYY format", () => {
    assert.equal(resolveRelativeDate("25.10.2026", baseWednesday), "2026-10-25");
  });

  it("6. resolves in N days", () => {
    assert.equal(resolveRelativeDate("через 5 днів", baseWednesday), "2026-10-26");
    assert.equal(resolveRelativeDate("in 3 days", baseWednesday), "2026-10-24");
  });

  it("7. resolves day of week in future on same week (Четвер)", () => {
    // 2026-10-21 це середа, четвер це наступний день 2026-10-22
    assert.equal(resolveRelativeDate("четвер", baseWednesday), "2026-10-22");
    assert.equal(resolveRelativeDate("у четвер", baseWednesday), "2026-10-22");
    assert.equal(resolveRelativeDate("thursday", baseWednesday), "2026-10-22");
  });

  it("8. resolves day of week in accusative case (в п'ятницю, у суботу)", () => {
    // П'ятниця: 2026-10-23
    assert.equal(resolveRelativeDate("в п'ятницю", baseWednesday), "2026-10-23");
    assert.equal(resolveRelativeDate("у пятницю", baseWednesday), "2026-10-23");
    // Субота: 2026-10-24
    assert.equal(resolveRelativeDate("в суботу", baseWednesday), "2026-10-24");
    // Неділя: 2026-10-25
    assert.equal(resolveRelativeDate("у неділю", baseWednesday), "2026-10-25");
  });

  it("9. resolves past day of week to next week (Понеділок)", () => {
    // Сьогодні середа. "Понеділок" уже минув, отже це наступний понеділок: 2026-10-26 (+5 днів)
    assert.equal(resolveRelativeDate("понеділок", baseWednesday), "2026-10-26");
    assert.equal(resolveRelativeDate("в понеділок", baseWednesday), "2026-10-26");
  });

  it("10. resolves same day of week to next week (Середа)", () => {
    // Сьогодні середа. "У середу" означає наступну середу: 2026-10-28 (+7 днів)
    assert.equal(resolveRelativeDate("середа", baseWednesday), "2026-10-28");
    assert.equal(resolveRelativeDate("в середу", baseWednesday), "2026-10-28");
  });

  it("11. handles month and year boundary correctly", () => {
    // 31 грудня 2026 року
    const endOfYear = new Date(2026, 11, 31, 23, 30, 0);
    assert.equal(resolveRelativeDate("завтра", endOfYear), "2027-01-01");
    assert.equal(resolveRelativeDate("післязавтра", endOfYear), "2027-01-02");
  });

  it("12. resolves time-only reminder (later today vs already passed)", () => {
    // Базовий час 14:00
    // Час 18:00 ще не настав сьогодні -> сьогоднішній день
    const resLater = resolveReminderTime({ remindAt: "18:00" }, baseWednesday);
    assert.equal(resLater.isoLocal, "2026-10-21T18:00:00");

    // Час 10:00 вже настав сьогодні -> переноситься на завтра
    const resEarlier = resolveReminderTime({ remindAt: "10:00" }, baseWednesday);
    assert.equal(resEarlier.isoLocal, "2026-10-22T10:00:00");
  });

  it("13. resolves duration_minutes into exact ISO timestamp", () => {
    const res = resolveReminderTime({ durationMinutes: 15 }, baseWednesday);
    assert.equal(res.isoLocal, "2026-10-21T14:15:00");
    assert.equal(res.durationMinutes, 15);
  });

  it("14. resolves combo date + time (завтра 09:30, п'ятниця 18:00)", () => {
    const resTomorrow = resolveReminderTime({ remindAt: "завтра 09:30" }, baseWednesday);
    assert.equal(resTomorrow.isoLocal, "2026-10-22T09:30:00");

    const resFriday = resolveReminderTime({ remindAt: "в п'ятницю 18:00" }, baseWednesday);
    assert.equal(resFriday.isoLocal, "2026-10-23T18:00:00");
  });

  it("15. formats local dates and datetimes with zero-padding", () => {
    const date = new Date(2026, 0, 5, 4, 7, 9);
    assert.equal(formatLocalDate(date), "2026-01-05");
    assert.equal(formatLocalDateTime(date), "2026-01-05T04:07:09");
  });
});

