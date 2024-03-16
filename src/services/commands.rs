use futures_channel::mpsc::UnboundedSender;
use marain_api::prelude::{ServerMsg, Timestamp};

use crate::domain::user::User;

use super::{app::Room, events::Event};

#[derive(Debug, Clone)]
pub struct Command {
    pub user: User,
    pub payload: CommandPayload,
}

#[derive(Debug, Clone)]
pub enum CommandPayload {
    RegisterUser(UnboundedSender<Event>),
    DropUser,
    MoveUser { target_room: Room },
    RecordMessage { target_room: Room, message: String },
    GetRecipients,
    Time(Timestamp),
}
