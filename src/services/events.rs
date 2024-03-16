use super::app::Room;
use crate::domain::{chat_log::MessageLog, user::User};

#[derive(Clone)]
pub enum Event {
    UserRegistered { token: String },
    UserJoined { user: User, room: Room },
    UserLeft { user: User, room: Room },
    MsgReceived { msg: MessageLog },
}
