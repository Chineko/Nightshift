#!/usr/bin/env fish

set -l script_dir (dirname (status --current-filename))
set -l root (realpath "$script_dir/..")
set -l hydra
set -l with_ui 0

for arg in $argv
    switch $arg
        case --hydra-mode --hydra
            set hydra --hydra-mode
        case --ui
            set with_ui 1
        case '*'
            echo "Unknown option: $arg" >&2
            echo "Usage: scripts/nightshift-dev.fish [--hydra] [--ui]" >&2
            exit 2
    end
end

cd $root; or exit 1

echo "Building Nightshift core..."
cargo build -p nightshift-daemon -p nightshift-osc; or exit $status
cargo build --release -p nightshift-overlay; or exit $status

if test $with_ui -eq 1
    cargo build -p nightshift-ui; or exit $status
end

set -g nightshift_children

function nightshift_cleanup --on-signal INT --on-signal TERM
    if test (count $nightshift_children) -gt 0
        echo
        echo "Stopping Nightshift hydra..."
        kill $nightshift_children 2>/dev/null
        wait $nightshift_children 2>/dev/null
        set -g nightshift_children
    end
end

function nightshift_exit_cleanup --on-event fish_exit
    if test (count $nightshift_children) -gt 0
        kill $nightshift_children 2>/dev/null
        wait $nightshift_children 2>/dev/null
    end
end

echo "Starting Nightshift daemon..."
$root/target/debug/nightshift-daemon &
set -a nightshift_children $last_pid

# Give the daemon a moment to bind its Unix socket. Workers also reconnect on
# their own, so this is convenience rather than a correctness requirement.
sleep 0.15

echo "Starting Nightshift OSC worker..."
$root/target/debug/nightshift-osc &
set -a nightshift_children $last_pid

echo "Starting Nightshift OpenXR overlay..."
$root/target/release/nightshift-overlay $hydra &
set -a nightshift_children $last_pid

if test $with_ui -eq 1
    echo "Starting Nightshift desktop UI..."
    $root/target/debug/nightshift-ui &
    set -a nightshift_children $last_pid
end

echo
echo "Nightshift is running. Ctrl+C stops the whole local stack."
echo "WayVR remains a separate process, because one hydra is enough."
echo

# Exit when any child exits unexpectedly; the fish_exit handler tears down the
# remaining processes so we never leave a daemon lurking under the bed.
wait -n $nightshift_children
set -l child_status $status

if test $child_status -ne 130 -a $child_status -ne 143
    echo "A Nightshift process exited (status $child_status); stopping the stack." >&2
end

exit $child_status
