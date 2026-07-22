export type RelayErrorKind =
  | 'InvalidKind'
  | 'TimestampTooFar'
  | 'MissingTag'
  | 'InvalidId'
  | 'InvalidSignature'
  | 'MalformedHex'
  | 'SerializationError'
  | 'LimitExceeded'
  | 'CryptoError'
  | 'Base64Error'
  | 'Utf8Error'
  | 'Generic';

export class RelayError extends Error {
  constructor(
    public readonly kind: RelayErrorKind,
    message: string,
  ) {
    super(message);
    this.name = 'RelayError';
  }

  static InvalidKind(): RelayError {
    return new RelayError('InvalidKind', 'blocked: event kind not allowed');
  }

  static TimestampTooFar(msg: string): RelayError {
    return new RelayError('TimestampTooFar', `invalid: ${msg}`);
  }

  static MissingTag(tag: string): RelayError {
    return new RelayError('MissingTag', `invalid: missing ${tag}`);
  }

  static InvalidId(): RelayError {
    return new RelayError('InvalidId', 'invalid: event ID mismatch');
  }

  static InvalidSignature(): RelayError {
    return new RelayError('InvalidSignature', 'invalid: signature verification failed');
  }

  static MalformedHex(context: string): RelayError {
    return new RelayError('MalformedHex', `invalid: malformed ${context}`);
  }

  static SerializationError(msg: string): RelayError {
    return new RelayError('SerializationError', `error: serialization failed: ${msg}`);
  }

  static LimitExceeded(msg: string): RelayError {
    return new RelayError('LimitExceeded', `rejected: ${msg}`);
  }

  static CryptoError(msg: string): RelayError {
    return new RelayError('CryptoError', `error: crypto failure: ${msg}`);
  }

  static Base64Error(msg: string): RelayError {
    return new RelayError('Base64Error', `error: base64 failure: ${msg}`);
  }

  static Utf8Error(msg: string): RelayError {
    return new RelayError('Utf8Error', `error: utf8 failure: ${msg}`);
  }

  static Generic(msg: string): RelayError {
    return new RelayError('Generic', `error: ${msg}`);
  }
}

export type Result<T> = T | RelayError;
