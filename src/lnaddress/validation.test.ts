import { describe, it, expect } from 'vitest';
import { isValidUsername } from './validation.ts';

describe('isValidUsername', () => {
  it('accepts alphanumeric', () => {
    expect(isValidUsername('alice')).toBe(true);
  });

  it('accepts underscore', () => {
    expect(isValidUsername('alice_smith')).toBe(true);
  });

  it('accepts hyphen', () => {
    expect(isValidUsername('alice-smith')).toBe(true);
  });

  it('accepts numeric', () => {
    expect(isValidUsername('user123')).toBe(true);
  });

  it('accepts single char', () => {
    expect(isValidUsername('a')).toBe(true);
  });

  it('rejects empty', () => {
    expect(isValidUsername('')).toBe(false);
  });

  it('rejects special chars', () => {
    expect(isValidUsername('alice@smith')).toBe(false);
  });

  it('rejects space', () => {
    expect(isValidUsername('alice smith')).toBe(false);
  });

  it('accepts dot', () => {
    expect(isValidUsername('alice.smith')).toBe(true);
  });

  it('accepts tilde', () => {
    expect(isValidUsername('alice~smith')).toBe(true);
  });

  it('rejects unicode', () => {
    expect(isValidUsername('hëllo')).toBe(false);
  });

  it('rejects emoji', () => {
    expect(isValidUsername('alice😀')).toBe(false);
  });
});
