export function parseLocalDate(value: string): Date {
  const date = value.slice(0, 10);
  const [year, month, day] = date.split("-").map(Number);
  return new Date(year, month - 1, day, 12);
}

export function formatLocalDate(
  value: string,
  options: Intl.DateTimeFormatOptions,
  locales?: Intl.LocalesArgument,
): string {
  return parseLocalDate(value).toLocaleDateString(locales, options);
}
