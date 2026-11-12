# 🔌 Ports Layer

The **ports layer** defines trait boundaries between the domain core and the outside world. Ports declare **what** the system needs without specifying **how** it's achieved — the adapters provide the concrete implementations.

## 🎯 Traits

### `AirbnbClient` (`airbnb_client.rs`)

The primary outbound port for fetching Airbnb data. The **8 async methods are required**, so every adapter and every test mock implements each one. Two methods have a default. `degradations()` (none): `CompositeClient` overrides it to report the operations whose GraphQL source failed and that the HTML fallback is serving; an operation leaves the list when GraphQL answers it again over the network (a cache hit does not count) or after 15 minutes. `clear_cache()` (no-op): the adapters drop their cached answers, so the status check probes Airbnb itself and never reports a cached answer as a healthy source.

```rust
#[async_trait]
pub trait AirbnbClient: Send + Sync {
    async fn search_listings(&self, params: &SearchParams) -> Result<SearchResult>;
    async fn get_listing_detail(&self, id: &str) -> Result<ListingDetail>;
    async fn get_reviews(&self, id: &str, cursor: Option<&str>) -> Result<ReviewsPage>;
    async fn get_price_calendar(&self, id: &str, months: u32) -> Result<PriceCalendar>;
    async fn get_host_profile(&self, listing_id: &str) -> Result<HostProfile>;
    async fn get_neighborhood_stats(&self, params: &SearchParams) -> Result<NeighborhoodStats>;
    async fn get_occupancy_estimate(&self, id: &str, months: u32) -> Result<OccupancyEstimate>;
    async fn get_price_quote(&self, request: &QuoteRequest) -> Result<PriceQuote>;
    fn degradations(&self) -> Vec<Degradation> { Vec::new() }
}
```

```mermaid
classDiagram
    class AirbnbClient {
        <<trait>>
        +search_listings(params) SearchResult
        +get_listing_detail(id) ListingDetail
        +get_reviews(id, cursor) ReviewsPage
        +get_price_calendar(id, months) PriceCalendar
        +get_host_profile(listing_id) HostProfile
        +get_neighborhood_stats(params) NeighborhoodStats
        +get_occupancy_estimate(id, months) OccupancyEstimate
        +get_price_quote(request) PriceQuote
        +degradations() Vec~Degradation~
        +clear_cache()
    }
```

### `ListingCache` (`cache.rs`)

Outbound port for caching serialized data with TTL.

```rust
pub trait ListingCache: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, value: &str, ttl: Duration);
    fn clear(&self);
}
```

## 🔗 Port → Adapter Mapping

```mermaid
flowchart LR
    subgraph Ports["🔌 Ports"]
        AC["AirbnbClient"]
        LC["ListingCache"]
    end

    subgraph Adapters["⚡ Adapters"]
        Composite["🔀 CompositeClient"]
        GQL["🔗 AirbnbGraphQLClient"]
        Scraper["🕷️ AirbnbScraper"]
        Cache["💾 MemoryCache"]
    end

    Composite -.->|implements| AC
    GQL -.->|implements| AC
    Scraper -.->|implements| AC
    Cache -.->|implements| LC
```

| Port | Adapter | Location | Notes |
|------|---------|----------|-------|
| `AirbnbClient` | 🔀 `CompositeClient` | `adapters/composite.rs` | GraphQL + Scraper with auto-fallback |
| `AirbnbClient` | 🔗 `AirbnbGraphQLClient` | `adapters/graphql/client.rs` | Primary — GraphQL API |
| `AirbnbClient` | 🕷️ `AirbnbScraper` | `adapters/scraper/client.rs` | Fallback — HTML scraping |
| `ListingCache` | 💾 `MemoryCache` | `adapters/cache/memory_cache.rs` | LRU with TTL |

## 🎨 Design Principles

- ✅ **Domain types only** — Ports use `SearchResult`, `ListingDetail`, `ReviewsPage`, `PriceCalendar`, `HostProfile`, `NeighborhoodStats`, `OccupancyEstimate` from the domain layer. No adapter-specific types leak through.
- ⚡ **`AirbnbClient` is async** — Uses `async_trait` because fetching data involves network I/O.
- 🔒 **`ListingCache` is synchronous** — Cache operations are fast (in-memory LRU behind `RwLock`), no async overhead needed.
- 🧵 **`Send + Sync` bounds** — Both traits require thread safety for sharing across tokio tasks via `Arc<dyn T>`.
- ❌ **Error type** — Both use `crate::error::Result<T>` (alias for `Result<T, AirbnbError>`).
- ❗ **Upstream drift** — an adapter that cannot recognise a response returns `AirbnbError::UpstreamSchema { operation, detail }`, never an empty `Ok` result.
