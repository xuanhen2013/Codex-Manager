export type ApiKeyUsageRangePreset =
  | "last_7_days"
  | "last_30_days"
  | "this_month"
  | "last_month"
  | "custom";

export interface ApiKeyUsageDateRange {
  startInput: string;
  endInput: string;
  startTs: number;
  endTs: number;
  dayBoundariesTs: number[];
}

function localDate(year: number, month: number, day: number): Date {
  return new Date(year, month, day, 0, 0, 0, 0);
}

export function formatLocalDateInput(date: Date): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function parseDateParts(value: string): [number, number, number] | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value.trim());
  if (!match) return null;
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  const date = localDate(year, month - 1, day);
  if (
    date.getFullYear() !== year ||
    date.getMonth() !== month - 1 ||
    date.getDate() !== day
  ) {
    return null;
  }
  return [year, month - 1, day];
}

export function parseLocalDateStartTs(value: string): number | null {
  const parts = parseDateParts(value);
  if (!parts) return null;
  return Math.floor(localDate(...parts).getTime() / 1000);
}

export function parseLocalDateEndExclusiveTs(value: string): number | null {
  const parts = parseDateParts(value);
  if (!parts) return null;
  const [year, month, day] = parts;
  return Math.floor(localDate(year, month, day + 1).getTime() / 1000);
}

export function buildApiKeyUsageDateRange(
  startInput: string,
  endInput: string,
): ApiKeyUsageDateRange | null {
  const startParts = parseDateParts(startInput);
  const endParts = parseDateParts(endInput);
  if (!startParts || !endParts) return null;
  const startDate = localDate(...startParts);
  const [endYear, endMonth, endDay] = endParts;
  const endDate = localDate(endYear, endMonth, endDay + 1);
  if (endDate <= startDate) return null;

  const dayBoundariesTs: number[] = [];
  let cursor = startDate;
  while (cursor < endDate) {
    dayBoundariesTs.push(Math.floor(cursor.getTime() / 1000));
    cursor = localDate(
      cursor.getFullYear(),
      cursor.getMonth(),
      cursor.getDate() + 1,
    );
  }
  dayBoundariesTs.push(Math.floor(endDate.getTime() / 1000));

  return {
    startInput,
    endInput,
    startTs: dayBoundariesTs[0],
    endTs: dayBoundariesTs.at(-1)!,
    dayBoundariesTs,
  };
}

export function createApiKeyUsagePresetRange(
  preset: Exclude<ApiKeyUsageRangePreset, "custom">,
  now = new Date(),
): ApiKeyUsageDateRange {
  const today = localDate(now.getFullYear(), now.getMonth(), now.getDate());
  let startDate = today;
  let endDate = today;

  if (preset === "last_7_days" || preset === "last_30_days") {
    const days = preset === "last_7_days" ? 7 : 30;
    startDate = localDate(
      today.getFullYear(),
      today.getMonth(),
      today.getDate() - (days - 1),
    );
  } else if (preset === "this_month") {
    startDate = localDate(today.getFullYear(), today.getMonth(), 1);
  } else {
    startDate = localDate(today.getFullYear(), today.getMonth() - 1, 1);
    endDate = localDate(today.getFullYear(), today.getMonth(), 0);
  }

  const range = buildApiKeyUsageDateRange(
    formatLocalDateInput(startDate),
    formatLocalDateInput(endDate),
  );
  if (!range) throw new Error("failed to build api key usage date range");
  return range;
}
