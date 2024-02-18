use std::sync::{Arc, Mutex};

use futures_channel::mpsc::UnboundedReceiver;
use futures_util::{stream::SplitSink, SinkExt, StreamExt};
use log::error;
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};

use crate::domain::{types::LockedRoomMap, user::User};

const BUS_MAX: usize = 25;

pub async fn global_message_handler(
    mut ws_sink: SplitSink<WebSocketStream<TcpStream>, Message>,
    mut message: UnboundedReceiver<Message>,
    room_map: LockedRoomMap,
    user: Arc<Mutex<User>>,
    mut user_inbox: UnboundedReceiver<Message>,
) {
    loop {
        tokio::select! {
            broadcast_msg_from_usr = message.next() => {
                let Some(msg) = broadcast_msg_from_usr else {
                    break
                };
                if !msg.is_text() {
                    break;
                }
                let rooms = room_map.lock().unwrap();
                let user_room = user.lock().unwrap().room;
                let mut msg_bus = rooms.get(&user_room).unwrap().message_bus.lock().unwrap();

                msg_bus.push_back(msg.clone());
                if msg_bus.len() > BUS_MAX {
                    msg_bus.pop_front();
                }
                let occupants = rooms.get(&user_room).unwrap().occupants.lock().unwrap();
                let receipients = occupants
                    .iter()
                    .filter_map(|(mapped_user_id, (_mapped_user, channel))| {
                        if mapped_user_id != &user.lock().unwrap().id {
                            Some(channel)
                        } else {
                            None
                        }
                    });

                for receipient in receipients {
                    log::info!("sending message {} to somebody i dunno", msg.to_string());
                    receipient
                        .unbounded_send(msg.clone())
                        .unwrap_or_else(|e| error!("{}", e))
                }
            }

            broacst_msg_to_usr = user_inbox.next() => {
                match broacst_msg_to_usr.clone() {
                    Some(m) => ws_sink.send(m).await.unwrap_or_else(|e| error!("{}", e)),
                    None => {}
                }
            }
        }
    }
}
