use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use futures_channel::mpsc::UnboundedSender;
use tokio_tungstenite::tungstenite::Message;

use super::{room::Room, user::User};

pub type PeerMap = HashMap<String, (Arc<Mutex<User>>, UnboundedSender<Message>)>;
pub type RoomMap = HashMap<u64, Room>;
pub type LockedPeerMap = Arc<Mutex<HashMap<String, (Arc<Mutex<User>>, UnboundedSender<Message>)>>>;
pub type LockedRoomMap = Arc<Mutex<HashMap<u64, Room>>>;
