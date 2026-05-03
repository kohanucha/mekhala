pub mod lnaddress;
pub mod bridge;
pub mod handler;

pub use handler::{handle_lnaddress, handle_lnaddress_callback};

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::Digest;

    #[test]
    fn test_lnaddress_metadata() {
        let addr = lnaddress::LNAddress::new("testuser");
        assert_eq!(addr.generate_metadata(), "[[\"text/plain\",\"Payment to testuser\"]]");
    }

    #[test]
    fn test_lnaddress_description_hash() {
        let addr = lnaddress::LNAddress::new("testuser");
        let hash = addr.get_description_hash();
        
        let mut expected_hasher = sha2::Sha256::new();
        expected_hasher.update(b"[[\"text/plain\",\"Payment to testuser\"]]");
        let expected_hash = hex::encode(expected_hasher.finalize());
        
        assert_eq!(hash, expected_hash);
    }

    #[test]
    fn test_lnaddress_info() {
        let addr = lnaddress::LNAddress::new("testuser");
        let info = addr.get_info("https://callback.url");
        assert_eq!(info["callback"], "https://callback.url");
        assert_eq!(info["maxSendable"], 100000000);
        assert_eq!(info["minSendable"], 1000);
        assert_eq!(info["tag"], "payRequest");
        assert_eq!(info["metadata"], "[[\"text/plain\",\"Payment to testuser\"]]");
    }
}