use crate::nostr::{Event, Filter, RelayMessage};

/// Result of one NWC RPC round trip through the relay.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum RpcState {
    /// Not started yet.
    Initial,
    /// Request published; waiting for the wallet's response event.
    AwaitingResponse,
    /// Response received and correlated with the request.
    Success(Event),
    /// The relay reported an error (e.g. a `NOTICE`).
    Failed(String),
}

/// An operation the caller must perform on the relay connection.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum RpcAction {
    /// Open the response subscription (`sub_id`, filter).
    Subscribe(String, Filter),
    /// Publish the given event.
    Publish(Event),
    /// Close the response subscription.
    Unsubscribe(String),
}

/// Drives a single NIP-47 request through the relay: opens a subscription
/// for the response, publishes the request, and correlates the reply by the
/// `e` tag pointing back at the request event.
pub struct NwcRpcMachine {
    request: Event,
    state: RpcState,
    sub_id: String,
}

impl NwcRpcMachine {
    /// Creates a machine for `request` in the [`RpcState::Initial`] state.
    pub fn new(request: Event) -> Self {
        Self {
            request,
            state: RpcState::Initial,
            sub_id: "rpc_sub".to_string(),
        }
    }

    /// Current state of the round trip.
    pub fn state(&self) -> &RpcState {
        &self.state
    }

    /// Enters [`RpcState::AwaitingResponse`] and returns the actions to run
    /// in order: subscribe to the response filter, then publish the request.
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

    /// Feeds a relay message into the machine: `Some(action)` when the
    /// caller must act (unsubscribe after success or a relay `NOTICE`),
    /// `None` for noise (`EOSE`) or events that do not reference the request.
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
