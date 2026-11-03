// A software passkey for the tests: a P-256 key per credential, "none" attestation, and the
// WebAuthn responses a browser would send for the options the Worker hands out.

const enc = new TextEncoder();

export function b64url(bytes: Uint8Array): string {
  let s = '';
  for (const b of bytes) s += String.fromCharCode(b);
  return btoa(s).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

export function unb64url(s: string): Uint8Array {
  const raw = atob(s.replace(/-/g, '+').replace(/_/g, '/'));
  return Uint8Array.from(raw, (c) => c.charCodeAt(0));
}

function cat(...parts: Uint8Array[]): Uint8Array {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.length;
  }
  return out;
}

// Just enough CBOR for an attestation object and a COSE key.
type Cbor = number | string | Uint8Array | Map<Cbor, Cbor>;
function head(major: number, n: number): Uint8Array {
  if (n < 24) return Uint8Array.of((major << 5) | n);
  if (n < 256) return Uint8Array.of((major << 5) | 24, n);
  return Uint8Array.of((major << 5) | 25, n >> 8, n & 255);
}
function cbor(v: Cbor): Uint8Array {
  if (typeof v === 'number') return v >= 0 ? head(0, v) : head(1, -1 - v);
  if (typeof v === 'string') {
    const b = enc.encode(v);
    return cat(head(3, b.length), b);
  }
  if (v instanceof Uint8Array) return cat(head(2, v.length), v);
  const parts = [head(5, v.size)];
  for (const [k, x] of v) parts.push(cbor(k), cbor(x));
  return cat(...parts);
}

async function sha256(b: Uint8Array): Promise<Uint8Array> {
  return new Uint8Array(await crypto.subtle.digest('SHA-256', b));
}

/** WebCrypto's raw r||s signature as the DER sequence WebAuthn sends. */
function der(raw: Uint8Array): Uint8Array {
  const int = (x: Uint8Array) => {
    let i = 0;
    while (i < x.length - 1 && x[i] === 0) i++;
    let v = x.subarray(i);
    if (v[0] & 0x80) v = cat(Uint8Array.of(0), v);
    return cat(Uint8Array.of(2, v.length), v);
  };
  const body = cat(int(raw.subarray(0, 32)), int(raw.subarray(32)));
  return cat(Uint8Array.of(0x30, body.length), body);
}

interface Credential {
  id: Uint8Array;
  keys: CryptoKeyPair;
  user: string;
  count: number;
}

export class Authenticator {
  creds: Credential[] = [];
  constructor(readonly origin: string) {}

  private clientData(type: string, challenge: string): Uint8Array {
    return enc.encode(JSON.stringify({ type, challenge, origin: this.origin, crossOrigin: false }));
  }

  /** The browser's answer to navigator.credentials.create with these options. */
  async create(options: { challenge: string; rp: { id: string }; user: { id: string } }) {
    const keys = (await crypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, true, ['sign', 'verify'])) as CryptoKeyPair;
    const jwk = (await crypto.subtle.exportKey('jwk', keys.publicKey)) as JsonWebKey;
    const cose = cbor(
      new Map<Cbor, Cbor>([
        [1, 2],
        [3, -7],
        [-1, 1],
        [-2, unb64url(jwk.x!)],
        [-3, unb64url(jwk.y!)],
      ]),
    );
    const id = crypto.getRandomValues(new Uint8Array(16));
    const authData = cat(
      await sha256(enc.encode(options.rp.id)),
      Uint8Array.of(0x45, 0, 0, 0, 0),
      new Uint8Array(16),
      Uint8Array.of(0, id.length),
      id,
      cose,
    );
    const attestation = cbor(
      new Map<Cbor, Cbor>([
        ['fmt', 'none'],
        ['attStmt', new Map()],
        ['authData', authData],
      ]),
    );
    this.creds.push({ id, keys, user: options.user.id, count: 0 });
    return {
      id: b64url(id),
      rawId: b64url(id),
      type: 'public-key',
      authenticatorAttachment: 'platform',
      clientExtensionResults: {},
      response: {
        clientDataJSON: b64url(this.clientData('webauthn.create', options.challenge)),
        attestationObject: b64url(attestation),
        transports: ['internal'],
      },
    };
  }

  /** The browser's answer to navigator.credentials.get, with the first (or given) credential. */
  async get(options: { challenge: string; rpId?: string }, cred = this.creds[0]) {
    cred.count++;
    const authData = cat(
      await sha256(enc.encode(options.rpId ?? new URL(this.origin).hostname)),
      Uint8Array.of(0x05, cred.count >>> 24, (cred.count >>> 16) & 255, (cred.count >>> 8) & 255, cred.count & 255),
    );
    const clientData = this.clientData('webauthn.get', options.challenge);
    const signed = cat(authData, await sha256(clientData));
    const raw = new Uint8Array(await crypto.subtle.sign({ name: 'ECDSA', hash: 'SHA-256' }, cred.keys.privateKey, signed));
    return {
      id: b64url(cred.id),
      rawId: b64url(cred.id),
      type: 'public-key',
      authenticatorAttachment: 'platform',
      clientExtensionResults: {},
      response: {
        clientDataJSON: b64url(clientData),
        authenticatorData: b64url(authData),
        signature: b64url(der(raw)),
        userHandle: cred.user,
      },
    };
  }
}
