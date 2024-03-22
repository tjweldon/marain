use chrono::Utc;
use futures_channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use futures_util::stream::SplitStream;
use futures_util::{stream::SplitSink, SinkExt, StreamExt};
use marain_api::prelude::{ClientMsg, ClientMsgBody, Timestamp};
use sphinx::prelude::cbc_decode;
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};

use crate::domain::commands::{Command, CommandPayload};
use crate::domain::events::Event;
use crate::domain::room::Room;
use crate::domain::user::User;

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

    fn parse_client_msg(&mut self, msg: ClientMsg) -> Result<Command> {
        match msg {
            ClientMsg { body, .. } => match body {
                ClientMsgBody::SendToRoom { contents: message } => Ok(Command {
                    user: self.user.clone(),
                    payload: CommandPayload::RecordMessage { message },
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

    async fn handle_client_msg(&mut self, msg: ClientMsg) -> Result<()> {
        match self.parse_client_msg(msg) {
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
            Event::UserLeft {
                room,
                occupant_names,
                notifications,
                msg_log,
                ..
            } => {
                let msg = SocketSendAdaptor::room_data_response(
                    &self.shared_secret,
                    msg_log,
                    notifications,
                    occupant_names,
                    &room,
                )?;
                self.user_sink.send(msg).await?;

                Ok(())
            }
            Event::UserJoined {
                msg_log,
                notifications,
                occupant_names,
                room,
                ..
            } => {
                let msg = SocketSendAdaptor::room_data_response(
                    &self.shared_secret,
                    msg_log,
                    notifications,
                    occupant_names,
                    &room,
                )?;
                self.user_sink.send(msg).await?;
                // let msg =
                //     SocketSendAdaptor::user_join_notification(&self.shared_secret, &user, &room)?;
                // self.user_sink.send(msg).await?;
                Ok(())
            }
        }
    }

    pub async fn end_session(&mut self) {
        self.app_socket.send_command(Command {
            user: self.user.clone(),
            payload: CommandPayload::DropUser,
        });
        loop {
            match self.app_socket.next_event().await {
                Some(Event::UserLeft { user, .. }) if user == self.user => {
                    return;
                }
                _ => {
                    continue;
                }
            };
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        let event_sink = self.give_sink()?;
        let register = Command {
            user: self.user.clone(),
            payload: CommandPayload::RegisterUser(event_sink),
        };

        self.app_socket.send_command(register);

        'main_loop: loop {
            tokio::select! {
                Some(msg) = self.user_source.next() => {
                    let msg_bytes = match msg {
                        Ok(Message::Binary(data)) => data,
                        Err(e) => {
                            log::error!("Invalid protocol, ending session. Error: {e}");
                            break 'main_loop;
                        },
                        Ok(Message::Close {..}) => {
                            break 'main_loop;
                        },
                        _ => {
                            log::warn!("Unhandled message: {msg:?}");
                            continue;
                        }
                    };

                    let decrypted = match SessionWorker::decrypt(&self.shared_secret, msg_bytes) {
                        Ok(data) => data,
                        Err(e) => {
                            log::error!("Decryption error, ending session. Error: {e}");
                            break 'main_loop;
                        }
                    };

                    let deserialized = match SessionWorker::deserialize(decrypted) {
                        Ok(data) => data,
                        Err(e) => {
                            log::error!("Deserialization error: {e}");
                            continue;
                        }
                    };

                    match self.handle_client_msg(deserialized).await {
                        Err(e) => {
                            log::error!("Failed to push user message downstream, exiting user session. Error: {e}");
                            break 'main_loop;
                        },
                        _ => {}
                    };
                }

                Some(event) = self.app_socket.next_event() => {
                    match self.handle_event(event).await {
                        Ok(_) => {},
                        Err(e) => {
                            log::warn!("Error in SessionWorker event handler. Error: {e:?}");
                            break 'main_loop;
                        }
                    }
                }
            }
        }
        self.end_session().await;
        return Ok(());
    }
}
