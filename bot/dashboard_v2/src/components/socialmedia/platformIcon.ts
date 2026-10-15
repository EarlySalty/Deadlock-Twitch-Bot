export function platformIconUrl(platform: string): string {
  return `${import.meta.env?.BASE_URL ?? '/'}brand/platforms/${platform}.svg`;
}
