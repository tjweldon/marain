use futures_channel::mpsc::UnboundedSender;
use marain_api::prelude::Timestamp;

use super::{events::Event, room::Room, user::User};

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
    RecordMessage { message: String },
    GetRecipients,
    Time(Timestamp),
}
