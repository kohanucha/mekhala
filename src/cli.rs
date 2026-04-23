use std::sync::Arc;

use crate::whitelist::WhitelistStore;

#[allow(dead_code)]
    pub fn validate_pubkey(pubkey: &str) -> Result<(), String> {
    if pubkey.len() != 64 {
        return Err("Pubkey must be 64 hex characters".to_string());
    }
    if pubkey.chars().any(|c| !c.is_ascii_hexdigit()) {
        return Err("Pubkey must be valid hex".to_string());
    }
    Ok(())
}

#[allow(dead_code)]
    pub async fn handle_add(store: Arc<WhitelistStore>, pubkey: &str) -> Result<String, String> {
    let was_present = store.add(pubkey).await?;
    if was_present {
        Ok(format!("Pubkey already exists: {}", pubkey))
    } else {
        Ok(format!("Added pubkey: {}", pubkey))
    }
}

#[allow(dead_code)]
    pub async fn handle_remove(store: Arc<WhitelistStore>, pubkey: &str) -> Result<String, String> {
    let was_present = store.remove(pubkey).await?;
    if was_present {
        Ok(format!("Removed pubkey: {}", pubkey))
    } else {
        Ok(format!("Pubkey not found: {}", pubkey))
    }
}

#[allow(dead_code)]
    pub async fn handle_list(store: Arc<WhitelistStore>) -> Result<String, String> {
    let pubkeys = store.list().await?;
    if pubkeys.is_empty() {
        Ok("No whitelisted pubkeys".to_string())
    } else {
        Ok(pubkeys.join("\n"))
    }
}