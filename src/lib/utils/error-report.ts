/** Keep useful error fields and causes, not arbitrary objects that may contain secrets. */
export function errorText(error: unknown): string {
  const seen = new Set<unknown>();
  function describe(value: unknown, depth: number): string {
    if (value == null) return '';
    if (typeof value === 'string') return value.trim();
    if (typeof value !== 'object') return String(value);
    if (seen.has(value) || depth >= 8) return '';
    seen.add(value);
    const record = value as Record<string, unknown>;
    const lines: string[] = [];
    if (typeof record.message === 'string') lines.push(record.message.trim());
    for (const field of ['code', 'status', 'requestId']) {
      if (typeof record[field] === 'string' || typeof record[field] === 'number') {
        lines.push(`${field}: ${record[field]}`);
      }
    }
    const cause = describe(record.cause, depth + 1);
    if (cause) lines.push(`Caused by: ${cause}`);
    return lines.filter(Boolean).join('\n');
  }
  return describe(error, 0) || 'The operation failed without additional error details.';
}

export function errorReport(error: unknown, context?: string) {
  const text = errorText(error);
  const firstLine = text.split(/\r?\n/)[0];
  const summary = firstLine.length > 200 ? `${firstLine.slice(0, 197)}…` : firstLine;
  return {
    summary: context ? `${context}: ${summary}` : summary,
    details: text !== summary ? text : '',
    copyText: context ? `${context}\n${text}` : text,
  };
}
