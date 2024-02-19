use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};

use futures_channel::mpsc::UnboundedSender;
use tokio_tungstenite::tungstenite::Message;

use super::{chat_log::MessageLog, types::LockedPeerMap};

const LOG_MAX_ENTRIES: usize = 25;

pub struct Room {
    pub occupants: LockedPeerMap,
    pub chat_log: Arc<Mutex<VecDeque<MessageLog>>>,
}

impl Room {
    pub fn new(occupants: LockedPeerMap, chat_log: Arc<Mutex<VecDeque<MessageLog>>>) -> Self {
        Room {
            occupants,
            chat_log,
        }
    }

    pub fn new_message(&mut self, msg: MessageLog) {
        match self.chat_log.lock() {
            Err(_) => log::error!("Could not acquire lock on room chat log to push new message"),
            Ok(mut chat_log) => {
                chat_log.push_back(msg);
            }
        }
    }

    pub fn remove_oldest_message(&mut self) {
        if let Ok(mut chat_log) = self.chat_log.lock() {
            if chat_log.len() > LOG_MAX_ENTRIES {
                chat_log.pop_front();
            }
        };
    }

    pub fn get_recipients_except(&self, user_id: &str) -> Vec<UnboundedSender<Message>> {
        let occupants = self.occupants.lock().unwrap();
        occupants
            .iter()
            .filter_map(|(candidate_id, (_mapped_user, channel))| {
                if user_id != candidate_id {
                    Some(channel.clone())
                } else {
                    None
                }
            })
            .collect()
    }
}

impl Default for Room {
    fn default() -> Self {
        Room::new(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(VecDeque::new())),
        )
    }
}
