#!/usr/bin/env fish

set -l script_dir (dirname (status --current-filename))
set -l root (realpath "$script_dir/..")
set -l hydra
set -l with_ui 1
set -l with_hr 1
set -l with_perf 0

function nightshift_usage
    echo "Usage: scripts/nightshift-dev.fish [--hydra] [--perf] [--no-ui] [--no-hr] [--core] [--full]"
    echo
    echo "By default this builds and starts the full local Nightshift stack:"
    echo "  daemon + OSC + heart-rate + OpenXR overlay + desktop UI"
    echo "  VRChat worker is supervised automatically by the daemon"
    echo
    echo "  --hydra    Enable overlay Hydra diagnostics"
    echo "  --perf     Sample Nightshift CPU/RAM/threads/FDs into a text report"
    echo "  --no-ui    Do not start the desktop UI"
    echo "  --no-hr    Do not start the BLE heart-rate worker"
    echo "  --core     Start only daemon + OSC + OpenXR overlay"
    echo "  --full     Explicitly select the default full stack"
    echo
    echo "Compatibility flags --ui, --hr, and --vrchat are still accepted."
end

for arg in $argv
    switch $arg
        case --hydra-mode --hydra
            set hydra --hydra-mode
        case --perf
            set with_perf 1
        case --ui
            set with_ui 1
        case --hr
            set with_hr 1
        case --vrchat
            # The daemon owns VRChat worker supervision now. Keep this flag accepted
            # so old muscle memory does not summon a duplicate worker hydra head.
        case --no-ui
            set with_ui 0
        case --no-hr
            set with_hr 0
        case --core
            set with_ui 0
            set with_hr 0
        case --full --all
            set with_ui 1
            set with_hr 1
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

echo "Building full Nightshift stack..."
cargo build \
    -p nightshift-daemon \
    -p nightshift-osc \
    -p nightshift-hr-worker \
    -p nightshift-ui \
    -p nightshift-vrchat-worker; or exit $status
cargo build --release -p nightshift-overlay; or exit $status

set -g nightshift_children
set -g nightshift_perf_pid
set -g nightshift_perf_report

function nightshift_stop_perf
    if test -n "$nightshift_perf_pid"
        kill $nightshift_perf_pid 2>/dev/null
        wait $nightshift_perf_pid 2>/dev/null
        set -g nightshift_perf_pid
    end
end

function nightshift_cleanup --on-signal INT --on-signal TERM
    if test (count $nightshift_children) -gt 0
        echo
        echo "Stopping Nightshift hydra..."
        kill $nightshift_children 2>/dev/null
        wait $nightshift_children 2>/dev/null
        set -g nightshift_children
    end
    nightshift_stop_perf
end

function nightshift_exit_cleanup --on-event fish_exit
    if test (count $nightshift_children) -gt 0
        kill $nightshift_children 2>/dev/null
        wait $nightshift_children 2>/dev/null
    end
    nightshift_stop_perf
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

# VRChat is deliberately not spawned here. The daemon supervises the current
# workspace worker, avoids duplicates, restarts it on failure, and owns cleanup.
echo "VRChat worker: managed by daemon"

echo "Starting Nightshift OpenXR overlay..."
$root/target/release/nightshift-overlay $hydra &
set -a nightshift_children $last_pid

if test $with_ui -eq 1
    echo "Starting Nightshift desktop UI..."
    $root/target/debug/nightshift-ui &
    set -a nightshift_children $last_pid
end

if test $with_perf -eq 1
    set -g nightshift_perf_report "$root/target/nightshift-perf-"(date +%Y%m%d-%H%M%S)".txt"
    echo "Starting Nightshift performance sampler..."
    fish "$root/scripts/nightshift-perf.fish" "$nightshift_perf_report" &
    set -g nightshift_perf_pid $last_pid
end

echo
echo "Nightshift is running. Ctrl+C stops the whole local stack."
if test $with_hr -eq 0
    echo "  heart-rate worker: off"
end
if test $with_ui -eq 0
    echo "  desktop UI: off"
end
echo "  VRChat worker: daemon-supervised"
if test $with_perf -eq 1
    echo "  performance report: $nightshift_perf_report"
end
echo "WayVR remains a separate process, because one hydra is enough."
echo

# Exit when any directly supervised child exits unexpectedly; the fish_exit
# handler tears down the remaining processes, and daemon shutdown owns its
# managed VRChat child so we never leave workers lurking under the bed.
wait -n $nightshift_children
set -l child_status $status

if test $child_status -ne 130 -a $child_status -ne 143
    echo "A Nightshift process exited (status $child_status); stopping the stack." >&2
end

exit $child_status
