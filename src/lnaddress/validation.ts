export function isValidUsername(s: string): boolean {
  // eslint-disable-next-line @typescript-eslint/no-misused-spread
  return s.length > 0 && [...s].every(c =>
    (c >= 'a' && c <= 'z') ||
    (c >= 'A' && c <= 'Z') ||
    (c >= '0' && c <= '9') ||
    c === '_' || c === '-' || c === '.' || c === '~',
  );
}
