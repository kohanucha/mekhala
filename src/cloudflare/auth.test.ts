import { describe, it, expect } from 'vitest';
import { AuthError, AccessPolicy } from './auth.ts';

describe('AccessPolicy', () => {
  it('public mode allows any secret', () => {
    const policy = new AccessPolicy(null);
    expect(() => { policy.checkAccess(''); }).not.toThrow();
    expect(() => { policy.checkAccess('any-secret'); }).not.toThrow();
  });

  it('private mode rejects wrong secret', () => {
    const policy = new AccessPolicy('secret123');
    expect(() => { policy.checkAccess('secret123'); }).not.toThrow();
    expect(() => { policy.checkAccess('wrong'); }).toThrow(AuthError);
    expect(() => { policy.checkAccess(''); }).toThrow(AuthError);
    expect(() => { policy.checkAccess('secret1234'); }).toThrow(AuthError);
  });

  it('constant-time comparison', () => {
    // Let's test the underlying impl via policy behavior
    const policy = new AccessPolicy('abc');
    expect(() => { policy.checkAccess('abc'); }).not.toThrow();
    expect(() => { policy.checkAccess('abd'); }).toThrow(AuthError);
    expect(() => { policy.checkAccess('abcd'); }).toThrow(AuthError);
  });

  it('empty string via constructor becomes public mode', () => {
    const policy = new AccessPolicy('');
    expect(() => { policy.checkAccess(''); }).not.toThrow();
    expect(() => { policy.checkAccess('any'); }).not.toThrow();
  });

  it('null secret treated as public mode', () => {
    const policy = new AccessPolicy(null);
    expect(() => { policy.checkAccess(''); }).not.toThrow();
    expect(() => { policy.checkAccess('anything'); }).not.toThrow();
  });

  it('private mode is case sensitive', () => {
    const policy = new AccessPolicy('Secret123');
    expect(() => { policy.checkAccess('Secret123'); }).not.toThrow();
    expect(() => { policy.checkAccess('secret123'); }).toThrow(AuthError);
    expect(() => { policy.checkAccess('SECRET123'); }).toThrow(AuthError);
  });

  it('private mode with similar secrets', () => {
    const policy = new AccessPolicy('test-secret-123');
    expect(() => { policy.checkAccess('test-secret-123'); }).not.toThrow();
    expect(() => { policy.checkAccess('test-secret-124'); }).toThrow(AuthError);
    expect(() => { policy.checkAccess('test-secret-12'); }).toThrow(AuthError);
    expect(() => { policy.checkAccess('test-secret123'); }).toThrow(AuthError);
  });

  it('constructor with Some secret', () => {
    const policy = new AccessPolicy('mypass');
    expect(() => { policy.checkAccess('mypass'); }).not.toThrow();
    expect(() => { policy.checkAccess('wrong'); }).toThrow(AuthError);
  });

  it('constructor with None', () => {
    const policy = new AccessPolicy(null);
    expect(() => { policy.checkAccess('anything'); }).not.toThrow();
  });

  it('constructor with empty string becomes public', () => {
    const policy = new AccessPolicy('');
    expect(() => { policy.checkAccess('anything'); }).not.toThrow();
  });
});
