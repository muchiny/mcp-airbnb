# 🔌 Ports Layer

The **ports layer** defines trait boundaries between the domain core and the outside world. Ports declare **what** the system needs without specifying **how** it's achieved — the adapters provide the concrete implementations.

## 🎯 Traits

### `AirbnbClient` (`airbnb_client.rs`)

The primary outbound port for fetching Airbnb data.

```rust
#[async_trait]
pub trait AirbnbClient: Send + Sync {
    async fn search_listings(&self, params: &SearchParams) -> Result<SearchResult>;
    async fn get_listing_detail(&self, id: &str) -> Result<ListingDetail>;
    async fn get_reviews(&self, id: &str, cursor: Option<&str>) -> Result<ReviewsPage>;
    async fn get_price_calendar(&self, id: &str, months: u32) -> Result<PriceCalendar>;
}
```

### `ListingCache` (`cache.rs`)

Outbound port for caching serialized data with TTL.

```rust
pub trait ListingCache: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, value: &str, ttl: Duration);
}
```

## 🔗 Port → Adapter Mapping

| Port | Adapter | Location |
|------|---------|----------|
| `AirbnbClient` | `AirbnbScraper` | `adapters/scraper/client.rs` |
| `ListingCache` | `MemoryCache` | `adapters/cache/memory_cache.rs` |

## 🎨 Design Principles

- **Domain types only** — Ports use `SearchResult`, `ListingDetail`, `ReviewsPage`, `PriceCalendar` from the domain layer. No adapter-specific types leak through.
- **`AirbnbClient` is async** — Uses `async_trait` because fetching data involves network I/O.
- **`ListingCache` is synchronous** — Cache operations are fast (in-memory LRU behind `RwLock`), no async overhead needed.
- **`Send + Sync` bounds** — Both traits require thread safety for sharing across tokio tasks via `Arc<dyn T>`.
- **Error type** — Both use `crate::error::Result<T>` (alias for `Result<T, AirbnbError>`).
