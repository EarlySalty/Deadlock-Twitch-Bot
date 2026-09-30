export function neverWordList(value: string): string[] {
  const seen = new Set<string>();
  return value
    .split('\n')
    .map((line) => [...line.trim().replace(/\s+/gu, ' ')].slice(0, 60).join('').trim())
    .filter((line) => {
      const key = line.toLowerCase();
      if (!line || seen.has(key)) return false;
      seen.add(key);
      return true;
    })
    .slice(0, 40);
}
