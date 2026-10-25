mod connection;
mod pipeline;
mod runtime;

use std::{
    env,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use nightshift_protocol::{
    ClientMessage, InviteAction, InviteActionKind, InviteState, PROTOCOL_VERSION, ServerMessage,
    StateEvent, VrchatStatus, WorkerReport,
};
use nightshift_vrchat_api::{
    SessionCookies, VrchatApiClient, VrchatNotification, parse_pipeline_notification, pipeline_url,
};
use nightshift_vrchat_session::{VrchatSession, VrchatSessionStore};
use nightshift_vrchat_status::{StatusAutomationConfig, sleeping_phase};
use nightshift_vrchat_worker::{
    instance_context::InstanceSource,
    invite_bridge::{
        InviteHandlingOutcome, InviteHandlingResult, handle_invite_notification,
    },
    invite_dedup::RecentInviteNotifications,
    invite_execute::InviteExecutionOutcome,
    request_coordinator::{RequestClass, shared_vrchat_requests},
};
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    sync::mpsc,
    task::{JoinHandle, JoinSet},
};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{info, warn};

const RECONNECT_DELAY: Duration = Duration::from_secs(1);
const REFRESH_INTERVAL: Duration = Duration::from_secs(60);
const STATUS_DEBOUNCE: Duration = Duration::from_secs(3);
const STATUS_MIN_INTERVAL: Duration = Duration::from_secs(10);
const PIPELINE_RECONNECT_DELAY: Duration = Duration::from_secs(10);
const PIPELINE_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

use connection::*;
use pipeline::*;
use runtime::*;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "nightshift_vrchat=info".into()),
        )
        .init();

    let path = socket_path()?;
    loop {
        match UnixStream::connect(&path).await {
            Ok(stream) => {
                if let Err(error) = run_connection(stream).await {
                    warn!(%error, "VRChat worker disconnected from daemon");
                }
            }
            Err(error) => {
                warn!(%error, path = %path.display(), "Nightshift daemon unavailable");
            }
        }
        tokio::time::sleep(RECONNECT_DELAY).await;
    }
}

#[cfg(test)]
mod tests;
