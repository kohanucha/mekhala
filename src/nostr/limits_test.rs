use super::*;

    #[test]
    fn test_limits_default() {
        let limits = Limits::default();
        assert_eq!(limits.max_content_length, 65536);
        assert_eq!(limits.max_subscriptions_per_connection, 100);
    }

    #[test]
    fn test_limits_new() {
        let limits = Limits::new(16384, 50);
        assert_eq!(limits.max_content_length, 16384);
        assert_eq!(limits.max_subscriptions_per_connection, 50);
    }
