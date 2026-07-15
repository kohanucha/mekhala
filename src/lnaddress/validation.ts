export function isValidUsername(s: string): boolean {
  return s.length > 0 && [...s].every(c =>
    (c >= 'a' && c <= 'z') ||
    (c >= 'A' && c <= 'Z') ||
    (c >= '0' && c <= '9') ||
    c === '_' || c === '-' || c === '.' || c === '~',
  );
}
