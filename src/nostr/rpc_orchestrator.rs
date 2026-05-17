use super::rpc_machine::{NwcRpcMachine, RpcAction, RpcState};
use super::Event;
use crate::common::NwcError;
use super::RelayMessage;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RpcReceiveError {
    Timeout,
    ChannelClosed,
}

#[async_trait::async_trait(?Send)]
pub trait RpcContext {
    fn now(&self) -> u64;
    async fn allocate_connection_id(&self) -> u32;
    async fn execute_action(&self, conn_id: u32, action: RpcAction) -> Result<(), NwcError>;
    async fn receive_response(&self, conn_id: u32, remaining_secs: u64) -> Result<String, RpcReceiveError>;
    async fn disconnect(&self, conn_id: u32);
}

pub async fn execute_nwc_rpc<C: RpcContext>(ctx: &C, request: Event) -> Result<Event, NwcError> {
    const RPC_TIMEOUT_SECS: u64 = 10;

    let id = ctx.allocate_connection_id().await;
    let mut machine = NwcRpcMachine::new(request);

    for action in machine.start() {
        ctx.execute_action(id, action).await?;
    }

    let start = ctx.now();

    loop {
        let elapsed = ctx.now().saturating_sub(start);
        let remaining = RPC_TIMEOUT_SECS.saturating_sub(elapsed);
        if remaining == 0 {
            let action = machine.handle_timeout();
            let _ = ctx.execute_action(id, action).await;
            ctx.disconnect(id).await;
            return Err(NwcError::Timeout);
        }

        match ctx.receive_response(id, remaining).await {
            Ok(text) => {
                let msg = RelayMessage::from_json(&text)
                    .map_err(|e| NwcError::ProtocolError(format!("malformed relay response: {}", e)))?;

                if let Some(action) = machine.transition(msg) {
                    ctx.execute_action(id, action).await?;
                }

                match machine.state() {
                    RpcState::Success(event) => {
                        ctx.disconnect(id).await;
                        return Ok(event.clone());
                    }
                    RpcState::Failed(err) => {
                        ctx.disconnect(id).await;
                        return Err(NwcError::ProtocolError(err.clone()));
                    }
                    _ => continue,
                }
            }
            Err(RpcReceiveError::Timeout) => {
                let action = machine.handle_timeout();
                let _ = ctx.execute_action(id, action).await;
                ctx.disconnect(id).await;
                return Err(NwcError::Timeout);
            }
            Err(RpcReceiveError::ChannelClosed) => {
                ctx.disconnect(id).await;
                return Err(NwcError::ProtocolError("channel closed".into()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;

    struct MockRpcContext {
        now_val: Cell<u64>,
        next_id: Cell<u32>,
        actions: RefCell<Vec<(u32, RpcAction)>>,
        responses: RefCell<VecDeque<Result<String, RpcReceiveError>>>,
        disconnects: RefCell<Vec<u32>>,
    }

    impl MockRpcContext {
        fn new() -> Self {
            Self {
                now_val: Cell::new(1000),
                next_id: Cell::new(1),
                actions: RefCell::new(Vec::new()),
                responses: RefCell::new(VecDeque::new()),
                disconnects: RefCell::new(Vec::new()),
            }
        }

        #[allow(dead_code)]
    fn advance_time(&self, secs: u64) {
        self.now_val.set(self.now_val.get() + secs);
    }

        fn push_response(&self, resp: Result<String, RpcReceiveError>) {
            self.responses.borrow_mut().push_back(resp);
        }

        fn push_event_response(&self, sub_id: &str, event: Event) {
            let msg = RelayMessage::Event(sub_id.to_string(), event);
            self.push_response(Ok(msg.to_json()));
        }

        fn push_eose_response(&self, sub_id: &str) {
            let msg = RelayMessage::Eose(sub_id.to_string());
            self.push_response(Ok(msg.to_json()));
        }

        fn push_notice_response(&self, notice: &str) {
            let msg = RelayMessage::Notice(notice.to_string());
            self.push_response(Ok(msg.to_json()));
        }
    }

    #[async_trait::async_trait(?Send)]
    impl RpcContext for MockRpcContext {
        fn now(&self) -> u64 {
            self.now_val.get()
        }

        async fn allocate_connection_id(&self) -> u32 {
            let id = self.next_id.get();
            self.next_id.set(id + 1);
            id
        }

        async fn execute_action(&self, conn_id: u32, action: RpcAction) -> Result<(), NwcError> {
            self.actions.borrow_mut().push((conn_id, action));
            Ok(())
        }

        async fn receive_response(&self, _conn_id: u32, _remaining_secs: u64) -> Result<String, RpcReceiveError> {
            self.responses.borrow_mut().pop_front().unwrap_or(Err(RpcReceiveError::ChannelClosed))
        }

        async fn disconnect(&self, conn_id: u32) {
            self.disconnects.borrow_mut().push(conn_id);
        }
    }

    fn mock_request_event() -> Event {
        Event {
            id: "req1".to_string(),
            pubkey: "pk1".to_string(),
            created_at: 1000,
            kind: 23194,
            tags: vec![],
            content: "request".to_string(),
            sig: "sig1".to_string(),
        }
    }

    fn mock_response_event(request_id: &str) -> Event {
        Event {
            id: "resp1".to_string(),
            pubkey: "pk2".to_string(),
            created_at: 1001,
            kind: 23194,
            tags: vec![super::super::Tag::e(request_id)],
            content: "response".to_string(),
            sig: "sig2".to_string(),
        }
    }

    #[test]
    fn test_happy_path() {
        futures::executor::block_on(async {
            let ctx = MockRpcContext::new();
            let request = mock_request_event();
            let response = mock_response_event("req1");

            ctx.push_eose_response("rpc_sub");
            ctx.push_event_response("rpc_sub", response.clone());

            let result = execute_nwc_rpc(&ctx, request).await;
            assert!(result.is_ok(), "expected success, got {:?}", result);
            let event = result.unwrap();
            assert_eq!(event.id, "resp1");

            let disconnects = ctx.disconnects.borrow();
            assert_eq!(disconnects.len(), 1, "should disconnect once");
        });
    }

    #[test]
    fn test_timeout() {
        futures::executor::block_on(async {
            let ctx = MockRpcContext::new();
            let request = mock_request_event();

            ctx.push_response(Err(RpcReceiveError::Timeout));

            let result = execute_nwc_rpc(&ctx, request).await;
            assert!(matches!(result, Err(NwcError::Timeout)), "expected Timeout, got {:?}", result);

            let actions = ctx.actions.borrow();
            let unsub_actions: Vec<_> = actions.iter().filter(|(_, a)| matches!(a, RpcAction::Unsubscribe(_))).collect();
            assert_eq!(unsub_actions.len(), 1, "should have one unsubscribe on timeout");

            let disconnects = ctx.disconnects.borrow();
            assert_eq!(disconnects.len(), 1, "should disconnect once");
        });
    }

    #[test]
    fn test_channel_closed() {
        futures::executor::block_on(async {
            let ctx = MockRpcContext::new();
            let request = mock_request_event();

            ctx.push_response(Err(RpcReceiveError::ChannelClosed));

            let result = execute_nwc_rpc(&ctx, request).await;
            match result {
                Err(NwcError::ProtocolError(msg)) => assert!(msg.contains("channel closed")),
                other => panic!("expected ProtocolError with 'channel closed', got {:?}", other),
            }

            let disconnects = ctx.disconnects.borrow();
            assert_eq!(disconnects.len(), 1, "should disconnect once");
        });
    }

    #[test]
    fn test_relay_notice_causes_failure() {
        futures::executor::block_on(async {
            let ctx = MockRpcContext::new();
            let request = mock_request_event();

            ctx.push_notice_response("restricted");

            let result = execute_nwc_rpc(&ctx, request).await;
            match result {
                Err(NwcError::ProtocolError(msg)) => assert!(msg.contains("Relay notice")),
                other => panic!("expected ProtocolError with 'Relay notice', got {:?}", other),
            }

            let disconnects = ctx.disconnects.borrow();
            assert_eq!(disconnects.len(), 1, "should disconnect once");
        });
    }

    #[test]
    fn test_eose_ignored_then_success() {
        futures::executor::block_on(async {
            let ctx = MockRpcContext::new();
            let request = mock_request_event();
            let response = mock_response_event("req1");

            ctx.push_eose_response("rpc_sub");
            ctx.push_eose_response("rpc_sub");
            ctx.push_event_response("rpc_sub", response.clone());

            let result = execute_nwc_rpc(&ctx, request).await;
            assert!(result.is_ok(), "expected success after EOSE noise, got {:?}", result);

            let disconnects = ctx.disconnects.borrow();
            assert_eq!(disconnects.len(), 1, "should disconnect once");
        });
    }

    #[test]
    fn test_start_actions_recorded() {
        futures::executor::block_on(async {
            let ctx = MockRpcContext::new();
            let request = mock_request_event();
            let response = mock_response_event("req1");

            ctx.push_event_response("rpc_sub", response);

            let _ = execute_nwc_rpc(&ctx, request).await;

            let actions = ctx.actions.borrow();
            assert!(actions.len() >= 2, "should have at least 2 start actions");

            let first_action = &actions[0];
            assert_eq!(first_action.0, 1, "connection id should be 1");
            assert!(matches!(first_action.1, RpcAction::Subscribe(_, _)), "first action should be Subscribe");

            let second_action = &actions[1];
            assert!(matches!(second_action.1, RpcAction::Publish(_)), "second action should be Publish");
        });
    }
}