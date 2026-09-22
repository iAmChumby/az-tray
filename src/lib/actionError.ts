export function actionErrorMessage(cause: unknown, fallback: string): string {
  const message = cause instanceof Error ? cause.message : typeof cause === "string" ? cause : "";
  return message.trim() || fallback;
}
