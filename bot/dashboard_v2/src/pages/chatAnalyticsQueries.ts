export function shouldFetchChatHypeTimeline(
  streamer: string | null | undefined,
  sessionId: number | null | undefined
): boolean {
  return Boolean(streamer && sessionId);
}
