import { describe, expect, it } from "vitest";
import { calculateQuotaPrediction, loadQuotaHistory, recordQuotaSample, quotaHistoryKey, saveQuotaHistory, type QuotaHistory } from "./quotaPrediction";

describe("quota prediction", () => {
  const now = new Date("2026-08-17T12:00:00Z");
  const snapshot = { provider: "codex" as const };
  const window = { windowSeconds: 604800 };

  it("keeps one local sample per day and trims old samples", () => {
    let history: QuotaHistory = {};
    const key = quotaHistoryKey(snapshot, window);
    for (let day = 1; day <= 10; day += 1) {
      history = recordQuotaSample(history, key, 100 - day, new Date(`2026-08-${String(day).padStart(2, "0")}T12:00:00Z`));
    }
    expect(history[key]).toHaveLength(8);
    expect(history[key][0].day).toBe("2026-08-03");
    history = recordQuotaSample(history, key, 12, new Date("2026-08-10T08:00:00Z"));
    expect(history[key]).toHaveLength(8);
    expect(history[key].find((point) => point.day === "2026-08-10")?.remainingPercent).toBe(12);
  });

  it("calculates average daily use, runway, and daily budget", () => {
    const points = [
      { day: "2026-08-14", remainingPercent: 80 },
      { day: "2026-08-15", remainingPercent: 70 },
      { day: "2026-08-16", remainingPercent: 60 },
    ];
    const prediction = calculateQuotaPrediction(50, "2026-08-20T12:00:00Z", points, now);
    expect(prediction.historyDays).toBe(4);
    expect(prediction.averageDailyUsagePercent).toBe(10);
    expect(prediction.daysAtAverage).toBe(5);
    expect(prediction.daysUntilReset).toBe(3);
    expect(prediction.recommendedDailyPercent).toBeCloseTo(16.6667, 4);
  });

  it("ignores a reset increase when averaging", () => {
    const points = [
      { day: "2026-08-14", remainingPercent: 40 },
      { day: "2026-08-15", remainingPercent: 20 },
      { day: "2026-08-16", remainingPercent: 95 },
    ];
    const prediction = calculateQuotaPrediction(85, "2026-08-20T12:00:00Z", points, now);
    expect(prediction.averageDailyUsagePercent).toBe(15);
    expect(prediction.daysAtAverage).toBeCloseTo(5.6667, 4);
  });

  it("round-trips only validated local data", () => {
    const values = new Map<string, string>();
    const storage = {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => values.set(key, value),
    };
    const history = { "codex:604800": [{ day: "2026-08-17", remainingPercent: 42 }] };
    saveQuotaHistory(history, storage);
    expect(loadQuotaHistory(storage)).toEqual(history);
    values.set("quota-pro:quota-history:v1", JSON.stringify({ bad: [{ day: "nope", remainingPercent: 900 }] }));
    expect(loadQuotaHistory(storage)).toEqual({});
  });
});
