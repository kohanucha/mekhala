use crate::nostr::{Event, Filter, RelayMessage};

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum RpcState {
    Initial,
    AwaitingResponse,
    Success(Event),
    Failed(String),
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum RpcAction {
    Subscribe(String, Filter),
    Publish(Event),
    Unsubscribe(String),
}

pub struct NwcRpcMachine {
    request: Event,
    state: RpcState,
    sub_id: String,
}

impl NwcRpcMachine {
    pub fn new(request: Event) -> Self {
        Self {
            request,
            state: RpcState::Initial,
            sub_id: "rpc_sub".to_string(),
        }
    }

    pub fn state(&self) -> &RpcState {
        &self.state
    }

    pub fn start(&mut self) -> Vec<RpcAction> {
        let filter = Filter {
            e_tags: Some(vec![self.request.id.clone()]),
            p_tags: Some(vec![self.request.pubkey.clone()]),
            ..Filter::default()
        };

        self.state = RpcState::AwaitingResponse;

        vec![
            RpcAction::Subscribe(self.sub_id.clone(), filter),
            RpcAction::Publish(self.request.clone()),
        ]
    }

    pub fn transition(&mut self, message: RelayMessage) -> Option<RpcAction> {
        match (&self.state, message) {
            (RpcState::AwaitingResponse, RelayMessage::Event(_, event)) => {
                // Correlation check: ensure this event references our request
                // In NWC, response should have an 'e' tag pointing to request ID
                let references_request = event.tags.iter().any(|t| {
                    matches!(t, crate::nostr::Tag::E(eid, _) if eid == &self.request.id)
                });

                if references_request {
                    self.state = RpcState::Success(event);
                    Some(RpcAction::Unsubscribe(self.sub_id.clone()))
                } else {
                    None
                }
            }
            (RpcState::AwaitingResponse, RelayMessage::Eose(_)) => {
                // Protocol noise, stay in current state
                None
            }
            (RpcState::AwaitingResponse, RelayMessage::Notice(msg)) => {
                self.state = RpcState::Failed(format!("Relay notice: {}", msg));
                Some(RpcAction::Unsubscribe(self.sub_id.clone()))
            }
            _ => None,
        }
    }

}

#[cfg(test)]
#[path = "rpc_machine_test.rs"]
mod rpc_machine_test;
