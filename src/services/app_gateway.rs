use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender};
use futures_util::StreamExt;

use anyhow::{anyhow, Result};

use crate::domain::commands::Command;

pub struct AppGateway {
    command_handler_sink: UnboundedSender<Command>,
    session_worker_source: UnboundedReceiver<Command>,
}

impl AppGateway {
    pub fn init(
        app_sink: UnboundedSender<Command>,
        sessions_source: UnboundedReceiver<Command>,
    ) -> Self {
        Self {
            command_handler_sink: app_sink,
            session_worker_source: sessions_source,
        }
    }

    async fn session_worker_fan_in(&mut self) -> Result<()> {
        loop {
            if let Some(s) = self.session_worker_source.next().await {
                self.command_handler_sink.unbounded_send(s).unwrap()
            } else {
                return Err(anyhow!(
                    "App gateway worker stopped due to upstream channel closure"
                ));
            }
        }
    }

    pub fn run(mut self) {
        tokio::spawn(async move {
            match self.session_worker_fan_in().await {
                Err(e) => panic!("AppGateway exited abnormally with Error: {e}"),
                _ => (),
            }
        });
    }
}
