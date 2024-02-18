use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};

use tokio_tungstenite::tungstenite::Message;

use super::types::LockedPeerMap;

pub struct Room {
    pub occupants: LockedPeerMap,
    pub message_bus: Arc<Mutex<VecDeque<Message>>>,
}

impl Room {
    pub fn new(occupants: LockedPeerMap, message_bus: Arc<Mutex<VecDeque<Message>>>) -> Self {
        Room {
            occupants,
            message_bus,
        }
    }

    pub fn new_message(&mut self, msg: Message) {
        self.message_bus.lock().unwrap().push_back(msg);
    }

    pub fn remove_oldest_message(&mut self) {
        self.message_bus.lock().unwrap().pop_front();
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
