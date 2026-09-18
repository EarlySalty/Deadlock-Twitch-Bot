export interface PublicProfile { login: string; headline: string }
export function parsePublicProfiles(value: unknown): PublicProfile[] {
  if (!value || typeof value !== "object" || !("profiles" in value) || !Array.isArray(value.profiles)) return [];
  return value.profiles.filter((entry): entry is PublicProfile => Boolean(entry && typeof entry === "object" && typeof entry.login === "string" && /^[a-z0-9_]{1,25}$/.test(entry.login) && typeof entry.headline === "string"));
}
