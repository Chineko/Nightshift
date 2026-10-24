#!/usr/bin/env fish

set -l script_dir (dirname (status --current-filename))
set -l root (realpath "$script_dir/..")
set -l hydra
set -l with_ui 0
set -l with_hr 0
set -l with_vrchat 0

function nightshift_usage
    echo "Usage: scripts/nightshift-dev.fish [--hydra] [--ui] [--hr] [--vrchat] [--full]"
    echo
    echo "  --hydra    Enable overlay Hydra diagnostics"
    echo "  --ui       Start the desktop UI"
    echo "  --hr       Start the BLE heart-rate worker"
    echo "  --vrchat   Start the VRChat runtime worker"
    echo "  --full     Start UI, heart-rate, and VRChat workers"
end

for arg in $argv
    switch $arg
        case --hydra-mode --hydra
            set hydra --hydra-mode
        case --ui
            set with_ui 1
        case --hr
            set with_hr 1
        case --vrchat
            set with_vrchat 1
        case --full --all
            set with_ui 1
            set with_hr 1
            set with_vrchat 1
        case --help -h
            nightshift_usage
            exit 0
        case '*'
            echo "Unknown option: $arg" >&2
            nightshift_usage >&2
            exit 2
    end
end

cd $root; or exit 1

set -l debug_packages -p nightshift-daemon -p nightshift-osc
if test $with_ui -eq 1
    set -a debug_packages -p nightshift-ui
end
if test $with_hr -eq 1
    set -a debug_packages -p nightshift-hr-worker
end
if test $with_vrchat -eq 1
    set -a debug_packages -p nightshift-vrchat-worker
end

echo "Building Nightshift core..."
cargo build $debug_packages; or exit $status
cargo build --release -p nightshift-overlay; or exit $status

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

if test $with_hr -eq 1
    echo "Starting Nightshift heart-rate worker..."
    $root/target/debug/nightshift-hr-worker &
    set -a nightshift_children $last_pid
end

if test $with_vrchat -eq 1
    echo "Starting Nightshift VRChat worker..."
    $root/target/debug/nightshift-vrchat-worker &
    set -a nightshift_children $last_pid
end

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
if test $with_hr -eq 0
    echo "  heart-rate worker: off (use --hr or --full)"
end
if test $with_vrchat -eq 0
    echo "  VRChat worker: off (use --vrchat or --full)"
end
if test $with_ui -eq 0
    echo "  desktop UI: off (use --ui or --full)"
end
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
