use std::time::Duration;

pub trait ListingCache: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, value: &str, ttl: Duration);
}
