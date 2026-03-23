# P95 Optimization Plan — Phases A through E

## Goal
Bring IAM/list_users P95 from ~5.23ms and SM/list_secrets P95 from ~5.37ms below the 5ms gate.

---

## Phase A: Gateway fast request-ID

**File: `crates/gateway/Cargo.toml`**
- Replace `uuid.workspace = true` with `rand.workspace = true`

**File: `crates/gateway/src/server.rs`**
- Remove `use uuid::Uuid;` (line 26)
- Add imports:
  ```rust
  use std::cell::RefCell;
  use rand::rngs::SmallRng;
  use rand::{RngExt, SeedableRng};
  ```
- Add thread-local fast RNG (near top, after constants):
  ```rust
  thread_local! {
      static FAST_RNG: RefCell<SmallRng> = RefCell::new(SmallRng::from_rng(&mut rand::rng()));
  }
  ```
- Add `fast_request_id()` function:
  ```rust
  fn fast_request_id() -> String {
      FAST_RNG.with(|rng| {
          let mut rng = rng.borrow_mut();
          let mut b = [0u8; 16];
          rng.fill(&mut b);
          b[6] = (b[6] & 0x0f) | 0x40;
          b[8] = (b[8] & 0x3f) | 0x80;
          const HEX: &[u8; 16] = b"0123456789abcdef";
          let mut out = [0u8; 36];
          let mut o = 0;
          for (i, &byte) in b.iter().enumerate() {
              if i == 4 || i == 6 || i == 8 || i == 10 {
                  out[o] = b'-';
                  o += 1;
              }
              out[o] = HEX[(byte >> 4) as usize];
              o += 1;
              out[o] = HEX[(byte & 0x0f) as usize];
              o += 1;
          }
          // SAFETY: HEX table and '-' are all valid ASCII/UTF-8.
          unsafe { String::from_utf8_unchecked(out.to_vec()) }
      })
  }
  ```
- Line 521: Replace `Uuid::new_v4().to_string()` with `fast_request_id()`

**Savings**: ~200-400ns per request (eliminates getrandom syscall)

---

## Phase B: Deduplicate request-ID

The gateway generates a request_id (line 521) and stores it in `RequestContext.request_id`.
IAM's `dispatch` generates a *second* request_id via its own `req_id()` (line 338).

**Problem**: The service-framework `RequestContext` (traits.rs) does NOT have a `request_id` field.

**Solution**: Add `request_id` to the service-framework `RequestContext` and pass it through.

**File: `crates/service-framework/src/traits.rs`**
- Add field to `RequestContext` struct (after `query_params`):
  ```rust
  /// Unique request ID for tracing (generated once by the gateway)
  pub request_id: String,
  ```
- Update `RequestContext::new()` to include `request_id: String::new()`

**File: `crates/gateway/src/context.rs`**
- In `to_service_request_context()`, add: `request_id: self.request_id,`

**File: `crates/services/iam/src/provider.rs`**
- In `dispatch()`, replace `let rid = req_id();` with reading from context:
  ```rust
  let rid = if ctx.request_id.is_empty() { req_id() } else { ctx.request_id.clone() };
  ```
  Actually — simpler: just use a reference. The `req_id()` generates a String that's used as `&rid`. If the gateway provides it, just use `&ctx.request_id`. But `ctx.request_id` may be empty for unit tests.
  
  Better approach: keep `req_id()` as fallback for tests, but use gateway's ID in prod:
  ```rust
  let rid: Cow<'_, str> = if ctx.request_id.is_empty() {
      Cow::Owned(req_id())
  } else {
      Cow::Borrowed(&ctx.request_id)
  };
  ```
  All usages of `&rid` continue to work since `Cow` derefs to `&str`.

**File: `crates/gateway/src/server.rs`**
- Line 887: Change `request_id: request_id.to_string()` to `request_id: request_id.clone()` (it's already a String).
  Actually `request_id` is a `String` (from `fast_request_id()`), and `build_request_context` takes `request_id: &str`. So in `RequestContext` construction it does `.to_string()` which clones. This is necessary since the gateway `RequestContext` owns it.

**Other services**: No changes needed — they don't use `request_id` (it just flows through unused). Only IAM uses it.

**Savings**: ~80-150ns (eliminates second SmallRng UUID generation + format! in IAM)

---

## Phase C: Zero-alloc gateway path

### C1: `query_string` borrow (line 523)
```rust
// Before:
let query_string = uri.query().unwrap_or("").to_string();
// After:
let query_string = uri.query().unwrap_or("");
```
Remove `.to_string()`. Already used as `&str` in `serde_urlencoded::from_str`.
Also need to adjust line 529 from `&query_string` to just `query_string` since it's already `&str`.

### C2: `detect_service` returns `Cow<'_, str>` (lines 893-957)
Change return type from `String` to `Cow<'static, str>`.
Every branch that returns a `&'static str` becomes `Cow::Borrowed(...)`.
The `normalize_service_name` call on line 902: returns `Cow<'static, str>`.
The hostname branch (line 919): `potential_service` is borrowed from the `host` header.
  - Since `host` is borrowed from `headers` and `headers` is consumed by `build_request_context`, 
    this won't work with a borrowed lifetime.
  - Solution: Make hostname branch return `Cow::Owned(potential_service.to_string())` — 
    only fires on hostname-based routing (uncommon), and it's already allocating today.

### C3: `normalize_service_name` returns `Cow<'static, str>` (lines 959-969)
```rust
fn normalize_service_name(service: &str) -> Cow<'static, str> {
    if service.eq_ignore_ascii_case("es") {
        return Cow::Borrowed("opensearch");
    }
    if service.bytes().all(|b| !b.is_ascii_uppercase()) {
        // service comes from SigV4 auth header which is always &str borrowed from
        // the header value. But we need 'static for the Cow.
        // Since service names from sigv4 are always known static strings,
        // use is_known_service to get the static ref.
        if let Some(s) = known_service_static(service) {
            Cow::Borrowed(s)
        } else {
            Cow::Owned(service.to_string())
        }
    } else {
        Cow::Owned(service.to_ascii_lowercase())
    }
}
```

Actually, simpler: since `service_from_target`, `service_from_query_action`, and `service_from_path` all already return `Option<&'static str>`, and the SigV4 service name comes from `normalize_service_name` — the issue is that `normalize_service_name` takes a `&str` from the parsed auth header. The service name in SigV4 is always a known AWS service name (iam, sqs, s3, etc.), so we can match it to a known static string:

```rust
fn normalize_service_name(service: &str) -> Cow<'static, str> {
    if service.eq_ignore_ascii_case("es") {
        return Cow::Borrowed("opensearch");
    }
    let lower = if service.bytes().all(|b| !b.is_ascii_uppercase()) {
        service
    } else {
        // Need to lowercase — rare path
        return Cow::Owned(service.to_ascii_lowercase());
    };
    // Try to resolve to a known static string to avoid allocation
    if is_known_service(lower) {
        // is_known_service checks a static list; return the static &str
        Cow::Borrowed(known_service_str(lower))
    } else {
        Cow::Owned(lower.to_string())
    }
}
```

Need a helper `known_service_str(name: &str) -> &'static str` that matches the input against the known service list and returns the static reference. Since `is_known_service` already exists, we need to see its implementation.

**Alternative simpler approach**: Just change `detect_service` to take the known service functions (which return `&'static str`) and only allocate for the truly unknown/hostname paths. The SigV4 service name path can look up the canonical static name:

```rust
fn detect_service(...) -> Cow<'static, str> {
    if let Some(svc) = service_from_auth {
        return match canonical_service_name(svc) {
            Some(s) => Cow::Borrowed(s),
            None => Cow::Owned(svc.to_ascii_lowercase()),
        };
    }
    // ... rest returns Cow::Borrowed for all &'static str paths
}
```

Where `canonical_service_name` is a match returning `Option<&'static str>` for all known services.

### C4: `SigV4Auth` borrow optimization (sigv4.rs)
Change `SigV4Auth` to borrow from the auth header string:

```rust
pub struct SigV4Auth<'a> {
    pub access_key: &'a str,
    pub region: &'a str,
    pub service: &'a str,
}
```

This requires `parse_sigv4_auth` to return `SigV4Auth<'a>` borrowing from the input `&'a str`.

**Impact on `build_request_context`** (server.rs:816-834):
- `sigv4.access_key` is used to derive `access_key` — currently `String`, consumed by `RequestContext`
- `sigv4.region` becomes the region — consumed by `RequestContext`
- `sigv4.service` becomes `service_from_auth: Option<&str>` — already `Option<&str>` via `.as_deref()`

Since `RequestContext` needs owned `String` for `access_key` and `region`, we still need `.to_string()` at the point of `RequestContext` construction, but we avoid allocating 3 Strings upfront (only 2 are needed — `access_key` and `region` — not `service`).

Wait — currently all 3 are allocated as `String` (lines 820, 42-44), then `service` is used via `.as_deref()` to get `Option<&str>`. If we borrow, we eliminate the `service` allocation entirely (since it's only ever used as `&str`) and we delay the `access_key`/`region` allocations until `RequestContext` construction — same total cost for those two.

Net savings: 1 String allocation (~the service name, typically 3-12 bytes). ~40-60ns.

### C5: `access_key_to_account_id` returns `Cow<'static, str>` (sigv4.rs:50-67)
```rust
pub fn access_key_to_account_id(access_key: &str) -> Cow<'static, str> {
    const DEFAULT_ACCOUNT: &str = "000000000000";
    const TEST_ACCESS_KEYS: &[(&str, &str)] = &[
        ("test", DEFAULT_ACCOUNT),
        ("mock", DEFAULT_ACCOUNT),
        ("AKIAIOSFODNN7EXAMPLE", DEFAULT_ACCOUNT),
    ];
    for (key, account) in TEST_ACCESS_KEYS {
        if access_key.starts_with(key) {
            return Cow::Borrowed(account);
        }
    }
    Cow::Owned(derive_account_id_from_key(access_key))
}
```

The consumer `build_request_context` stores it as `account_id: String` in `RequestContext`. 
When it's `Cow::Borrowed`, calling `.into_owned()` or `.to_string()` would allocate. But we can change `RequestContext.account_id` to `Cow<'static, str>` — that's a bigger change.

Alternative: Since `account_id` in `RequestContext` is `String` and consumed by the service framework, just keep it. The common path (`"test"` access key -> `"000000000000"`) would use `Cow::Borrowed("000000000000")`, and then at `RequestContext` construction we'd call `.into_owned()` which allocates for the Owned case but is... wait, `Cow::Borrowed(...).into_owned()` also allocates. So this doesn't save anything unless we change the downstream to accept `Cow`.

**Decision**: Skip C5 for now — the saving is small (~40ns) and requires changing `RequestContext.account_id` type which cascades through all services.

Actually — wait. We can be smarter. The `build_request_context` takes `account_id` and puts it into both gateway `RequestContext` (String) and then into service-framework `RequestContext` (String). If we make service-framework `account_id` a `Cow<'static, str>`, that's a big cascade. If we just eliminate one of the two `.to_string()` calls, we save one alloc.

**Decision**: Defer C5. Focus on C1-C4.

**Total Phase C savings**: ~120-200ns (query_string borrow + detect_service zero-alloc + one fewer SigV4 alloc)

---

## Phase D: In-place serialization

### D1: IAM ListUsers — serialize inside the DashMap read lock

**File: `crates/services/iam/src/provider.rs`** (lines 415-442)

Current code clones all users out, drops lock, then serializes. Change to serialize directly inside the lock:

```rust
"ListUsers" => {
    match self.store.get(account_id, region) {
        None => {
            return Ok(xml_resp(
                "ListUsers",
                &rid,
                "<Users /><IsTruncated>false</IsTruncated>",
            ));
        }
        Some(store) => {
            // Sort by name using only references (no clone)
            let mut names: Vec<&str> = store.users.keys().map(String::as_str).collect();
            names.sort_unstable();
            Ok(xml_response_write(
                "ListUsers",
                &rid,
                8 + names.len() * 260,
                |buf| {
                    buf.push_str("<Users>");
                    for name in &names {
                        if let Some(u) = store.users.get(*name) {
                            write_user_xml(buf, u);
                        }
                    }
                    buf.push_str("</Users><IsTruncated>false</IsTruncated>");
                },
            ))
            // DashMap read lock released here after serialization
        }
    }
}
```

The DashMap read lock is *shared* — multiple readers can hold it simultaneously. The only contention is with writers (CreateUser/DeleteUser), which are rare relative to ListUsers in benchmarks. The serialization takes ~1-3us for a few users, which is much less than the time to clone 5N strings.

**Savings**: Eliminates N * 5+ String clones + Vec allocation + sort of large structs. For the benchmark (1 seeded user): ~5 String clones saved = ~200-400ns. For larger N the savings grow dramatically.

### D2: SecretsManager ListSecrets — borrow-based serialization

**File: `crates/services/secretsmanager/src/provider.rs`**

Add a borrow-based summary struct:
```rust
#[derive(Serialize)]
struct SecretSummaryRef<'a> {
    #[serde(rename = "ARN")]
    arn: &'a str,
    #[serde(rename = "Name")]
    name: &'a str,
    #[serde(rename = "Description")]
    description: &'a str,
    #[serde(rename = "CreatedDate")]
    created_date: i64,
    #[serde(rename = "LastChangedDate")]
    last_changed_date: i64,
    #[serde(rename = "DeletedDate", skip_serializing_if = "Option::is_none")]
    deleted_date: Option<i64>,
}

#[derive(Serialize)]
struct ListSecretsResponseRef<'a> {
    #[serde(rename = "SecretList")]
    secret_list: Vec<SecretSummaryRef<'a>>,
}
```

Change ListSecrets handler (lines 396-413):
```rust
"ListSecrets" => {
    match self.store.get(account_id, region) {
        None => Ok(json_ok_bytes(Bytes::from_static(b"{\"SecretList\":[]}"))),
        Some(store) => {
            let secret_list: Vec<SecretSummaryRef<'_>> = store
                .secrets
                .values()
                .filter(|s| !s.deleted)
                .map(|s| SecretSummaryRef {
                    arn: &s.arn,
                    name: &s.name,
                    description: &s.description,
                    created_date: s.created.timestamp(),
                    last_changed_date: s.last_changed.timestamp(),
                    deleted_date: s.deletion_date.map(|d| d.timestamp()),
                })
                .collect();
            let resp = ListSecretsResponseRef { secret_list };
            let estimated = 64 + secret_list.len() * 200;
            let mut buf = Vec::with_capacity(estimated);
            serde_json::to_writer(&mut buf, &resp).unwrap();
            Ok(json_ok_bytes(Bytes::from(buf)))
            // DashMap read lock released here
        }
    }
}
```

**Savings**: Eliminates 3N String clones + empty-list short-circuit with `Bytes::from_static`. For benchmark (1 seeded secret): ~3 String clones saved = ~120-180ns plus static empty case.

---

## Phase E: DispatchResponse content_type Cow<'static, str>

**File: `crates/service-framework/src/traits.rs`**
- Add `use std::borrow::Cow;`
- Change `pub content_type: String` to `pub content_type: Cow<'static, str>` (line 205)
- Update `ok_json`: `content_type: Cow::Borrowed("application/json")`
- Update `ok_xml`: `content_type: Cow::Borrowed("text/xml")`
- Update `streaming`: `content_type: content_type.into()` needs signature change to `impl Into<Cow<'static, str>>`
  - Or just keep `impl Into<String>` and convert: hmm, that loses the zero-alloc.
  - Change signature to `content_type: Cow<'static, str>`. The single call site (S3) can do `Cow::Owned(ct)`.

**File: `crates/gateway/src/server.rs`**
- Line 656: `response.content_type` — already returns `Cow<'static, str>`, used as `&content_type` in `.header()` — works fine since `Cow` derefs to `&str`.
- Line 702: `ct.to_string()` — change to `Cow::Borrowed(ct)` since `serialize_error` returns `&'static str`.

**All 24 service provider files**: Change every `content_type: "...".to_string()` to `content_type: Cow::Borrowed("...")`.

The files and approximate line counts:
- `iam/provider.rs` (4 sites)
- `secretsmanager/provider.rs` (3 sites)  
- `s3/provider.rs` (2 sites + 2 empty string + 1 dynamic) — dynamic ones use `Cow::Owned(ct)`
- `sqs/provider.rs` (5 sites)
- `cloudwatch/provider.rs` (3 sites)
- `lambda/provider.rs` (12 sites)
- `kinesis/provider.rs` (2 sites)
- `sns/provider.rs` (3 sites)
- `ssm/provider.rs` (2 sites)
- `opensearch/provider.rs` (4 sites)
- `firehose/provider.rs` (2 sites)
- `eventbridge/provider.rs` (3 sites)
- `ecr/provider.rs` (3 sites)
- `dynamodb/provider.rs` (2 sites)
- `cloudformation/provider.rs` (2 sites)
- `apigateway/provider.rs` (4 sites)
- `acm/provider.rs` (2 sites)
- `sts/provider.rs` (2 sites)
- `stepfunctions/provider.rs` (2 sites)
- `ses/provider.rs` (3 sites)
- `route53/provider.rs` (3 sites)
- `redshift/provider.rs` (2 sites)
- `kms/provider.rs` (2 sites)
- `ec2/provider.rs` (2 sites)
- `aws-protocol/protocol.rs` — `SerializedResponse.content_type: String` → also change to `Cow<'static, str>`

**Savings**: ~40-60ns per response (eliminates one String allocation per request)

---

## Verification

After all phases:
```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace --lib --bins
SKIP_DOCKER_TESTS=1 cargo test --workspace --tests
```

---

## Estimated Total Savings

| Phase | Savings | Confidence |
|-------|---------|------------|
| A: Gateway fast request-ID | 200-400ns | High |
| B: Dedup request-ID | 80-150ns | High |
| C: Zero-alloc gateway | 120-200ns | Medium |
| D: In-place serialization | 300-500ns | High |
| E: content_type Cow | 40-60ns | High |
| **Total** | **740-1310ns** | |

Current P95 overshoot: IAM ~230ns, SM ~370ns. The combined savings of 740-1310ns provide a comfortable 3-5x margin beyond what's needed.
