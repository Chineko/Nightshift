use std::{env, path::PathBuf, time::Duration};

use anyhow::{Context, Result, bail};
use nightshift_protocol::{
    ClientMessage, PROTOCOL_VERSION, ServerMessage, VrchatStatus, WorkerReport,
};
use nightshift_vrchat_api::{SessionCookies, VrchatApiClient};
use nightshift_vrchat_session::{VrchatSession, VrchatSessionStore};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};
use tracing::{info, warn};

const RECONNECT_DELAY: Duration = Duration::from_secs(1);
const REFRESH_INTERVAL: Duration = Duration::from_secs(60);

fn socket_path() -> Result<PathBuf> {
    let runtime_dir = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .context("XDG_RUNTIME_DIR is not set")?;
    Ok(runtime_dir.join("nightshift").join("nightshift.sock"))
}

fn disconnected_report() -> WorkerReport {
    WorkerReport::VrchatRuntime {
        connected: false,
        display_name: None,
        status: VrchatStatus::Offline,
    }
}

fn session_cookies(session: &VrchatSession) -> SessionCookies {
    SessionCookies {
        auth: session.auth_cookie.clone(),
        two_factor_auth: session.two_factor_cookie.clone(),
    }
}

async fn load_session(store: VrchatSessionStore) -> Result<Option<VrchatSession>> {
    tokio::task::spawn_blocking(move || store.load())
        .await
        .context("VRChat session keyring task stopped")?
}

async fn save_session(store: VrchatSessionStore, session: VrchatSession) -> Result<()> {
    tokio::task::spawn_blocking(move || store.save(&session))
        .await
        .context("VRChat session keyring task stopped")?
}

async fn refresh_runtime(
    api: &VrchatApiClient,
    store: &VrchatSessionStore,
) -> WorkerReport {
    let mut session = match load_session(store.clone()).await {
        Ok(Some(session)) => session,
        Ok(None) => return disconnected_report(),
        Err(error) => {
            warn!(%error, "failed to read VRChat session from desktop keyring");
            return disconnected_report();
        }
    };

    let user = match api.current_user(&session_cookies(&session)).await {
        Ok(user) => user,
        Err(error) => {
            warn!(%error, "VRChat session validation failed");
            return disconnected_report();
        }
    };

    if let Some(expected_user_id) = session.user_id.as_deref() {
        if expected_user_id != user.id {
            warn!(
                expected_user_id,
                actual_user_id = %user.id,
                "VRChat keyring session resolved to an unexpected account"
            );
            return disconnected_report();
        }
    } else {
        session.user_id = Some(user.id.clone());
        if let Err(error) = save_session(store.clone(), session).await {
            warn!(%error, "failed to attach VRChat user id to stored session");
        }
    }

    WorkerReport::VrchatRuntime {
        connected: true,
        display_name: Some(user.display_name),
        status: user.status,
    }
}

async fn write_client_message(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    message: &ClientMessage,
) -> Result<()> {
    let mut bytes = serde_json::to_vec(message).context("serializing daemon IPC message")?;
    bytes.push(b'\n');
    writer
        .write_all(&bytes)
        .await
        .context("writing daemon IPC message")?;
    Ok(())
}

async fn report_if_changed(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    previous: &mut Option<WorkerReport>,
    report: WorkerReport,
) -> Result<()> {
    if previous.as_ref() == Some(&report) {
        return Ok(());
    }
    write_client_message(
        writer,
        &ClientMessage::Report {
            report: report.clone(),
        },
    )
    .await?;
    *previous = Some(report);
    Ok(())
}

async fn run_connection(stream: UnixStream) -> Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    write_client_message(
        &mut write_half,
        &ClientMessage::Hello {
            protocol: PROTOCOL_VERSION,
            client: "nightshift-vrchat".into(),
            client_version: env!("CARGO_PKG_VERSION").into(),
        },
    )
    .await?;

    let api = VrchatApiClient::new()?;
    let store = VrchatSessionStore::default();
    let mut last_report: Option<WorkerReport> = None;
    let mut interval = tokio::time::interval(REFRESH_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut line = String::new();
    let mut handshake_complete = false;

    loop {
        tokio::select! {
            read = reader.read_line(&mut line) => {
                if read.context("reading daemon IPC")? == 0 {
                    bail!("daemon closed the socket");
                }

                let message: ServerMessage = serde_json::from_str(line.trim_end())
                    .context("decoding daemon IPC message")?;
                line.clear();

                match message {
                    ServerMessage::Hello { protocol, .. } => {
                        if protocol != PROTOCOL_VERSION {
                            bail!(
                                "daemon protocol mismatch: vrchat={}, daemon={protocol}",
                                PROTOCOL_VERSION
                            );
                        }
                        handshake_complete = true;
                        info!(protocol, "Nightshift VRChat worker connected to daemon");
                        let report = refresh_runtime(&api, &store).await;
                        report_if_changed(&mut write_half, &mut last_report, report).await?;
                    }
                    ServerMessage::FullState { .. } | ServerMessage::Event { .. } => {}
                    ServerMessage::Error { message } => {
                        warn!(%message, "daemon rejected VRChat worker message");
                    }
                }
            }
            _ = interval.tick(), if handshake_complete => {
                let report = refresh_runtime(&api, &store).await;
                report_if_changed(&mut write_half, &mut last_report, report).await?;
            }
        }
    }
}

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
mod tests {
    use super::*;

    #[test]
    fn converts_stored_session_to_vrchat_cookie_names() {
        let session = VrchatSession {
            auth_cookie: "auth-cookie".into(),
            two_factor_cookie: Some("2fa-cookie".into()),
            user_id: None,
        };
        assert_eq!(
            session_cookies(&session).cookie_header(),
            "auth=auth-cookie; twoFactorAuth=2fa-cookie"
        );
    }

    #[test]
    fn disconnected_report_is_explicitly_offline() {
        assert_eq!(
            disconnected_report(),
            WorkerReport::VrchatRuntime {
                connected: false,
                display_name: None,
                status: VrchatStatus::Offline,
            }
        );
    }
}
