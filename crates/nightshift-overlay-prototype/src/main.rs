mod content;
mod controller_input;
mod home_layout;
mod interaction;
mod ipc;
mod laser;
mod layout;
mod panel;
mod panel_cache;
mod panel_state;
mod placement;
mod raster;
mod runtime;
mod runtime_state;
mod surface;
mod widget;

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

use anyhow::{Context, Result, anyhow};

use ipc::spawn_ipc_client;
use runtime::{DEFAULT_PLACEMENT, run_overlay};
use runtime_state::DEFAULT_TRANSITION_SECONDS;

// Hydra IPC diagnostics live in ipc.rs; keep the [Hydra/IPC] contract healthy.

fn main() -> Result<()> {
    let placement = parse_flag_u32("--placement")?.unwrap_or(DEFAULT_PLACEMENT);
    let hydra_mode = has_flag("--hydra-mode");

    println!("Nightshift OpenXR overlay");
    println!("  placement  : {placement}");
    println!("  daemon IPC : enabled");
    println!("  fallback fade default : {DEFAULT_TRANSITION_SECONDS:.1}s");
    println!(
        "  hydra mode : {}",
        if hydra_mode { "AWAKENED" } else { "sleeping" }
    );
    println!("  Ctrl+C to stop");
    println!();
    println!("The tiny evil square now takes orders from the daemon.");
    if hydra_mode {
        println!("Hydra has entered the chat. Terminal dignity is no longer guaranteed.");
    }

    let running = Arc::new(AtomicBool::new(true));
    let signal = running.clone();
    ctrlc::set_handler(move || signal.store(false, Ordering::Relaxed))
        .context("installing Ctrl+C handler")?;

    let (updates_tx, updates_rx) = mpsc::channel();
    let (reports_tx, reports_rx) = mpsc::channel();
    let (commands_tx, commands_rx) = mpsc::channel();
    spawn_ipc_client(
        running.clone(),
        updates_tx,
        reports_rx,
        commands_rx,
        hydra_mode,
    );

    run_overlay(
        running,
        updates_rx,
        reports_tx,
        commands_tx,
        placement,
        hydra_mode,
    )
}

fn parse_flag_u32(name: &str) -> Result<Option<u32>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let mut index = 0;

    while index < args.len() {
        if args[index] == name {
            let value = args
                .get(index + 1)
                .ok_or_else(|| anyhow!("missing value for {name}"))?;
            return value
                .parse::<u32>()
                .with_context(|| format!("parsing {name} value {value}"))
                .map(Some);
        }
        index += 1;
    }

    Ok(None)
}

fn has_flag(name: &str) -> bool {
    std::env::args().skip(1).any(|arg| arg == name)
}
