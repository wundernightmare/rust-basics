# secrets

Small cryptographic helpers, modelled on tracehub-edge's `tracehub-secrets`
(generic, no project coupling). Pure-Rust RustCrypto — no OpenSSL.

| API                          | Purpose                                                       |
| ---------------------------- | ------------------------------------------------------------- |
| `SealingKey` + `seal`/`open` | AES-256-GCM authenticated encryption; random 96-bit nonce prepended to the ciphertext |
| `SecretString` / `SecretBox` | re-exported from `secrecy`; render as `[REDACTED]` in `Debug` |
| `SecretCache`                | lock-free (`arc-swap`) hot-swappable cache of decrypted secrets |
| `constant_time_eq`           | length-checked constant-time byte comparison (token/HMAC checks) |

## Usage

```rust
use secrets::{open, seal, SealingKey};

let key = SealingKey::from_bytes([7u8; 32]);          // or ::from_base64 / ::from_file
let sealed = seal(&key, b"hunter2");                  // nonce(12) || ciphertext
assert_eq!(open(&key, &sealed).unwrap(), b"hunter2");
assert!(open(&SealingKey::from_bytes([0u8; 32]), &sealed).is_err()); // wrong key
```

```rust
use secrets::{ExposeSecret, SecretString};

let token = SecretString::from("s3cr3t".to_owned());
assert!(!format!("{token:?}").contains("s3cr3t"));    // redacted in logs
assert_eq!(token.expose_secret(), "s3cr3t");          // explicit opt-in to read
```

Used by [`ping`](../ping): the `/secure` route holds its key as a `SecretString`
and compares the Bearer token with `constant_time_eq`.

## Develop

```sh
just test
just lint
```
