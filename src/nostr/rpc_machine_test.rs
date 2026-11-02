use super::*;

fn mock_event(id: &str, pubkey: &str) -> Event {
    Event {
        id: id.to_string(),
        pubkey: pubkey.to_string(),
        created_at: 0,
        kind: 23194,
        tags: vec![],
        content: String::new(),
        sig: String::new(),
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

#[test]
fn test_rpc_machine_notice_transition() {
    let req = mock_event("req1", "pk1");
    let mut machine = NwcRpcMachine::new(req);
    machine.start();

    let action = machine.transition(RelayMessage::Notice("rate limited".into()));
    assert_eq!(action, Some(RpcAction::Unsubscribe("rpc_sub".into())));
    assert_eq!(
        machine.state,
        RpcState::Failed("Relay notice: rate limited".into())
    );
}

#[test]
fn test_rpc_machine_state_accessor() {
    let mut machine = NwcRpcMachine::new(mock_event("req1", "pk1"));
    assert_eq!(machine.state(), &RpcState::Initial);

    machine.start();
    assert_eq!(machine.state(), &RpcState::AwaitingResponse);

    machine.transition(RelayMessage::Notice("nope".into()));
    assert!(matches!(machine.state(), RpcState::Failed(_)));
}

#[test]
fn test_rpc_machine_start_scopes_filter_to_request() {
    let req = mock_event("req1", "pk1");
    let mut machine = NwcRpcMachine::new(req);

    let actions = machine.start();
    match &actions[0] {
        RpcAction::Subscribe(sub_id, filter) => {
            assert_eq!(sub_id, "rpc_sub");
            // The response must be correlated by both the `e` tag (request id)
            // and the `p` tag (wallet pubkey); dropping either widens the
            // subscription to unrelated traffic.
            assert_eq!(filter.e_tags, Some(vec!["req1".to_string()]));
            assert_eq!(filter.p_tags, Some(vec!["pk1".to_string()]));
        }
        other => panic!("expected Subscribe, got {other:?}"),
    }
    match &actions[1] {
        RpcAction::Publish(event) => assert_eq!(event.id, "req1"),
        other => panic!("expected Publish, got {other:?}"),
    }
}
