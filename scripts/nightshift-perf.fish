#!/usr/bin/env fish

set -l root (realpath (dirname (status --current-filename))/..)
set -l output $argv[1]
if test -z "$output"
    set output "$root/target/nightshift-perf-"(date +%Y%m%d-%H%M%S)".txt"
end

mkdir -p (dirname "$output")

echo -e "# Nightshift performance report\n# started\t"(date --iso-8601=seconds)"\n# sample_interval_seconds\t2\n# fields\tkind\ttimestamp\tpid\tprocess\tcpu_percent\trss_mib\tthreads\topen_fds" > "$output"

function nightshift_perf_summary --argument-names output
    echo >> "$output"
    echo -e "# summary\n# fields\tprocess\tsamples\tavg_cpu_percent\tpeak_cpu_percent\tavg_rss_mib\tpeak_rss_mib\tmax_threads\tmax_open_fds" >> "$output"
    awk -F '\t' '
        $1 == "sample" {
            name=$4; count[name]++;
            cpu_sum[name]+=$5; if ($5>cpu_max[name]) cpu_max[name]=$5;
            rss_sum[name]+=$6; if ($6>rss_max[name]) rss_max[name]=$6;
            if ($7>threads_max[name]) threads_max[name]=$7;
            if ($8>fds_max[name]) fds_max[name]=$8;
        }
        END {
            for (name in count) {
                printf "summary\t%s\t%d\t%.2f\t%.2f\t%.2f\t%.2f\t%d\t%d\n",
                    name, count[name], cpu_sum[name]/count[name], cpu_max[name],
                    rss_sum[name]/count[name], rss_max[name], threads_max[name], fds_max[name];
            }
        }
    ' "$output" | sort >> "$output"
    echo -e "# finished\t"(date --iso-8601=seconds) >> "$output"
end

function nightshift_perf_stop --on-signal INT --on-signal TERM
    nightshift_perf_summary "$output"
    echo "Nightshift performance report written to $output"
    exit 0
end

while true
    set -l timestamp (date --iso-8601=seconds)
    ps -u (id -u) -o pid=,comm=,%cpu=,rss=,nlwp= | while read -l pid process cpu rss_kib threads
        if not string match -qr '^nightshift-' -- "$process"
            continue
        end

        set -l fds 0
        if test -d "/proc/$pid/fd"
            set fds (count /proc/$pid/fd/* 2>/dev/null)
        end

        set -l rss_mib (awk -v kib="$rss_kib" 'BEGIN { printf "%.2f", kib / 1024.0 }')
        echo -e "sample\t$timestamp\t$pid\t$process\t$cpu\t$rss_mib\t$threads\t$fds" >> "$output"
    end
    sleep 2
end
