use std::rc::Rc;
use std::time::Duration;
use futures::channel::oneshot;
use futures::lock::Mutex;
use futures_util::FutureExt;
use worker::*;
use crate::cloudflare::connection::CloudflareConnectionTransport;
use crate::cloudflare::id_allocator::{DoIdBatchStore, IdAllocator};
use crate::cloudflare::storage::CloudflareStorage;
use crate::nostr::connection::ConnectionManager;
use crate::nostr::engine::NostrEngine;
use crate::nostr::rpc_machine::RpcAction;
use crate::nostr::rpc_orchestrator::{RpcContext, RpcReceiveError};

/// Production adapter for `RpcContext` that coordinates engine and manager
/// to execute NWC RPC actions against Cloudflare's Durable Object runtime.
pub(crate) struct CloudflareRpcContext {
    pub(crate) engine: Rc<Mutex<NostrEngine<CloudflareStorage>>>,
    pub(crate) manager: Rc<Mutex<ConnectionManager<CloudflareConnectionTransport>>>,
    pub(crate) id_allocator: Rc<IdAllocator<DoIdBatchStore>>,
}

impl CloudflareRpcContext {
    pub(crate) fn new(
        engine: Rc<Mutex<NostrEngine<CloudflareStorage>>>,
        manager: Rc<Mutex<ConnectionManager<CloudflareConnectionTransport>>>,
        id_allocator: Rc<IdAllocator<DoIdBatchStore>>,
    ) -> Self {
        Self {
            engine,
            manager,
            id_allocator,
        }
    }
}

#[async_trait::async_trait(?Send)]
impl RpcContext for CloudflareRpcContext {
    fn now(&self) -> u64 {
        crate::util::now()
    }

    async fn allocate_connection_id(&self) -> u32 {
        self.id_allocator.allocate().await
    }

    async fn execute_action(&self, conn_id: u32, action: RpcAction) -> Result<(), crate::common::NwcError> {
        let mut engine = self.engine.lock().await;
        let engine_ref = &mut *engine;

        let responses = match action {
            RpcAction::Subscribe(sub_id, filter) => {
                engine_ref.handle_req(conn_id, sub_id, vec![filter], crate::nostr::engine::SubscriptionOrigin::Internal).await
            }
            RpcAction::Publish(event) => {
                let event_msg = crate::nostr::ClientMessage::Event(event);
                engine_ref.handle_typed(conn_id, event_msg).await
            }
            RpcAction::Unsubscribe(sub_id) => {
                engine_ref.process_close(conn_id, sub_id).await
            }
        };

        let mut manager = self.manager.lock().await;
        manager.dispatch(responses, engine_ref).await;
        Ok(())
    }

    async fn receive_response(&self, conn_id: u32, remaining_secs: u64) -> Result<String, RpcReceiveError> {
        let (tx, rx) = oneshot::channel();
        self.manager.lock().await.add_internal_channel(conn_id, tx);

        let delay = Delay::from(Duration::from_secs(remaining_secs)).fuse();
        let pinned_rx = rx.fuse();
        futures_util::pin_mut!(pinned_rx, delay);

        match futures_util::future::select(pinned_rx, delay).await {
            futures_util::future::Either::Left((Ok(resp), _)) => Ok(resp),
            futures_util::future::Either::Left((Err(_), _)) => Err(RpcReceiveError::ChannelClosed),
            futures_util::future::Either::Right(_) => {
                self.manager.lock().await.remove_internal_channel(conn_id);
                Err(RpcReceiveError::Timeout)
            }
        }
    }

    async fn disconnect(&self, conn_id: u32) {
        let mut engine = self.engine.lock().await;
        let engine_ref = &mut *engine;
        let _ = engine_ref.on_disconnect(conn_id).await;
        self.manager.lock().await.remove_internal_channel(conn_id);
    }
}
