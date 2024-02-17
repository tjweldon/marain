use std::sync::{Arc, Mutex};

use futures_channel::mpsc::UnboundedReceiver;
use futures_util::{stream::SplitSink, SinkExt, StreamExt};
use log::error;
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};

use crate::domain::{types::LockedRoomMap, user::User};

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
                if broadcast_msg_from_usr == None {
                    break
                }
                let rooms = room_map.lock().unwrap();
                let bus_max = rooms.get(&user.lock().unwrap().room).unwrap().bus_max;
                let mut msg_bus = rooms.get(&user.lock().unwrap().room).unwrap().message_bus.lock().unwrap();
                let mut msg_with_sender_prefix = user.lock().unwrap().name.to_string();
                msg_with_sender_prefix += broadcast_msg_from_usr.clone().unwrap().to_text().unwrap();

                msg_bus.push_back(Message::Text(msg_with_sender_prefix.clone()));
                if msg_bus.len() > bus_max {
                    msg_bus.pop_front();
                }
                let occupants = rooms.get(&user.lock().unwrap().room).unwrap().occupants.lock().unwrap();
                let receipients = occupants
                    .iter()
                    .filter_map(|(mapped_user_id, (_mapped_user, channel))| if mapped_user_id != &user.lock().unwrap().id { Some(channel) } else { None });

                for receipient in receipients {
                    receipient.unbounded_send(Message::Text(msg_with_sender_prefix.clone())).unwrap_or_else(|e| error!("{}", e))
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
