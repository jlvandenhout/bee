// Copyright 2021 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event::*,
    grpc::{plugin_client::PluginClient, ShutdownRequest},
    streamer::PluginStreamer,
    PluginError, PluginHandshake, PluginId, UniqueId,
};

use bee_event_bus::EventBus;

use log::{debug, info, warn};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    select, spawn,
    sync::{mpsc::unbounded_channel, oneshot::Sender},
    task::JoinHandle,
    time::sleep,
};
use tonic::{transport::Channel, Request};

use std::{
    any::type_name,
    collections::{hash_map::Entry, HashMap},
    process::Stdio,
    time::Duration,
};

/// A handler for a plugin.
pub(crate) struct PluginHandler {
    /// The name of the plugin.
    name: String,
    /// The identifier of the plugin.
    plugin_id: PluginId,
    /// Shutdown for every `PluginStreamer` used by the plugin.
    shutdowns: HashMap<EventId, Sender<()>>,
    /// The OS process running the plugin.
    process: Child,
    /// The gRPC client.
    client: PluginClient<Channel>,
    /// The task handling stdio redirection.
    stdio_task: JoinHandle<Result<(), std::io::Error>>,
}

impl PluginHandler {
    /// Creates a new [`PluginHandler`] from a process running the plugin logic.
    pub(crate) async fn new(
        plugin_id: PluginId,
        mut command: Command,
        bus: &EventBus<'static, UniqueId>,
    ) -> Result<Self, PluginError> {
        command.kill_on_drop(true).stdout(Stdio::piped()).stderr(Stdio::piped());

        debug!(
            "spawning command `{:?}` for the plugin with identifier {}",
            command, plugin_id
        );
        let mut process = command.spawn()?;

        // `stdout` is guaranteed to be `Some` because we piped it in the command.
        let mut stdout = BufReader::new(process.stdout.take().unwrap());
        // `stderr` is guaranteed to be `Some` because we piped it in the command.
        let stderr = BufReader::new(process.stderr.take().unwrap());

        let mut buf = String::new();
        stdout.read_line(&mut buf).await?;
        let handshake = PluginHandshake::parse(&buf)?;

        let name = format!("{}-{}", handshake.name, plugin_id);
        let target = format!("plugins::{}", name);

        let stdio_task = tokio::spawn(async move {
            let mut stdout_lines = stdout.lines();
            let mut stderr_lines = stderr.lines();

            loop {
                tokio::select! {
                    res = stdout_lines.next_line() => match res? {
                        Some(line) => {
                            info!(target: &target, "{}", line);
                        },
                        None => break,
                    },
                    res = stderr_lines.next_line() => match res? {
                        Some(line) => {
                            warn!(target: &target, "{}", line);
                        },
                        None => break,
                    }
                }
            }

            Ok(())
        });

        let address = format!("http://{}/", handshake.address);
        debug!("connecting to the \"{}\" plugin at {}", name, address);

        let client = async {
            let mut count = 0;
            loop {
                match PluginClient::connect(address.clone()).await {
                    Ok(client) => break Ok(client),
                    Err(e) => {
                        warn!("connection to the \"{}\" plugin failed: {}", name, e);
                        if count == 5 {
                            warn!("connection to the \"{}\" plugin will not be retried anymore", name);
                            break Err(e);
                        } else {
                            let secs = 5u64.pow(count);
                            warn!(
                                "connection to the \"{}\" plugin will be retried in {} seconds",
                                name, secs
                            );
                            tokio::time::sleep(tokio::time::Duration::from_secs(secs)).await;
                            count += 1;
                        }
                    }
                }
            }
        }
        .await?;
        debug!("connection to the \"{}\" plugin was successful", name);

        let mut handler = Self {
            name,
            plugin_id,
            process,
            client,
            shutdowns: Default::default(),
            stdio_task,
        };

        for event_id in handshake.event_ids {
            handler.register_callback(event_id, bus);
        }

        Ok(handler)
    }

    /// Registers a callback for an event with the specified [`EventId`] in the event bus.
    fn register_callback(&mut self, event_id: EventId, bus: &EventBus<'static, UniqueId>) {
        if let Entry::Vacant(entry) = self.shutdowns.entry(event_id) {
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
            entry.insert(shutdown_tx);

            macro_rules! spawn_streamers {
                ($($event_var:pat => $event_ty:ty),*) => {{
                    match event_id {
                        $(
                            $event_var => {
                                let (tx, rx) = unbounded_channel::<$event_ty>();
                                let client = self.client.clone();

                                spawn(async move {
                                    PluginStreamer::new(rx, shutdown_rx, client).run().await;
                                });

                                debug!("registering `{}` callback for the \"{}\" plugin", type_name::<$event_ty>(), self.name);
                                bus.add_listener_with_id(move |event: &$event_ty| {
                                    if let Err(e) = tx.send(event.clone()) {
                                        warn!("failed to send event: {}", e);
                                    }
                                }, UniqueId::Object(self.plugin_id));
                            }
                        )*
                    }
                }};
            }

            spawn_streamers! {
                EventId::MessageParsed => MessageParsedEvent,
                EventId::ParsingFailed => ParsingFailedEvent,
                EventId::MessageRejected => MessageRejectedEvent
            }
        }
    }

    /// Shutdowns the plugin by shutting down all the plugin streamers, removing the plugin callbacks from the event bus
    /// and killing the plugin process.
    pub(crate) async fn shutdown(mut self, bus: &EventBus<'static, UniqueId>) -> Result<(), PluginError> {
        debug!("shutting down streamers for the \"{}\" plugin", self.name);
        for (_id, shutdown) in self.shutdowns {
            // If sending fails, this means that the receiver was already dropped which means that the streamer is
            // already gone.
            shutdown.send(()).ok();
        }

        debug!("removing callbacks for the \"{}\" plugin", self.name);
        bus.remove_listeners_with_id(self.plugin_id.into());

        debug!("sending shutdown request to the \"{}\" plugin", self.name);
        let shutdown = self.client.shutdown(Request::new(ShutdownRequest {}));
        let delay = sleep(Duration::from_secs(30));

        select! {
            result = shutdown => {
                result?;
            },
            _ = delay => {
                warn!("the shutdown request for the \"{}\" plugin timed out", self.name);
            },
        }

        self.stdio_task.abort();
        if let Err(e) = self.stdio_task.await {
            if e.is_panic() {
                warn!("stdio redirection for the \"{}\" plugin panicked: {}", self.name, e);
            }
        };

        debug!("killing process for the \"{}\" plugin", self.name);
        self.process.kill().await?;

        Ok(())
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }
}