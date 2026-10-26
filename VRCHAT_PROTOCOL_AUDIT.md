# Nightshift VRChat communication parity audit

Audit date: 2026-10-26

Reference implementations and behavior compared:

- VRChat community API reference and pipeline documentation (`api.vrchat.cloud/api/1`, `pipeline.vrchat.cloud`)
- current OyasumiVR `develop` VRChat API/socket implementation
- Nightshift `prototype/hardware-vrchat-auth-state`

The VRChat HTTP/pipeline API is unofficial and can change without notice. Nightshift should treat OyasumiVR as a useful proven client, not as the protocol specification itself.

## Executive summary

Nightshift now matches the important transport requirements that caused the hardware failures found during this branch:

- persisted `auth` / `twoFactorAuth` cookie sessions
- URI-component encoded Basic login fields
- stable application User-Agent on REST requests
- stable application User-Agent on the websocket handshake
- `wss://pipeline.vrchat.cloud/?authToken=...` with encoded auth token
- double-decoding pipeline `content` when VRChat stringifies JSON
- minimal status-only sleep writes (`{"status":"ask me"}` etc.)
- live `user-update` capture so manual client status changes beat lagging REST snapshots
- secret redaction in pipeline failures
- Oyasumi-style global API request budget (15 requests/minute maximum start rate)

Remaining hardening work is listed below. The highest-value remaining items are session/cookie refresh persistence and websocket failure-driven session revalidation.

## Protocol matrix

| Surface | VRChat / Oyasumi behavior | Nightshift status | Notes |
| --- | --- | --- | --- |
| REST base URL | `https://api.vrchat.cloud/api/1` | ALIGNED | Same base URL. |
| REST User-Agent | Proper app User-Agent | ALIGNED | `Nightshift/<version> (https://github.com/Chineko/Nightshift)`. |
| Origin header | Oyasumi explicitly removes `Origin`; unknown Origin can cause 403 | ALIGNED | `reqwest` does not add a browser Origin by default. Do not add one. |
| Auth cookie | Persist/reuse `auth` | ALIGNED | Stored in keyring session; avoids creating a new login session on each launch. |
| 2FA cookie | Persist/reuse `twoFactorAuth` | ALIGNED | Correct VRChat cookie name/casing. |
| Basic login encoding | URL-encode username and password separately before Basic pair encoding | ALIGNED | Nightshift implements encodeURIComponent-compatible encoding. |
| TOTP / email OTP | `/auth/twofactorauth/{totp,emailotp}/verify` | ALIGNED | Both supported. |
| Current user | `GET /auth/user` with session cookies | ALIGNED | Nightshift validates saved session and account ID. |
| Status write | `PUT /users/{userId}` | ALIGNED | Correct endpoint and user-ID validation. |
| Status wire strings | `active`, `ask me`, `busy`, `join me`, `offline` | ALIGNED | Exact mappings tested. |
| Minimal status body | Oyasumi sends only fields that changed | PARTIAL | Sleep status is status-only. General wake/restore still uses status + statusDescription together. |
| Status source of truth | Oyasumi patches successful write locally and consumes websocket `user-update` | HARDENED | Nightshift now consumes live `user-update`, but some paths still perform immediate REST read-back that can lag. |
| Global API budget | Oyasumi total queue limit 15/min | ALIGNED | Added shared 4-second global request-start gate; stricter per-class gates remain. |
| Status write budget | Oyasumi 6/min | ALIGNED | Nightshift status writes have a 10-second minimum gap (6/min). |
| Pipeline URL | `wss://pipeline.vrchat.cloud/?authToken=<encoded>` | ALIGNED | Same endpoint and encoded token. |
| Pipeline User-Agent | Required by current community docs | ALIGNED | Added explicit handshake User-Agent. |
| Pipeline content | Usually stringified JSON inside outer JSON | ALIGNED | Handles string or object content. |
| Pipeline errors | `{"err":"..."}` then server closes | ALIGNED | Recognized and safely logged with token redaction. |
| `user-update` | Live current-user changes | ALIGNED | Captured and preferred over stale REST snapshot for sleep-status decisions. |
| Notifications | Pipeline notification events | ALIGNED | Parsed and routed to invite automation. |
| Reconnect interval | Oyasumi retries every 10 seconds | ALIGNED | Nightshift retries every 10 seconds with 15-second connect timeout. |
| Repeated websocket failures | Oyasumi revalidates REST session after repeated short failures | REMAINING | Nightshift currently keeps retrying the same pipeline token until the normal runtime refresh notices a session problem. |
| Socket close diagnostics | Oyasumi records close code/reason/health streak | PARTIAL | Nightshift logs handshake/read errors, but does not yet retain close-code/lifetime/failure-streak diagnostics. |
| Cookie refresh after login | Oyasumi captures Set-Cookie on later `/auth/user` calls and persists it | REMAINING | Nightshift captures cookies on login/2FA only. Defensive persistence of later cookie rotation should be added. |
| 401 handling | Treat as expired/logged-out session | ALIGNED | Typed session-expired handling exists. |
| 429 handling | Rate limit and retry/back off where appropriate | PARTIAL | Typed 429 handling + conservative request gates exist; generic retry-after/backoff is not centralized yet. |
| Secrets in logs | Never expose auth cookies/tokens | ALIGNED | Cookie Debug output and pipeline errors redact secrets. |

## Findings fixed during this audit

### Global request budget

Nightshift previously had independent request-class gates. Session reads, relationship reads, status writes, and invite writes could therefore collectively start requests faster than OyasumiVR's proven total limit of 15 per minute.

Nightshift now has a shared four-second global gate in addition to its existing per-class limits. The request permit holds both the class and global gates while the request is in flight, preventing parallel classes from stampeding the API.

### Live state versus REST lag

Hardware testing demonstrated that `/auth/user` could still report `AskMe` after the VRChat client had manually changed to `Busy`. Treating that REST snapshot as authoritative caused Nightshift to incorrectly skip the sleep write.

Nightshift now captures pipeline `user-update` presence and prefers it for the current-account sleep decision. This follows Oyasumi's event-driven model and avoids using a lagging REST snapshot as the only live-state authority.

### Websocket handshake identity

The current VRChat pipeline documentation requires a proper User-Agent. Nightshift originally used tungstenite's URL-only `connect_async` path without explicitly supplying the Nightshift User-Agent. The handshake now builds an explicit request with the same stable app identity used for REST calls.

## Remaining remediation

### P1: Revalidate the session after repeated short websocket failures

Oyasumi tracks websocket health. After repeated early closes it calls session revalidation instead of reconnecting the same token forever.

Nightshift should signal the connection state machine after a small failure streak (for example three connections that fail before 30 seconds). The worker should run its normal validated `/auth/user` flow immediately. If the session is invalid, transition to disconnected; if valid, rebuild the pipeline using the freshly loaded persisted session.

Do not log the auth token or pipeline URL while doing this.

### P1: Stop using immediate `/auth/user` read-back as authoritative write verification

`PUT /users/{userId}` returns user data, and Oyasumi patches the fields it successfully sent. Nightshift still has paths that perform a second `/auth/user` request immediately after a successful write. Hardware testing has already shown that endpoint can lag the live client.

Prefer the successful PUT response for immediate local write state, then allow pipeline `user-update` to reconcile live state. This also removes an unnecessary API request from every presence mutation.

### P1: Make wake/restore writes field-minimal

Oyasumi sends only fields that differ. Nightshift's sleep path now does this for status-only changes, but generic restore uses both `status` and `statusDescription`.

Restore should compare current and target presence immediately before the write:

- status differs, description same: send status only
- status same, description differs: send description only
- both differ: send both
- neither differs: no request

This minimizes unexpected status-message normalization and matches the proven client behavior.

### P2: Persist defensive cookie refreshes

Oyasumi examines later successful authentication/current-user responses for `Set-Cookie` and persists any refreshed `auth` or `twoFactorAuth` value. Nightshift currently stores cookies obtained during login/2FA and then treats them as immutable.

VRChat's public community docs describe the auth cookie as being issued on login, so rotation is not documented as a guaranteed routine behavior. Still, adopting Oyasumi's defensive refresh handling reduces the chance of a long-running stored profile retaining a stale cookie if server behavior changes.

Any cookie refresh implementation must keep the account-ID mismatch guard and must update the keyring atomically before switching the pipeline token.

### P2: Add websocket health diagnostics

Record safe metadata only:

- close code
- whether a close was clean
- connection lifetime bucket
- number of consecutive early failures
- whether an `err` frame was received

Never log the auth token, full websocket URL, cookies, Authorization header, or response bodies that may contain authToken fields.

### P2: Centralize 429 retry policy

Nightshift now avoids most rate-limit pressure via global/per-class gates, but a server-side 429 should still honor a bounded retry policy where the operation is safe to retry. Prefer `Retry-After` when present and cap retry attempts. Do not blindly retry login or 2FA flows.

## Hardware validation checklist

For the next VR test:

1. Start `scripts/nightshift-dev.fish --perf`.
2. Confirm one `VRChat pipeline connected` message and no reconnect spam.
3. Manually change VRChat status while awake; confirm Nightshift receives a `user-update` and reflects the new state.
4. Enter sleep with a different configured target; confirm a status mutation is actually sent and VRChat visibly changes.
5. Wake; confirm the captured pre-sleep presence is restored visibly.
6. Exercise an invite/request so the pipeline + REST invite path are both tested.
7. Leave the stack running long enough to cross at least one 60-second session refresh.
8. Stop with Ctrl+C and retain the perf report alongside the runtime log.

If communication fails, capture the first safe error line and the surrounding state transition rather than increasing log verbosity around credentials.
