# Branch cleanup checklist

This branch exists only as a temporary cleanup marker.

Canonical development branch:

- `prototype/integrated-mvp`

## Safe to delete

These branches are fully contained in `prototype/integrated-mvp`, identical to it, or are explicitly known to be superseded/dead work.

- `prototype/avatar-lock-refresh`
- `prototype/avatar-picker-validation`
- `prototype/hydra-continuity-note`
- `prototype/openxr-dimmer`
- `prototype/refactor-split-massive-files`
- `prototype/taffy-basement`
- `prototype/taffy-final-composition`
- `prototype/taffy-home-primary`
- `prototype/taffy-home-status-cards` — accidental duplicate slice, superseded by final Taffy composition
- `prototype/taffy-nav`
- `prototype/taffy-settings-hr`
- `prototype/taffy-social-tabs`
- `prototype/ui-cosmic-text`
- `prototype/ui-effects-layer`
- `prototype/ui-layout-taffy`
- `prototype/ui-widget-ids`
- `prototype/vr-social-avatars`
- `prototype/vrchat-request-coordinator`

## Review before deleting

These branches have commits that are not ancestors of the current canonical branch. They may still be obsolete, but should be inspected before their refs are removed.

- `prototype/avatar-groundwork-validation` — 3 unique commits
- `prototype/refactor-clippy-diagnostics` — 1 unique commit
- `prototype/refactor-daemon-generator` — 4 unique commits
- `prototype/refactor-ui-generator` — 5 unique commits
- `prototype/refactor-vrchat-generator` — 5 unique commits
- `prototype/taffy-social-remove` — 2 unique commits
- `prototype/ui-panel-layout` — 5 unique commits

## Keep

- `prototype/integrated-mvp` — canonical
- `delete-me` — temporary cleanup checklist; delete this branch last

Once the stale refs are removed, delete `delete-me` too.