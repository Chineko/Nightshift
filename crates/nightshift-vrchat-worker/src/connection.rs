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

pub(crate) fn schedule_status_update(
    task: &mut AbortTask,
    result_tx: mpsc::UnboundedSender<StatusTaskResult>,
    api: VrchatApiClient,
    store: VrchatSessionStore,
    target: nightshift_vrchat_status::VrchatPresence,
    expected_session: Option<SessionCookies>,
    last_write: Option<Instant>,
    generation: u64,
) {
    let delay = status_update_delay(last_write);
    task.replace(tokio::spawn(async move {
        tokio::time::sleep(delay).await;
        let result = set_runtime_presence(api, store, target, expected_session).await;
        let _ = result_tx.send(StatusTaskResult { generation, result });
    }));
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

    let api = VrchatApiClient::new()?;
    let store = VrchatSessionStore::default();
    let mut last_report: Option<WorkerReport> = None;
    let mut interval = tokio::time::interval(REFRESH_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    interval.reset();
    let mut line = String::new();
    let mut state_synchronized = false;

    let mut sleeping = false;
    let mut vrchat_connected = false;
    let mut current_presence = offline_presence();
    let mut captured_presence: Option<nightshift_vrchat_status::VrchatPresence> = None;
    let mut automation = StatusAutomationConfig::default();
    let mut last_status_write: Option<Instant> = None;
    let mut status_generation = 0_u64;
    let mut status_task = AbortTask(None);
    let (status_tx, mut status_rx) = mpsc::unbounded_channel::<StatusTaskResult>();

    let mut pipeline_task = AbortTask(None);
    let mut pipeline_auth_token: Option<String> = None;
    let (pipeline_tx, mut pipeline_rx) = mpsc::unbounded_channel::<VrchatNotification>();

    let mut invite_state = InviteState::default();
    let mut active_session: Option<SessionCookies> = None;
    let mut invite_tasks = JoinSet::<Result<InviteHandlingResult>>::new();
    let mut recent_invites = RecentInviteNotifications::default();
    let instance_source = Arc::new(Mutex::new(InstanceSource::default()));

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
                        info!(protocol, "Nightshift VRChat worker connected to daemon");
                    }
                    ServerMessage::FullState { state } => {
                        // Snapshot is synchronization, not a fresh Sleep/Wake edge.
                        sleeping = sleeping_phase(state.sleep.phase);
                        automation = status_config(&state.settings);
                        sync_invite_state(&mut invite_state, state.invites, &mut invite_tasks);

                        if !state_synchronized {
                            state_synchronized = true;
                            let RuntimeRefresh {
                                report,
                                presence,
                                pipeline_auth_token: next_pipeline_auth_token,
                                session_cookies: next_session,
                            } = refresh_runtime(&api, &store).await;
                            (vrchat_connected, _) = report_runtime(&report);
                            current_presence = presence.unwrap_or_else(offline_presence);
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
                            if session_changed {
                                captured_presence = None;
                            }
                            sync_pipeline_task(
                                &mut pipeline_task,
                                &mut pipeline_auth_token,
                                next_pipeline_auth_token,
                                &pipeline_tx,
                            );
                            report_if_changed(&mut write_half, &mut last_report, report).await?;
                        }
                    }
                    ServerMessage::Event { event } => match event {
                        StateEvent::SettingsChanged(settings) => {
                            let previous = automation;
                            let pending = status_task.is_pending();
                            automation = status_config(&settings);

                            if pending && automation != previous {
                                status_generation = status_generation.wrapping_add(1);
                                status_task.abort();
                                if let Some(target) = pending_presence_retarget(
                                    true,
                                    sleeping,
                                    vrchat_connected,
                                    &current_presence,
                                    captured_presence.as_ref(),
                                    automation,
                                ) {
                                    schedule_status_update(
                                        &mut status_task,
                                        status_tx.clone(),
                                        api.clone(),
                                        store.clone(),
                                        target,
                                        active_session.clone(),
                                        last_status_write,
                                        status_generation,
                                    );
                                }
                            }
                            if !automation.enabled && !sleeping {
                                captured_presence = None;
                            }
                        }
                        StateEvent::VrchatChanged(state) => {
                            vrchat_connected = state.connected;
                            current_presence.status = if state.connected {
                                state.status
                            } else {
                                VrchatStatus::Offline
                            };
                            if !state.connected {
                                current_presence.status_description.clear();
                            }
                            cancel_pending_status_on_disconnect(
                                vrchat_connected,
                                &mut status_generation,
                                &mut status_task,
                            );
                            if !vrchat_connected {
                                let session_changed = sync_invite_session(
                                    &mut active_session,
                                    None,
                                    &mut invite_tasks,
                                    &mut recent_invites,
                                );
                                if session_changed {
                                    captured_presence = None;
                                }
                                sync_pipeline_task(
                                    &mut pipeline_task,
                                    &mut pipeline_auth_token,
                                    None,
                                    &pipeline_tx,
                                );
                            }
                        }
                        StateEvent::SleepChanged(state) => {
                            let next_sleeping = sleeping_phase(state.phase);
                            let edge = next_sleeping != sleeping;
                            if edge
                                && next_sleeping
                                && automation.enabled
                                && vrchat_connected
                                && captured_presence.is_none()
                            {
                                // One ticket home per sleep session. Do not overwrite it with our own pajamas.
                                captured_presence = Some(current_presence.clone());
                            }
                            let target = if edge {
                                pending_presence_retarget(
                                    true,
                                    next_sleeping,
                                    vrchat_connected,
                                    &current_presence,
                                    captured_presence.as_ref(),
                                    automation,
                                )
                            } else {
                                None
                            };
                            sleeping = next_sleeping;
                            if let Some(target) = target {
                                status_generation = status_generation.wrapping_add(1);
                                schedule_status_update(
                                    &mut status_task,
                                    status_tx.clone(),
                                    api.clone(),
                                    store.clone(),
                                    target,
                                    active_session.clone(),
                                    last_status_write,
                                    status_generation,
                                );
                            } else if edge && !sleeping {
                                captured_presence = None;
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
                    presence,
                    pipeline_auth_token: next_pipeline_auth_token,
                    session_cookies: next_session,
                } = refresh_runtime(&api, &store).await;
                (vrchat_connected, _) = report_runtime(&report);
                current_presence = presence.unwrap_or_else(offline_presence);
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
                if session_changed {
                    captured_presence = None;
                }
                sync_pipeline_task(
                    &mut pipeline_task,
                    &mut pipeline_auth_token,
                    next_pipeline_auth_token,
                    &pipeline_tx,
                );
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
                            presence,
                            pipeline_auth_token: next_pipeline_auth_token,
                            session_cookies: next_session,
                        } = refresh;
                        (vrchat_connected, _) = report_runtime(&report);
                        current_presence = presence.unwrap_or_else(offline_presence);
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
                        if session_changed {
                            captured_presence = None;
                        } else if !sleeping {
                            // A successful awake write is the verified restore. Ticket punched; goblin goes home.
                            captured_presence = None;
                        }
                        sync_pipeline_task(
                            &mut pipeline_task,
                            &mut pipeline_auth_token,
                            next_pipeline_auth_token,
                            &pipeline_tx,
                        );
                        report_if_changed(&mut write_half, &mut last_report, report).await?;
                    }
                    Err(error) => {
                        warn!(%error, "VRChat sleep/wake presence update failed");
                    }
                }
            }
            Some(notification) = pipeline_rx.recv() => {
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
