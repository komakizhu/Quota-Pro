import type { ProviderSnapshot, UsageWindow } from "../types";

export const QUOTA_HISTORY_STORAGE_KEY = "quota-pro:quota-history:v1";
const MAX_HISTORY_POINTS = 8;
const DAY_MS = 24 * 60 * 60 * 1000;

function browserStorage(): Storage | undefined {
  try {
    return typeof window === "undefined" ? undefined : window.localStorage;
  } catch {
    return undefined;
  }
}

export interface QuotaHistoryPoint {
  day: string;
  remainingPercent: number;
}

export type QuotaHistory = Record<string, QuotaHistoryPoint[]>;

export interface QuotaPrediction {
  historyDays: number;
  averageDailyUsagePercent: number | null;
  daysAtAverage: number | null;
  daysUntilReset: number | null;
  recommendedDailyPercent: number | null;
}

export function quotaHistoryKey(snapshot: Pick<ProviderSnapshot, "provider">, window: Pick<UsageWindow, "windowSeconds">): string {
  return `${snapshot.provider}:${window.windowSeconds}`;
}

function clampPercent(value: number): number {
  return Math.min(100, Math.max(0, value));
}

function localDay(value: Date): string {
  const year = value.getFullYear();
  const month = String(value.getMonth() + 1).padStart(2, "0");
  const day = String(value.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function dayIndex(day: string): number | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(day);
  if (!match) return null;
  const value = Date.UTC(Number(match[1]), Number(match[2]) - 1, Number(match[3]));
  return Number.isFinite(value) ? Math.floor(value / DAY_MS) : null;
}

function normalizePoints(value: unknown): QuotaHistoryPoint[] {
  if (!Array.isArray(value)) return [];
  return value
    .filter((point): point is { day?: unknown; remainingPercent?: unknown } => Boolean(point) && typeof point === "object")
    .map((point) => ({
      day: typeof point.day === "string" ? point.day : "",
      remainingPercent: typeof point.remainingPercent === "number" ? clampPercent(point.remainingPercent) : Number.NaN,
    }))
    .filter((point) => dayIndex(point.day) !== null && Number.isFinite(point.remainingPercent))
    .sort((left, right) => left.day.localeCompare(right.day))
    .slice(-MAX_HISTORY_POINTS);
}

/** Read only the small, local quota history used for the forecast. */
export function loadQuotaHistory(storage: Pick<Storage, "getItem"> | undefined = browserStorage()): QuotaHistory {
  if (!storage) return {};
  try {
    const raw = storage.getItem(QUOTA_HISTORY_STORAGE_KEY);
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    return Object.fromEntries(Object.entries(parsed).map(([key, value]) => [key, normalizePoints(value)]).filter(([, value]) => value.length > 0));
  } catch {
    return {};
  }
}

export function saveQuotaHistory(history: QuotaHistory, storage: Pick<Storage, "setItem"> | undefined = browserStorage()): void {
  if (!storage) return;
  try {
    storage.setItem(QUOTA_HISTORY_STORAGE_KEY, JSON.stringify(history));
  } catch {
    // Storage can be unavailable in private WebViews. Forecasting remains
    // useful for the current session, so persistence failure is non-fatal.
  }
}

export function recordQuotaSample(history: QuotaHistory, key: string, remainingPercent: number, at = new Date()): QuotaHistory {
  const day = localDay(at);
  const points = [...(history[key] ?? [])];
  const nextPoint = { day, remainingPercent: clampPercent(remainingPercent) };
  const existing = points.findIndex((point) => point.day === day);
  if (existing >= 0) points[existing] = nextPoint;
  else points.push(nextPoint);
  points.sort((left, right) => left.day.localeCompare(right.day));
  return { ...history, [key]: points.slice(-MAX_HISTORY_POINTS) };
}

function recentPoints(points: QuotaHistoryPoint[], now: Date): QuotaHistoryPoint[] {
  const today = Math.floor(Date.UTC(now.getFullYear(), now.getMonth(), now.getDate()) / DAY_MS);
  return normalizePoints(points).filter((point) => {
    const index = dayIndex(point.day);
    return index !== null && index <= today && today - index <= 7;
  });
}

export function calculateQuotaPrediction(
  currentPercent: number,
  resetAt: string | null,
  points: QuotaHistoryPoint[] = [],
  now = new Date(),
): QuotaPrediction {
  const current = clampPercent(currentPercent);
  const currentDay = localDay(now);
  const samples = recentPoints(points, now);
  const currentIndex = samples.findIndex((point) => point.day === currentDay);
  if (currentIndex >= 0) samples[currentIndex] = { day: currentDay, remainingPercent: current };
  else samples.push({ day: currentDay, remainingPercent: current });
  samples.sort((left, right) => left.day.localeCompare(right.day));

  let usage = 0;
  let coveredDays = 0;
  for (let index = 1; index < samples.length; index += 1) {
    const previous = samples[index - 1];
    const next = samples[index];
    const previousDay = dayIndex(previous.day);
    const nextDay = dayIndex(next.day);
    if (previousDay === null || nextDay === null) continue;
    const elapsedDays = nextDay - previousDay;
    const consumed = previous.remainingPercent - next.remainingPercent;
    // A rise usually means the provider rolled the quota window over. Do
    // not treat that reset as negative usage or let it skew the average.
    if (elapsedDays <= 0 || consumed < 0) continue;
    usage += consumed;
    coveredDays += elapsedDays;
  }

  const averageDailyUsagePercent = coveredDays > 0 ? usage / coveredDays : null;
  const daysAtAverage = averageDailyUsagePercent && averageDailyUsagePercent > 0
    ? current / averageDailyUsagePercent
    : null;
  const resetTime = resetAt ? new Date(resetAt).getTime() : Number.NaN;
  const resetDelta = resetTime - now.getTime();
  const daysUntilReset = Number.isFinite(resetDelta) && resetDelta > 0 ? resetDelta / DAY_MS : null;
  const recommendedDailyPercent = daysUntilReset ? current / daysUntilReset : null;

  return {
    historyDays: samples.length,
    averageDailyUsagePercent,
    daysAtAverage,
    daysUntilReset,
    recommendedDailyPercent,
  };
}
