use super::*;

pub(crate) async fn write_client_message(
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

pub(crate) async fn report_if_changed(
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

pub(crate) async fn run_connection(stream: UnixStream) -> Result<()> {
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

    let mut line = String::new();
    let read = tokio::time::timeout(
        nightshift_protocol::IPC_HELLO_TIMEOUT,
        reader.read_line(&mut line),
    )
    .await
    .context("timed out waiting for daemon hello")??;
    if read == 0 {
        bail!("daemon closed the socket during handshake");
    }
    match serde_json::from_str::<ServerMessage>(line.trim_end())
        .context("decoding daemon hello")?
    {
        ServerMessage::Hello { protocol, .. } if protocol == PROTOCOL_VERSION => {
            info!(protocol, "Nightshift VRChat worker connected to daemon");
        }
        ServerMessage::Hello { protocol, .. } => bail!(
            "daemon protocol mismatch: vrchat={}, daemon={protocol}",
            PROTOCOL_VERSION
        ),
        ServerMessage::Error { message } => bail!("daemon rejected connection: {message}"),
        _ => bail!("daemon returned an unexpected handshake response"),
    }
    line.clear();

    let api = VrchatApiClient::new()?;
    let store = VrchatSessionStore::default();
    let mut last_report: Option<WorkerReport> = None;
    let mut interval = tokio::time::interval(REFRESH_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    interval.reset();
    let mut state_synchronized = false;

    let mut sleeping = false;
    let mut vrchat_connected = false;
    let mut automation = StatusAutomationConfig::default();
    let mut last_status_write: Option<Instant> = None;
    let mut status_generation = 0_u64;
    let mut status_task = AbortTask(None);
    let (status_tx, mut status_rx) = mpsc::unbounded_channel::<StatusTaskResult>();

    let mut pipeline_task = AbortTask(None);
    let mut pipeline_auth_token: Option<String> = None;
    let (pipeline_tx, mut pipeline_rx) = mpsc::unbounded_channel::<PipelineEvent>();

    let mut invite_state = InviteState::default();
    let mut active_session: Option<SessionCookies> = None;
    let mut invite_tasks = JoinSet::<Result<InviteHandlingResult>>::new();
    let mut recent_invites = RecentInviteNotifications::default();
    let instance_source = Arc::new(Mutex::new(InstanceSource::default()));

    loop {
        tokio::select! {
            read = reader.read_line(&mut line) => {
                if read.context("reading daemon IPC")? == 0 {
                    status_task.abort();
                    bail!("daemon closed the socket");
                }

                let message: ServerMessage = serde_json::from_str(line.trim_end())
                    .context("decoding daemon IPC message")?;
                line.clear();

                match message {
                    ServerMessage::Hello { .. } => bail!("daemon repeated IPC hello after handshake"),
                    ServerMessage::FullState { state } => {
                        // Snapshot synchronization must not force the awake status. If the worker
                        // attaches while sleep mode is already active, however, reconcile the
                        // configured sleep status because there will be no new sleep edge.
                        sleeping = sleeping_phase(state.sleep.phase);
                        automation = status_config(&state.settings);
                        sync_invite_state(&mut invite_state, state.invites, &mut invite_tasks);

                        if !state_synchronized {
                            state_synchronized = true;
                            let RuntimeRefresh {
                                report,
                                pipeline_auth_token: next_pipeline_auth_token,
                                session_cookies: next_session,
                            } = refresh_runtime(&api, &store).await;
                            (vrchat_connected, _) = report_runtime(&report);
                            let session_changed = sync_invite_session(
                                &mut active_session,
                                next_session,
                                &mut invite_tasks,
                                &mut recent_invites,
                            );
                            cancel_pending_status_on_session_change(
                                session_changed,
                                &mut status_generation,
                                &mut status_task,
                            );
                            sync_pipeline_task(
                                &mut pipeline_task,
                                &mut pipeline_auth_token,
                                next_pipeline_auth_token,
                                &pipeline_tx,
                            );
                            if sleeping && automation.enabled && vrchat_connected {
                                schedule_status_update(
                                    &mut status_task,
                                    status_tx.clone(),
                                    api.clone(),
                                    store.clone(),
                                    automation.sleep_status,
                                    active_session.clone(),
                                    last_status_write,
                                    status_generation,
                                );
                            }
                            report_if_changed(&mut write_half, &mut last_report, report).await?;
                        }
                    }
                    ServerMessage::Event { event } => match event {
                        StateEvent::SettingsChanged(settings) => {
                            let previous = automation;
                            automation = status_config(&settings);

                            if automation != previous {
                                let pending = status_task.is_pending();
                                cancel_status_edge(
                                    pending,
                                    &mut status_generation,
                                    &mut status_task,
                                );

                                // A changed sleep target should take effect while already sleeping.
                                // While awake, wait for an actual wake edge instead of overwriting a
                                // manually chosen VRChat status merely because settings changed.
                                if sleeping && automation.enabled && vrchat_connected {
                                    schedule_status_update(
                                        &mut status_task,
                                        status_tx.clone(),
                                        api.clone(),
                                        store.clone(),
                                        automation.sleep_status,
                                        active_session.clone(),
                                        last_status_write,
                                        status_generation,
                                    );
                                }
                            }
                        }
                        StateEvent::VrchatChanged(state) => {
                            let was_connected = vrchat_connected;
                            vrchat_connected = state.connected;
                            cancel_pending_status_on_disconnect(
                                vrchat_connected,
                                &mut status_generation,
                                &mut status_task,
                            );
                            if !vrchat_connected {
                                sync_invite_session(
                                    &mut active_session,
                                    None,
                                    &mut invite_tasks,
                                    &mut recent_invites,
                                );
                                sync_pipeline_task(
                                    &mut pipeline_task,
                                    &mut pipeline_auth_token,
                                    None,
                                    &pipeline_tx,
                                );
                            } else if !was_connected && sleeping && automation.enabled {
                                // Reconnecting while already asleep has no fresh sleep edge, so
                                // restore the desired sleep-mode status for the active session.
                                schedule_status_update(
                                    &mut status_task,
                                    status_tx.clone(),
                                    api.clone(),
                                    store.clone(),
                                    automation.sleep_status,
                                    active_session.clone(),
                                    last_status_write,
                                    status_generation,
                                );
                            }
                        }
                        StateEvent::SleepChanged(state) => {
                            let next_sleeping = sleeping_phase(state.phase);
                            let edge = next_sleeping != sleeping;
                            sleeping = next_sleeping;

                            if edge {
                                cancel_status_edge(true, &mut status_generation, &mut status_task);

                                if automation.enabled && vrchat_connected {
                                    schedule_status_update(
                                        &mut status_task,
                                        status_tx.clone(),
                                        api.clone(),
                                        store.clone(),
                                        status_for_mode(automation, sleeping),
                                        active_session.clone(),
                                        last_status_write,
                                        status_generation,
                                    );
                                }
                            }
                        }
                        StateEvent::InvitesChanged(state) => {
                            sync_invite_state(&mut invite_state, state, &mut invite_tasks);
                        }
                        _ => {}
                    },
                    ServerMessage::Error { message } => {
                        warn!(%message, "daemon rejected VRChat worker message");
                    }
                }
            }
            _ = interval.tick(), if state_synchronized => {
                let RuntimeRefresh {
                    report,
                    pipeline_auth_token: next_pipeline_auth_token,
                    session_cookies: next_session,
                } = refresh_runtime(&api, &store).await;
                (vrchat_connected, _) = report_runtime(&report);
                cancel_pending_status_on_disconnect(
                    vrchat_connected,
                    &mut status_generation,
                    &mut status_task,
                );
                let session_changed = sync_invite_session(
                    &mut active_session,
                    next_session,
                    &mut invite_tasks,
                    &mut recent_invites,
                );
                cancel_pending_status_on_session_change(
                    session_changed,
                    &mut status_generation,
                    &mut status_task,
                );
                sync_pipeline_task(
                    &mut pipeline_task,
                    &mut pipeline_auth_token,
                    next_pipeline_auth_token,
                    &pipeline_tx,
                );
                if session_changed && sleeping && automation.enabled && vrchat_connected {
                    schedule_status_update(
                        &mut status_task,
                        status_tx.clone(),
                        api.clone(),
                        store.clone(),
                        automation.sleep_status,
                        active_session.clone(),
                        last_status_write,
                        status_generation,
                    );
                }
                report_if_changed(&mut write_half, &mut last_report, report).await?;
            }
            Some(task_result) = status_rx.recv() => {
                if task_result.generation != status_generation {
                    continue;
                }
                status_task.0 = None;
                match task_result.result {
                    Ok(refresh) => {
                        last_status_write = Some(Instant::now());
                        let RuntimeRefresh {
                            report,
                            pipeline_auth_token: next_pipeline_auth_token,
                            session_cookies: next_session,
                        } = refresh;
                        (vrchat_connected, _) = report_runtime(&report);
                        let session_changed = sync_invite_session(
                            &mut active_session,
                            next_session,
                            &mut invite_tasks,
                            &mut recent_invites,
                        );
                        cancel_pending_status_on_session_change(
                            session_changed,
                            &mut status_generation,
                            &mut status_task,
                        );
                        sync_pipeline_task(
                            &mut pipeline_task,
                            &mut pipeline_auth_token,
                            next_pipeline_auth_token,
                            &pipeline_tx,
                        );
                        report_if_changed(&mut write_half, &mut last_report, report).await?;
                    }
                    Err(error) => {
                        warn!(%error, "VRChat sleep/wake status update failed");
                    }
                }
            }
            Some(pipeline_event) = pipeline_rx.recv() => {
                match pipeline_event {
                    PipelineEvent::RevalidateSession => {
                        let RuntimeRefresh {
                            report,
                            pipeline_auth_token: next_pipeline_auth_token,
                            session_cookies: next_session,
                        } = refresh_runtime(&api, &store).await;
                        (vrchat_connected, _) = report_runtime(&report);
                        cancel_pending_status_on_disconnect(
                            vrchat_connected,
                            &mut status_generation,
                            &mut status_task,
                        );
                        let session_changed = sync_invite_session(
                            &mut active_session,
                            next_session,
                            &mut invite_tasks,
                            &mut recent_invites,
                        );
                        cancel_pending_status_on_session_change(
                            session_changed,
                            &mut status_generation,
                            &mut status_task,
                        );
                        sync_pipeline_task(
                            &mut pipeline_task,
                            &mut pipeline_auth_token,
                            next_pipeline_auth_token,
                            &pipeline_tx,
                        );
                        if session_changed && sleeping && automation.enabled && vrchat_connected {
                            schedule_status_update(
                                &mut status_task,
                                status_tx.clone(),
                                api.clone(),
                                store.clone(),
                                automation.sleep_status,
                                active_session.clone(),
                                last_status_write,
                                status_generation,
                            );
                        }
                        report_if_changed(&mut write_half, &mut last_report, report).await?;
                    }
                    PipelineEvent::Notification(notification) => {
                        if !recent_invites.observe(&notification.id) {
                            continue;
                        }

                        info!(
                            notification_id = %notification.id,
                            sender_user_id = %notification.sender_user_id,
                            sender_username = notification.sender_username.as_deref().unwrap_or("<unknown>"),
                            kind = ?notification.kind,
                            "received VRChat invite notification"
                        );

                        if let Some(cookies) = active_session.clone() {
                            let api = api.clone();
                            let state = invite_state.clone();
                            let instance_source = instance_source.clone();
                            invite_tasks.spawn(async move {
                                handle_invite_notification(
                                    api,
                                    cookies,
                                    state,
                                    instance_source,
                                    notification,
                                )
                                .await
                            });
                        } else {
                            warn!(
                                notification_id = %notification.id,
                                "ignored VRChat invite notification because no validated session is active"
                            );
                        }
                    }
                }
            }
            Some(join_result) = invite_tasks.join_next(), if !invite_tasks.is_empty() => {
                match join_result {
                    Ok(Ok(result)) => {
                        log_invite_result(&result);
                        let (last_invite_action, invite_error) = invite_feedback(&result);
                        if (last_invite_action.is_some() || invite_error.is_some())
                            && let Some(report) = runtime_feedback_report(
                                &last_report,
                                last_invite_action,
                                invite_error,
                            )
                        {
                            report_if_changed(&mut write_half, &mut last_report, report).await?;
                        }
                    }
                    Ok(Err(error)) => {
                        warn!(%error, "VRChat invite automation task failed without taking a fallback action");
                        if let Some(report) = runtime_feedback_report(
                            &last_report,
                            None,
                            Some(error.to_string()),
                        ) {
                            report_if_changed(&mut write_half, &mut last_report, report).await?;
                        }
                    }
                    Err(error) if error.is_cancelled() => {}
                    Err(error) => {
                        warn!(%error, "VRChat invite automation task stopped unexpectedly");
                        if let Some(report) = runtime_feedback_report(
                            &last_report,
                            None,
                            Some("VRChat invite automation task stopped unexpectedly".into()),
                        ) {
                            report_if_changed(&mut write_half, &mut last_report, report).await?;
                        }
                    }
                }
            }
        }
    }
}
