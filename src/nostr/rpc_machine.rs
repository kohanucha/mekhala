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
mod tests {
    use super::*;

    fn mock_event(id: &str, pubkey: &str) -> Event {
        Event {
            id: id.to_string(),
            pubkey: pubkey.to_string(),
            created_at: 0,
            kind: 23194,
            tags: vec![],
            content: "".to_string(),
            sig: "".to_string(),
        }
    }

    #[test]
    fn test_rpc_machine_flow() {
        let req = mock_event("req1", "pk1");
        let mut machine = NwcRpcMachine::new(req);

        let actions = machine.start();
        assert_eq!(actions.len(), 2);
        assert!(matches!(actions[0], RpcAction::Subscribe(_, _)));
        assert!(matches!(actions[1], RpcAction::Publish(_)));
        assert_eq!(machine.state, RpcState::AwaitingResponse);

        // Feed EOSE - should be ignored
        let action = machine.transition(RelayMessage::Eose("rpc_sub".into()));
        assert!(action.is_none());
        assert_eq!(machine.state, RpcState::AwaitingResponse);

        // Feed Response EVENT
        let mut resp = mock_event("resp1", "pk2");
        resp.tags = vec![crate::nostr::Tag::E("req1".to_string(), vec![])];
        let action = machine.transition(RelayMessage::Event("rpc_sub".into(), resp.clone()));

        assert_eq!(action, Some(RpcAction::Unsubscribe("rpc_sub".into())));
        assert_eq!(machine.state, RpcState::Success(resp));
    }

    #[test]
    fn test_rpc_machine_unmatched_response() {
        let req = mock_event("req1", "pk1");
        let mut machine = NwcRpcMachine::new(req);
        machine.start();

        // Response with wrong e-tag should not match
        let mut resp = mock_event("resp1", "pk2");
        resp.tags = vec![crate::nostr::Tag::E("wrong_req".to_string(), vec![])];
        let action = machine.transition(RelayMessage::Event("rpc_sub".into(), resp));
        assert!(action.is_none());
        assert_eq!(machine.state, RpcState::AwaitingResponse);
    }

    #[test]
    fn test_rpc_machine_transition_before_start() {
        let req = mock_event("req1", "pk1");
        let mut machine = NwcRpcMachine::new(req);

        // Machine is still in Initial state. Any transition should be ignored.
        let action = machine.transition(RelayMessage::Eose("rpc_sub".into()));
        assert!(action.is_none());
        assert_eq!(machine.state, RpcState::Initial);
    }
}
