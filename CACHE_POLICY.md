# Nightshift cache policy

Nightshift may keep stale **presentation data** usable when doing so improves continuity, but stale cached data must never remain authoritative for policy, authorization, identity, or automation decisions.

## Stale-but-usable rule

When previously valid presentation data exists and a refresh is pending or temporarily fails:

- keep showing the last known good display name, avatar, profile image, or other non-authoritative presentation metadata;
- record freshness/generation metadata so callers can tell whether the value is fresh, refreshing, or stale;
- replace cached presentation data atomically only after a complete valid refresh is ready;
- fall back to initials/placeholders only when there is no usable previously valid presentation data;
- prefer bounded background refresh and bounded retry over clearing useful data on transient failures.

## Never stale for decisions

The following state is authoritative and must not use stale presentation-cache membership or values to make decisions:

- whitelist and blacklist membership;
- invite rules and automation policy;
- authenticated VRChat session/account identity;
- sleep/wake automation state;
- permissions, allow/deny decisions, and destructive actions;
- any state the user has explicitly edited or removed locally.

A cached friend/profile entry may still exist after a user is removed from a whitelist, but that cached entry is only presentation metadata. It must not cause the user to remain whitelisted, appear as whitelisted, or receive whitelist behavior.

## Source-of-truth separation

Keep policy state and presentation caches separate by design. Whitelist/blacklist entries store stable VRChat user IDs and policy state; friend/profile caches map those IDs to display metadata. Rendering may join the two at read time, but cache contents must never imply policy membership.

User edits invalidate or update authoritative policy state immediately. Presentation caches do not override those edits.

## Session and generation safety

Cached data is scoped to the authenticated session/account where appropriate. Async work started under an older session or generation must not write into newer state. Stale-but-usable applies only within a still-valid identity boundary; data from a previous account must not leak into the active account.

## Cache housekeeping and bounds

Stale-but-usable is not permission to retain data indefinitely. Every cache must have an explicit cleanup policy appropriate to its cost and purpose.

- set a hard upper bound on cache entries and/or total retained bytes where practical;
- track last-used and last-refreshed timestamps for entries that can accumulate over time;
- periodically prune entries that have exceeded their retention window and are not currently needed;
- prefer LRU-style eviction when a size bound is reached so recently useful presentation data survives while abandoned entries leave first;
- immediately discard session-scoped cache entries when the authenticated account/session generation changes;
- failed refreshes may extend usability of an existing entry only within the configured retention ceiling; repeated failures must not make an entry immortal;
- image/thumbnail caches should use a shorter or separately bounded retention policy than lightweight identity metadata when their memory/disk cost is materially higher;
- housekeeping must be bounded work itself: prune incrementally or at coarse intervals rather than scanning large caches on every read;
- expose cache size/age information to diagnostics where useful so unbounded growth is detectable before it becomes a problem.

A cache implementation is incomplete if it has a way to add entries but no credible way to age out, evict, or invalidate entries.

## Design test

When adding a cache, ask four questions:

1. Is this value only for presentation/continuity, or can it change what Nightshift is allowed to do?
2. If the user changes the authoritative state now, can any old cached value reverse or mask that change?
3. What is the hard bound or retention rule that prevents this cache from growing forever?
4. What event immediately invalidates this cache across session/account boundaries?

If the answer to the second question is yes, the cache boundary is wrong. If the third or fourth question has no clear answer, the cache lifecycle is incomplete.
