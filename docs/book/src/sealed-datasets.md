# Held-out dataset seals

`sharpebench-attest` can encrypt frozen dataset bytes before publication. This
protects the ciphertext under a secret key; it does not establish when the data
was committed, who held the key, or what an entrant previously observed.

## V2 format and API

The `scheme` is `sharpebench.sealed.aes-256-gcm-siv.v2`. Each object contains a
hex 96-bit nonce, ciphertext followed by a 16-byte authentication tag, and the
public commitment (`content_hash`, `canary`, `len`). AES-256-GCM-SIV authenticates
the commitment metadata as well as the plaintext. Associated data uses the fixed
scheme domain, little-endian u64-length-prefixed UTF-8 hash and canary fields,
then the plaintext length as a little-endian u64. It does not depend on JSON
whitespace or property ordering.

`seal_dataset(plaintext, key, canary)` returns `Result<SealedDataset, SealError>`.
Supply a cryptographically random 32-byte key from operator-controlled secret
storage, not a password. Native calls obtain a fresh nonce from OS entropy and
refuse if that fails. There is no deterministic fallback. The wasm32 build has
no implicit entropy backend and returns `EntropyUnavailable`; callers with
secure host entropy can use `seal_dataset_with_nonce` and supply a fresh random
12-byte nonce on every call.

`open_dataset(sealed, key)` returns `Some(plaintext)` only after authentication
and the plaintext hash/length check. Invalid keys, changed metadata, malformed
nonces, truncated ciphertext, unknown schemes and legacy formats return `None`.
The application must also compare the commitment to its independently trusted
earlier copy. Authenticating an embedded commitment is not evidence that it was
published earlier.

## Migration from unversioned V1

V1 reused the same HMAC-derived keystream for every message under a key. A known
plaintext could therefore disclose corresponding bytes of other datasets sealed
under that key. Its canary field was also not authenticated.

1. Stop creating or accepting V1 seals. V2 decoding may recognize their shape,
   but opening explicitly refuses them.
2. Recover the original bytes in a trusted private environment, preferably from
   the retained source dataset. If legacy software is necessary, do not expose
   its unauthenticated decoder as a service. Verify the result against a trusted
   earlier commitment, not just metadata bundled with a supplied ciphertext.
3. Create a fresh random key and a new V2 seal. Keep prior published commitments
   and evidence immutable and record the replacement's lineage separately.
4. Assess prior exposure. Re-encryption cannot make already disclosed data secret
   again; retire affected evaluation windows where secrecy is no longer defensible.

## Guarantees and limits

The implementation uses RustCrypto's `aes-gcm-siv`, following
[RFC 8452](https://www.rfc-editor.org/rfc/rfc8452.html). Nonce misuse resistance
avoids V1's two-time-pad failure, but is not permission to reuse nonces. The RFC's
per-key message and size limits still apply. The whole RustCrypto crate and this
integration are not independently audited; the upstream
[security warning](https://docs.rs/aes-gcm-siv/0.12.1/aes_gcm_siv/#security-warning)
also specifies constant-time platform assumptions. Unit regressions and RFC
known-answer vectors are verification evidence, not a security proof.

The public plaintext hash exposes equality and allows offline guesses when data
is predictable; the length is public too. This is not a hiding commitment. The
canary is public metadata and is not automatically inserted in the plaintext.
An agent repeating that public marker alone does not establish training leakage.
Encrypting data does not prevent the operator or a key-holder from disclosing it.
