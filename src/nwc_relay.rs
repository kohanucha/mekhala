use crate::relay::{Event, Filter, KIND_NWC_INFO, RelayMessage};
use crate::utils::HibernationState;
use lru::LruCache;
use std::cell::RefCell;
use std::collections::HashMap;
use worker::*;
use wasm_bindgen::JsValue;

pub struct RelayHandler<'a> {
    pub state: &'a State,
    pub active_wallets: &'a RefCell<HashMap<String, usize>>,
    pub verification_cache: &'a RefCell<LruCache<String, Result<(), String>>>,
}

impl<'a> RelayHandler<'a> {
    pub async fn handle_event(
        &self,
        ws: &WebSocket,
        conn_state: &mut crate::ConnectionState,
        event: Event,
    ) -> Result<()> {
        let current_time = (Date::now().as_millis() / 1000) as u64;

        // 1. Verify Event (with cache)
        let cached_res = self.verification_cache.borrow_mut().get(&event.id).cloned();
        let verification_result = if let Some(res) = cached_res {
            res
        } else {
            let res = event
                .verify(current_time, &conn_state.limits)
                .map_err(|e| e.to_string());
            self.verification_cache
                .borrow_mut()
                .put(event.id.clone(), res.clone());
            res
        };

        if let Err(reason) = verification_result {
            let _ = ws.send_with_str(
                &RelayMessage::Ok(event.id, false, reason).to_json(),
            );
            return Ok(());
        }

        // 2. Cache Info Event (if applicable)
        if event.kind == KIND_NWC_INFO {
            conn_state.info_event = Some(event.clone());
            let _ = ws.serialize_attachment(&*conn_state);
        }

        // 3. Acknowledge Event
        let _ = ws.send_with_str(
            &RelayMessage::Ok(event.id.clone(), true, "".into()).to_json(),
        );

        // 4. Broadcast
        self.broadcast(event).await
    }

    async fn broadcast(&self, event: Event) -> Result<()> {
        let mut target_websockets: Vec<WebSocket> = Vec::new();

        let mut add_if_unique = |target_ws: WebSocket| {
            if !target_websockets
                .iter()
                .any(|w| {
                    let w_js: &JsValue = w.as_ref();
                    let target_js: &JsValue = target_ws.as_ref();
                    w_js == target_js
                })
            {
                target_websockets.push(target_ws);
            }
        };

        if self.state.is_tags_supported() {
            for target_ws in self.state.get_tagged_websockets(&event.pubkey) {
                add_if_unique(target_ws);
            }
            for tag in &event.tags {
                if tag.len() >= 2 && tag[0].as_str() == Some("p") {
                    if let Some(p_pubkey) = tag[1].as_str() {
                        for target_ws in self.state.get_tagged_websockets(p_pubkey) {
                            add_if_unique(target_ws);
                        }
                    }
                }
            }
        } else {
            for ws in self.state.get_websockets() {
                target_websockets.push(ws);
            }
        }

        for target_ws in &target_websockets {
            let other_state: crate::ConnectionState = match target_ws.deserialize_attachment() {
                Ok(Some(s)) => s,
                _ => continue,
            };
            for (sub_id, filters) in &other_state.subscriptions {
                if filters.iter().any(|f| f.matches(&event)) {
                    let _ = target_ws.send_with_str(
                        &RelayMessage::Event(sub_id.clone(), event.clone()).to_json(),
                    );
                }
            }
        }

        Ok(())
    }

    pub async fn handle_req(
        &self,
        ws: &WebSocket,
        conn_state: &mut crate::ConnectionState,
        sub_id: String,
        filters: Vec<Filter>,
    ) -> Result<()> {
        if conn_state.subscriptions.len() >= 20 && !conn_state.subscriptions.contains_key(&sub_id) {
            let _ = ws.send_with_str(
                &RelayMessage::Closed(sub_id, "rate-limited: max 20 subscriptions".into()).to_json(),
            );
            return Ok(());
        }

        if filters.iter().any(|f| !f.is_valid(&conn_state.limits)) {
            let msg = "restricted: NIP-47 subscriptions must be narrowed by author, p-tag, or e-tag";
            let _ = ws.send_with_str(&RelayMessage::Closed(sub_id, msg.into()).to_json());
            return Ok(());
        }

        conn_state.subscriptions.insert(sub_id.clone(), filters.clone());
        ws.serialize_attachment(&*conn_state)?;

        self.update_wallet_count(&filters, true);
        self.update_websocket_tags(ws, conn_state);

        // Serve cached Info Events
        if filters
            .iter()
            .any(|f| f.kinds.as_ref().map_or(false, |k| k.contains(&KIND_NWC_INFO)))
        {
            for other_ws in self.state.get_websockets() {
                let other_state: crate::ConnectionState = match other_ws.deserialize_attachment() {
                    Ok(Some(s)) => s,
                    _ => continue,
                };
                if let Some(cached_info) = &other_state.info_event {
                    if filters.iter().any(|f| f.matches(cached_info)) {
                        let _ = ws.send_with_str(
                            &RelayMessage::Event(sub_id.clone(), cached_info.clone()).to_json(),
                        );
                    }
                }
            }
        }

        let _ = ws.send_with_str(&RelayMessage::Eose(sub_id).to_json());
        Ok(())
    }

    pub fn update_wallet_count(&self, filters: &[Filter], increment: bool) {
        let mut wallets = self.active_wallets.borrow_mut();
        for filter in filters {
            for pubkey in filter.pubkeys() {
                if increment {
                    *wallets.entry(pubkey).or_insert(0) += 1;
                } else if let Some(count) = wallets.get_mut(&pubkey) {
                    *count = count.saturating_sub(1);
                }
            }
        }
        if !increment {
            wallets.retain(|_, v| *v > 0);
        }
    }

    pub fn update_websocket_tags(&self, ws: &WebSocket, conn_state: &crate::ConnectionState) {
        let mut unique_pubkeys: std::collections::HashSet<String> = std::collections::HashSet::new();
        for filters in conn_state.subscriptions.values() {
            for filter in filters {
                for pubkey in filter.pubkeys() {
                    unique_pubkeys.insert(pubkey);
                }
            }
        }
        let tags: Vec<String> = unique_pubkeys.into_iter().take(10).collect();
        self.state.set_tags(ws, tags);
    }
}
