# Single Account Refactor

## Overview

Reworked the client storage architecture to support only **one account per installation**, replacing the previous multi-user `.cache/users/<user_id>/` structure with a single `data/` directory.

## Changes Made

### 1. Directory Structure

**Before:**
```
.cache/
├── users/
│   ├── 1/
│   │   ├── rura_config.json
│   │   ├── identity.enc
│   │   └── persistent.enc
│   ├── 2/
│   │   ├── rura_config.json
│   │   ├── identity.enc
│   │   └── persistent.enc
│   └── ...
└── current_user.json
```

**After:**
```
data/
├── rura_config.json    # Salt for password derivation
├── identity.enc        # Ed25519 keypair + 256-bit user_id
└── persistent.enc      # Encrypted SQLite database
```

### 2. Environment Variable

- **Old:** `RURA_CLIENT_CACHE_DIR`
- **New:** `RURA_CLIENT_DATA_DIR`

Default: `../data` (relative to current working directory)

### 3. API Changes

#### `local_storage.rs`

**Removed:**
- `init_for_user(user_id: i64)` → `init_storage()`
- `set_current_user(user_id: i64)` → *(removed, no longer needed)*
- `get_current_user()` → *(removed, no longer needed)*
- `append_ephemeral_message(user_id, ...)` → `append_ephemeral_message(...)`
- `append_persistent_message(user_id, ...)` → `append_persistent_message(...)`
- `load_history(user_id, ...)` → `load_history(...)`
- `wipe_ephemeral(user_id)` → `wipe_ephemeral()`
- `snapshot_persistent(user_id)` → `snapshot_persistent()`

**Added:**
- `data_dir_exists() -> bool` - Check if account exists

**Internal Changes:**
- Replaced `HashMap<i64, LocalStorage>` with `Option<LocalStorage>`
- Removed `user_id` field from `LocalStorage` struct

#### `security.rs`

**Removed:**
- `unlock_local(user_id: i64, password)` → `unlock_local(password)`
- `encrypt_blob(user_id: i64, data)` → `encrypt_blob(data)`
- `decrypt_blob(user_id: i64, data)` → `decrypt_blob(data)`
- `generate_and_store_identity(user_id)` → `generate_and_store_identity()`
- `load_identity(user_id)` → `load_identity()`

**Added:**
- `data_dir_exists() -> bool` - Check if account exists

**Internal Changes:**
- Replaced `HashMap<i64, [u8; 32]>` with `Option<[u8; 32]>`
- All functions now use single data directory paths

#### `api.rs`

**Updated Functions:**
- `append_local_message()` - Removed `user_id` parameter
- `load_local_history()` - Removed `user_id` parameter
- `login_and_load_local_history_tls()` - Checks `data_dir_exists()`, requires account to exist
- `register_and_load_local_history_tls()` - Checks `!data_dir_exists()`, requires no existing account

### 4. Registration & Login Logic

#### **Register**
- **Before:** Always creates new `users/<user_id>/` directory
- **After:** 
  - Checks if `data/` exists
  - **If exists:** Returns error "Account already exists. Please login instead."
  - **If not exists:** Creates account in `data/`

#### **Login**
- **Before:** Loads from `users/<user_id>/` based on current_user.json
- **After:**
  - Checks if `data/` exists
  - **If not exists:** Returns error "No account found. Please register first."
  - **If exists:** Unlocks account from `data/`

### 5. Identity Bundle

Updated `IdentityBundle` to include a **256-bit random user_id**:

```rust
pub struct IdentityBundle {
    pub scheme: String,        // "ed25519-v1"
    pub public_b64: String,    // Ed25519 public key
    pub pkcs8_b64: String,     // Ed25519 keypair
    pub user_id: String,       // 256-bit random identifier (base64)
}
```

This unique identifier can be used instead of server-assigned user_ids (relates to idea.txt point #10).

## Migration Notes

### For Existing Installations

**If you have existing `.cache/users/<N>/` data:**

1. Choose which account to keep (if multiple exist)
2. Move the chosen account's files:
   ```bash
   mkdir -p data
   mv .cache/users/1/rura_config.json data/
   mv .cache/users/1/identity.enc data/
   mv .cache/users/1/persistent.enc data/
   ```
3. Delete old structure:
   ```bash
   rm -rf .cache
   ```

### For New Installations

No migration needed. First run will create `data/` directory on registration.

## Testing

All tests pass:
```bash
cd crates/client && cargo test
```

- ✅ 4 unit tests passing
- ✅ Build succeeds with only deprecation warnings (base64 API)
- ✅ Single account enforcement working

## Benefits

1. **Simplified Architecture** - No user_id tracking, no current_user.json
2. **Clear Single Account Model** - Matches the idea of one account per device
3. **Easier to Distribute** - Package just includes `data/` directory
4. **Prevents Multi-Account Confusion** - Register/login errors guide users
5. **Unique Identity** - 256-bit random user_id replaces server-assigned IDs

## Related

See `docs/idea.txt` for the original design concept that motivated this change.
