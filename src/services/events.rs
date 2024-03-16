use super::{app::Room, user::User, chat_log::MessageLog};

#[derive(Clone)]
pub enum Event {
    UserRegistered { token: String },
    UserJoined { user: User, room: Room },
    UserLeft { user: User, room: Room },
    MsgReceived { msg: MessageLog },
}
