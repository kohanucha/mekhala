import { sha256 } from '@noble/hashes/sha2.js';

export class AuthError extends Error {
  static forbidden(): AuthError {
    return new AuthError('Forbidden');
  }
}

export class AccessPolicy {
  private expectedSecret: string | null;

  constructor(expectedSecret: string | null) {
    if (expectedSecret != null && expectedSecret !== '') {
      this.expectedSecret = expectedSecret;
    } else {
      this.expectedSecret = null;
    }
  }

  checkAccess(providedSecret: string): void {
    if (this.expectedSecret == null) return;

    if (!constantTimeEq(providedSecret, this.expectedSecret)) {
      throw AuthError.forbidden();
    }
  }
}

function constantTimeEq(a: string, b: string): boolean {
  const hashA = sha256(new TextEncoder().encode(a));
  const hashB = sha256(new TextEncoder().encode(b));

  let diff = 0;
  for (let i = 0; i < hashA.length; i++) {
    diff |= hashA[i] ^ hashB[i];
  }
  return diff === 0;
}
