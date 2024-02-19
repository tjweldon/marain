use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use futures_channel::mpsc::UnboundedSender;
use marain_api::prelude::ServerMsg;

use super::{room::Room, user::User};

pub type PeerMap = HashMap<String, (Arc<Mutex<User>>, UnboundedSender<ServerMsg>)>;
pub type RoomMap = HashMap<u64, Room>;
pub type LockedPeerMap =
    Arc<Mutex<HashMap<String, (Arc<Mutex<User>>, UnboundedSender<ServerMsg>)>>>;
pub type LockedRoomMap = Arc<Mutex<HashMap<u64, Room>>>;
