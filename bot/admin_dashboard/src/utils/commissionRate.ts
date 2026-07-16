export function parseCommissionRate(value: string): number | null {
  if (!value.trim()) {
    return null;
  }

  const rate = Number(value);
  return Number.isInteger(rate) && rate >= 0 && rate <= 100 ? rate : null;
}
