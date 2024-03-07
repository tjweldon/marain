use std::sync::{Arc, Mutex};

use futures_channel::mpsc::UnboundedSender;
use futures_util::{future, stream::SplitStream, StreamExt};
use log;
use marain_api::prelude::*;
use sphinx::prelude::cbc_decode;
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};

use crate::domain::{room::Room, types::LockedRoomMap, user::User};

use super::commands::Commands;

const INIT_VEC: u64 = 0x00000000_00000000;

fn decrypt(user_key: &[u8; 32], enc: Vec<u8>) -> Option<Vec<u8>> {
    match cbc_decode(user_key.to_vec(), enc, INIT_VEC) {
        Ok(dec) => Some(dec),
        Err(e) => {
            log::error!("Failed to decode user message with error: {e}");
            None
        }
    }
}

fn deserialize(msg: Vec<u8>) -> Option<ClientMsg> {
    match bincode::deserialize::<ClientMsg>(&msg[..]) {
        Ok(cm) => Some(cm),
        Err(e) => {
            log::warn!("Unrecognised message from client: {e}");
            None
        }
    }
}

pub async fn recv_routing_handler(
    ws_source: SplitStream<WebSocketStream<TcpStream>>,
    user: Arc<Mutex<User>>,
    command_pipe: UnboundedSender<Commands>,
    message_pipe: UnboundedSender<ClientMsg>,
    room_map: LockedRoomMap,
) {
    let user_key = user.lock().unwrap().shared_secret;
    _ = ws_source
        .for_each(|msg_maybe| {
            match msg_maybe {
                Ok(Message::Binary(msg_bytes)) => {
                    // can fail if decrypt returns error
                    let Some(decoded) = decrypt(&user_key, msg_bytes) else {
                        return future::ready(());
                    };

                    // can fail if deserialization fails
                    let Some(usr_msg) = deserialize(decoded) else {
                        return future::ready(());
                    };

                    // no failure modes
                    match usr_msg {
                        ClientMsg {
                            token: Some(_),
                            body: ClientMsgBody::SendToRoom { .. },
                            ..
                        } => {
                            message_pipe.unbounded_send(usr_msg).unwrap();
                            log::info!("published chat message")
                        }
                        ClientMsg {
                            token: Some(_),
                            body: ClientMsgBody::GetTime,
                            ..
                        } => {
                            command_pipe.unbounded_send(Commands::GetTime).unwrap();
                            log::info!("Pushed Time command to handler")
                        }
                        ClientMsg {
                            token: Some(id),
                            body: ClientMsgBody::Move { target },
                            ..
                        } => {
                            command_pipe
                                .unbounded_send(Commands::Move {
                                    user_id: id,
                                    target,
                                })
                                .unwrap();

                            log::info!("Pushed move command to handler");
                        }

                        _ => {}
                    }
                }

                // close the connection
                Ok(Message::Close(..)) => {
                    remove_user(room_map.clone(), user.clone());
                }

                // unhandled message formats
                Ok(Message::Text(..)) | Ok(Message::Ping(..)) | Ok(Message::Pong(..)) | Ok(Message::Frame(..)) => {
                    log::warn!("Received unhandled message format: Mesage::Text | Message::Ping | Message::Pong | Message::Frame")
                }

                // upstream connection closed
                Err(e) => {
                    remove_user(room_map.clone(), user.clone());
                    log::warn!("Disconnected user due to upstream error: {e}");
                }
            }

            future::ready(())
        })
        .await;
}

fn remove_user(
    room_map: Arc<Mutex<std::collections::HashMap<u64, crate::domain::room::Room>>>,
    user: Arc<Mutex<User>>,
) {
    let rooms = room_map.lock().unwrap();
    let empty = Room::default();
    let mut members = rooms
        .get(&user.lock().unwrap().room)
        .unwrap_or(&empty)
        .occupants
        .lock()
        .expect("Something else broke. ‾\\(`>`)/‾");
    members.remove(&user.lock().unwrap().id);
}
