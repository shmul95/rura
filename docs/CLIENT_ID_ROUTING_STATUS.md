# Client-Provided ID Routing - Implementation Status

## Goal
Enable the server to route messages using the client's 256-bit random user_id (from `identity.enc`) instead of database-assigned user_ids, with routing kept only in memory (no database persistence).

## Completed Changes ✅

### Client Side:
1. **Added `get_account_id()` function** - Returns the 256-bit user_id from identity bundle
2. **Modified auth flow** - Client now loads identity and sends `identity_key` in `AuthRequest`
3. **Temporary debug print** - Prints generated ID on registration

### Server Side:
1. **AppState refactored** - Changed from `HashMap<i64, ClientHandle>` to `HashMap<String, ClientHandle>`
2. **Auth handlers updated** - All return `Option<String>` instead of `Option<i64>`
3. **Hybrid auth support** - Accepts both client-provided `identity_key` and falls back to DB auth

## Remaining Work ⚠️

### Critical Fixes Needed:

1. **Message Protocol Models** (`crates/models/src/messaging.rs`):
   ```rust
   // Change from:
   pub struct DirectMessageReq {
       pub to_user_id: i64,  // ← Change to String
       ...
   }
   
   pub struct DirectMessageEvent {
       pub from_user_id: i64,  // ← Change to String
       ...
   }
   ```

2. **Client Loop Task** (`crates/server/src/client/loop_task.rs`):
   - Fix String move/copy issues (use `.clone()` or `.as_ref()`)
   - Change `authenticated_user_id` handling from `Copy` i64 to String refs

3. **Message Handlers** (`crates/server/src/messaging/handlers.rs`):
   - Update `state.get_sender()` calls to use `&str` instead of `i64`

4. **Authenticated Client Handler** (`crates/server/src/client/authed.rs`):
   - Update all functions expecting `i64` to accept `String`

## Build Status
❌ Currently broken - type mismatches between `i64` and `String` throughout codebase

## Next Steps (In Order):

1. Change all `user_id` fields in `messaging.rs` from `i64` to `String`
2. Update `client/authed.rs` to use `String` for user identification  
3. Fix String cloning issues in `loop_task.rs` (add `.clone()` where needed)
4. Update client Flutter code to send/receive String user_ids instead of i64
5. Test full flow: register → get ID → send message with String ID

## Alternative Simpler Approach

If full String conversion is too complex, consider:
- Hash the 256-bit ID to an i64 for internal routing
- Keep message protocol as i64
- Map base64 identity_key → hashed i64 on server
- Less invasive, maintains backward compatibility

## Files Modified So Far:
- ✅ `crates/server/src/messaging/state.rs`
- ✅ `crates/server/src/auth/handlers.rs`
- ✅ `crates/client/src/api.rs`
- ⚠️  `crates/server/src/client/dispatch.rs` (partial)
- ⚠️  `crates/server/src/client/unauth.rs` (partial)
- ⚠️  `crates/server/src/client/loop_task.rs` (partial)
- ❌ `crates/models/src/messaging.rs` (not started)
- ❌ `crates/server/src/client/authed.rs` (not started)
- ❌ `crates/server/src/messaging/handlers.rs` (not started)

## Testing Checklist (Once Fixed):
- [ ] Register new account
- [ ] Verify identity_key sent to server
- [ ] Server registers client with String ID in memory
- [ ] Send message using String ID
- [ ] Verify routing works
- [ ] Check multiple clients can connect with unique IDs
- [ ] Verify no database writes for routing
