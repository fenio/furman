export type SseKeyFormat = 'base64' | 'hex' | 'raw';

/** Convert explicit key formats to S3's base64 representation; never log keys. */
export function normalizeSseCustomerKey(value: string, format: SseKeyFormat = 'base64'): string {
  let bytes: Uint8Array;
  try {
    if (format === 'hex') {
      const hex = value.trim();
      if (!/^[0-9a-fA-F]{64}$/.test(hex)) throw new Error();
      bytes = Uint8Array.from(hex.match(/../g)!, pair => parseInt(pair, 16));
    } else if (format === 'raw') {
      bytes = new TextEncoder().encode(value);
    } else {
      bytes = Uint8Array.from(atob(value.trim()), character => character.charCodeAt(0));
    }
    if (bytes.length !== 32) throw new Error();
  } catch {
    throw new Error('SSE-C requires the original 32-byte AES-256 key: base64, 64 hex digits, or 32 raw UTF-8 bytes. This is not your S3 access secret.');
  }
  return btoa(String.fromCharCode(...bytes));
}
