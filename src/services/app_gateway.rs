use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender};
use futures_util::StreamExt;

use super::commands::Command;

struct AppGateway {
    command_handler_sink: UnboundedSender<Command>,
    session_worker_source: UnboundedReceiver<Command>,
}

impl AppGateway {
    fn init(
        app_sink: UnboundedSender<Command>,
        sessions_source: UnboundedReceiver<Command>,
    ) -> Self {
        Self {
            command_handler_sink: app_sink,
            session_worker_source: sessions_source,
        }
    }

    async fn session_worker_fan_in(&mut self) {
        loop {
            if let Some(s) = self.session_worker_source.next().await {
                self.command_handler_sink.unbounded_send(s).unwrap()
            }
        }
    }
}
