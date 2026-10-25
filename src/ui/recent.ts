const KEEP = 8;

/** The repositories saved by `addRecent`, whatever else storage holds. */
export function readRecent(raw: string): string[] {
  try {
    const list: unknown = JSON.parse(raw);
    return Array.isArray(list) ? list.filter((r): r is string => typeof r === 'string' && !!r).slice(0, KEEP) : [];
  } catch {
    return [];
  }
}

/** `repo` first, the others after it, at most eight. */
export const addRecent = (list: string[], repo: string): string[] => [repo, ...list.filter(r => r !== repo)].slice(0, KEEP);
