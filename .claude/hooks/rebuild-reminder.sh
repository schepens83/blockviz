#!/usr/bin/env bash
# Stop hook: if the native app's sources are newer than its release binary,
# tell Claude to rebuild before finishing, so the app the user launches is current.
input=$(cat)
[ "$(jq -r '.stop_hook_active // false' <<<"$input")" = "true" ] && exit 0
app="$CLAUDE_PROJECT_DIR/crates/blockviz-app"
bin="$app/target/release/blockviz-app"
if [ ! -e "$bin" ] || [ -n "$(find "$app/src" "$app/Cargo.toml" "$app/assets" -newer "$bin" -type f -print -quit 2>/dev/null)" ]; then
  jq -n '{decision: "block", reason: "The native app sources are newer than the release binary. Run `cargo build --release` in crates/blockviz-app so the app the user launches includes the change, then tell them to restart it."}'
fi
exit 0
