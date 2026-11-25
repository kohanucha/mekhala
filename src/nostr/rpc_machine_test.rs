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

/// A request signed by `client` and addressed to `wallet`, the way
/// `create_request_event` builds one.
fn mock_request(id: &str, client: &str, wallet: &str) -> Event {
    let mut event = mock_event(id, client);
    event.tags = vec![crate::nostr::Tag::p(wallet)];
    event
}

/// A reply signed by `author` that references `request_id` in an `e` tag.
fn mock_reply(id: &str, author: &str, request_id: &str) -> Event {
    let mut event = mock_event(id, author);
    event.kind = 23195;
    event.tags = vec![crate::nostr::Tag::E(request_id.to_string(), vec![])];
    event
}

#[test]
fn test_rpc_machine_flow() {
    let req = mock_request("req1", "client", "wallet");
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
    let resp = mock_reply("resp1", "wallet", "req1");
    let action = machine.transition(RelayMessage::Event("rpc_sub".into(), resp.clone()));

    assert_eq!(action, Some(RpcAction::Unsubscribe("rpc_sub".into())));
    assert_eq!(machine.state, RpcState::Success(resp));
}

#[test]
fn test_rpc_machine_unmatched_response() {
    let req = mock_request("req1", "client", "wallet");
    let mut machine = NwcRpcMachine::new(req);
    machine.start();

    // Response with wrong e-tag should not match
    let resp = mock_reply("resp1", "wallet", "wrong_req");
    let action = machine.transition(RelayMessage::Event("rpc_sub".into(), resp));
    assert!(action.is_none());
    assert_eq!(machine.state, RpcState::AwaitingResponse);
}

#[test]
fn test_rpc_machine_ignores_a_reply_from_another_author() {
    let req = mock_request("req1", "client", "wallet");
    let mut machine = NwcRpcMachine::new(req);
    machine.start();

    // The request is published to the relay, so its id is public: anyone can
    // publish an event that names it. A reply is only a reply when the wallet
    // the request was addressed to signed it, so a forged one is ignored and
    // the round trip keeps waiting instead of ending on it.
    let forged = mock_reply("forged", "mallory", "req1");
    let action = machine.transition(RelayMessage::Event("rpc_sub".into(), forged));
    assert!(action.is_none());
    assert_eq!(machine.state, RpcState::AwaitingResponse);

    // The real answer still lands.
    let real = mock_reply("real", "wallet", "req1");
    let action = machine.transition(RelayMessage::Event("rpc_sub".into(), real.clone()));
    assert_eq!(action, Some(RpcAction::Unsubscribe("rpc_sub".into())));
    assert_eq!(machine.state, RpcState::Success(real));
}

#[test]
fn test_rpc_machine_without_a_recipient_matches_no_reply() {
    // A request with no `p` tag is addressed to nobody, so nothing can answer
    // it. The machine must not fall back to accepting the first event that
    // happens to name the request id.
    let mut machine = NwcRpcMachine::new(mock_event("req1", "client"));
    let actions = machine.start();
    match &actions[0] {
        RpcAction::Subscribe(_, filter) => assert_eq!(filter.authors, None),
        other => panic!("expected Subscribe, got {other:?}"),
    }

    let reply = mock_reply("resp1", "wallet", "req1");
    let action = machine.transition(RelayMessage::Event("rpc_sub".into(), reply));
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
    let req = mock_request("req1", "client", "wallet1");
    let mut machine = NwcRpcMachine::new(req);

    let actions = machine.start();
    match &actions[0] {
        RpcAction::Subscribe(sub_id, filter) => {
            assert_eq!(sub_id, "rpc_sub");
            // The reply has to reference the request (`e`), be addressed to
            // the client (`p`), and be signed by the wallet the request went
            // to (`authors`). Dropping any of the three widens the
            // subscription to traffic that cannot be the answer.
            assert_eq!(filter.e_tags, Some(vec!["req1".to_string()]));
            assert_eq!(filter.p_tags, Some(vec!["client".to_string()]));
            assert_eq!(filter.authors, Some(vec!["wallet1".to_string()]));
        }
        other => panic!("expected Subscribe, got {other:?}"),
    }
    match &actions[1] {
        RpcAction::Publish(event) => assert_eq!(event.id, "req1"),
        other => panic!("expected Publish, got {other:?}"),
    }
}
