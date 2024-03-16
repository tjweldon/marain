use std::collections::VecDeque;

use chrono::Utc;
use futures_channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use futures_util::stream::SplitStream;
use futures_util::{stream::SplitSink, SinkExt, StreamExt};
use marain_api::prelude::{ClientMsg, ClientMsgBody, Timestamp};
use sphinx::prelude::cbc_decode;
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};

use super::user::User;

use super::app::Room;
use super::commands::{Command, CommandPayload};
use super::events::Event;
use super::message_builder::SocketSendAdaptor;
use anyhow::{anyhow, Result};

struct SessionBus {
    app_gateway_sink: UnboundedSender<Command>,
    event_sink: Option<UnboundedSender<Event>>,
    event_source: UnboundedReceiver<Event>,
}

impl SessionBus {
    fn new(gateway_sink: UnboundedSender<Command>) -> Self {
        let (sink, src) = unbounded();
        Self {
            app_gateway_sink: gateway_sink,
            event_sink: Some(sink),
            event_source: src,
        }
    }

    async fn next_event(&mut self) -> Option<Event> {
        self.event_source.next().await
    }

    fn send_command(&mut self, command: Command) {
        self.app_gateway_sink.unbounded_send(command).unwrap()
    }
}

pub struct SessionWorker {
    user: User,
    app_socket: SessionBus,
    user_sink: SplitSink<WebSocketStream<TcpStream>, Message>,
    user_source: SplitStream<WebSocketStream<TcpStream>>,
    staged_messages: VecDeque<ClientMsg>,
    shared_secret: [u8; 32],
}

impl SessionWorker {
    pub fn new(
        user: User,
        gateway_sink: UnboundedSender<Command>,
        user_sink: SplitSink<WebSocketStream<TcpStream>, Message>,
        user_source: SplitStream<WebSocketStream<TcpStream>>,
    ) -> Self {
        SessionWorker {
            user: user.clone(),
            app_socket: SessionBus::new(gateway_sink),
            user_sink,
            user_source,
            staged_messages: VecDeque::new(),
            shared_secret: user.shared_secret.clone(),
        }
    }

    fn give_sink(&mut self) -> Result<UnboundedSender<Event>> {
        if let Some(s) = self.app_socket.event_sink.clone() {
            self.app_socket.event_sink = None;
            Ok(s)
        } else {
            Err(anyhow!(
                "Failure in SessionWorker.give_sinks(). One of the sinks is not present.",
            ))
        }
    }

    fn decrypt(user_key: &[u8; 32], enc: Vec<u8>) -> Result<Vec<u8>> {
        match cbc_decode(user_key.to_vec(), enc) {
            Ok(dec) => Ok(dec),
            Err(e) => {
                log::error!("Failed to decode user message with error: {e}");
                Err(anyhow!("{e:?}"))
            }
        }
    }

    fn deserialize(msg: Vec<u8>) -> Result<ClientMsg, Box<bincode::ErrorKind>> {
        bincode::deserialize::<ClientMsg>(&msg[..])
    }

    fn parse_command(&mut self, msg: ClientMsg) -> Result<Command> {
        match msg {
            ClientMsg { body, .. } => match body {
                // ClientMsgBody::Login(name, client_public_key) => match self.give_sink() {
                //     Ok(event_sink) => Ok(Command {
                //         user: self.user.clone(),
                //         payload: CommandPayload::RegisterUser(event_sink, client_public_key),
                //     }),
                //     Err(e) => {
                //         return Err(anyhow!("Error: {e:?} in parse_command"));
                //     }
                // },
                ClientMsgBody::SendToRoom { contents: message } => Ok(Command {
                    user: self.user.clone(),
                    payload: CommandPayload::RecordMessage {
                        message,
                    },
                }),
                ClientMsgBody::Move { target } => Ok(Command {
                    user: self.user.clone(),
                    payload: CommandPayload::MoveUser {
                        target_room: Room { name: target },
                    },
                }),
                ClientMsgBody::GetTime => Ok(Command {
                    user: self.user.clone(),
                    payload: CommandPayload::Time(Timestamp::from(Utc::now())),
                }),
                _ => {
                    return Err(anyhow!("Cannot parse command. Command: {body:?}"));
                }
            },
        }
    }

    async fn handle_command(&mut self, msg: ClientMsg) -> Result<()> {
        match self.parse_command(msg) {
            Ok(cmd) => match cmd.payload {
                CommandPayload::Time(t) => {
                    let ts = SocketSendAdaptor::prepare_send_time(&self.shared_secret, t)?;
                    self.user_sink.send(ts).await?;
                    Ok(())
                }
                _ => {
                    self.app_socket.send_command(cmd);
                    Ok(())
                }
            },
            Err(e) => return Err(anyhow!("Error in handle_command: {e:?}")),
        }
    }

    async fn handle_event(&mut self, event: Event) -> Result<()> {
        match event {
            Event::UserRegistered { token } => {
                log::info!("Successfully registered User: {token}");
                Ok(())
            }
            Event::MsgReceived { msg } => {
                let msg =
                    SocketSendAdaptor::prepare_send_msg_log(msg, &self.user, &self.shared_secret)?;
                self.user_sink.send(msg).await?;
                Ok(())
            }
            Event::UserLeft { user, room } => {
                let m = format!("{} left {}.", user.name, room.name)
                    .as_bytes()
                    .to_vec();
                self.user_sink.send(Message::Binary(m)).await?;
                Ok(())
            }
            Event::UserJoined { user, room } => {
                let m = format!("{} joined {}.", user.name, room.name)
                    .as_bytes()
                    .to_vec();
                self.user_sink.send(Message::Binary(m)).await?;
                Ok(())
            }
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        let event_sink = self.give_sink()?;
        let register = Command {
            user: self.user.clone(),
            payload: CommandPayload::RegisterUser(event_sink),
        };
        
        self.app_socket.send_command(register);
        
        loop {
            tokio::select! {
                Some(msg) = self.user_source.next() => {
                    let msg_bytes = match msg {
                        Ok(Message::Binary(data)) => data,
                        Err(e) => {
                            log::error!("Invalid protocol: {e}");
                            continue;
                        },
                        Ok(Message::Close {..}) => {
                            return Ok(());
                        },
                        _ => {
                            log::warn!("Unexpected message encoding");
                            continue;
                        }
                    };

                    let decrypted = match SessionWorker::decrypt(&self.shared_secret, msg_bytes) {
                        Ok(data) => data,
                        Err(e) => {
                            log::error!("decryption error: {e}");
                            continue;
                        }
                    };

                    let deserialized = match SessionWorker::deserialize(decrypted) {
                        Ok(data) => data,
                        Err(e) => {
                            log::error!("deserialization error: {e}");
                            continue;
                        }
                    };

                    match self.handle_command(deserialized).await {
                        Err(e) => log::error!("Failed to push user message downstream: {e}"),
                        _ => {}
                    };
                }

                Some(event) = self.app_socket.next_event() => {
                    match self.handle_event(event).await {
                        Ok(_) => {},
                        Err(e) => log::error!("Error in SessionWorker event handler. Error: {e:?}")
                    }
                }
            }
        }
    }
}
