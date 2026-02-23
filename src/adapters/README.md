# ⚡ Adapters Layer

The **adapters layer** provides concrete implementations of the port traits. This is where all I/O happens — HTTP requests to Airbnb and in-memory caching.

## 📂 Structure

```
adapters/
├── scraper/          # 🕷️ Web scraping — implements AirbnbClient
│   ├── client.rs     #    HTTP client, retry logic, cache-aside
│   ├── search_parser.rs
│   ├── detail_parser.rs
│   ├── review_parser.rs
│   ├── calendar_parser.rs
│   └── rate_limiter.rs
├── cache/            # 💾 Caching — implements ListingCache
│   └── memory_cache.rs
└── mod.rs
```

> See [scraper/README.md](scraper/README.md) for detailed scraper documentation.

## 🌐 Scraper Adapter

`AirbnbScraper` implements `AirbnbClient` by scraping public Airbnb HTML pages. It uses `reqwest` with cookie support, applies rate limiting, and caches results with configurable TTLs.

## 💾 Cache Adapter

`MemoryCache` implements `ListingCache` using an in-memory LRU cache (`lru` crate) protected by `RwLock`. Each entry stores the serialized JSON value alongside its expiration timestamp. Expired entries are evicted on access.

## 🔄 Parsing Strategy

All parsers follow the same multi-tier extraction strategy:

```mermaid
flowchart TD
    HTML["📄 Raw HTML Response"]
    HTML --> ND{"🔍 script#__NEXT_DATA__<br/>exists?"}
    ND -->|Yes| ParseJSON["Parse JSON payload"]
    ND -->|No| DS{"🔍 script[data-deferred-state]<br/>exists?"}
    ParseJSON --> Extract["Extract data via<br/>known JSON paths"]
    Extract --> Found{"✅ Data found?"}
    Found -->|Yes| Result["✅ Return parsed result"]
    Found -->|No| Deep["🔎 Recursive deep search<br/>for listing-like objects"]
    Deep -->|Found| Result
    Deep -->|Not found| DS
    DS -->|Yes| ParseDeferred["Parse deferred<br/>state JSON"]
    ParseDeferred --> Extract
    DS -->|No| CSS["🎨 CSS Selector Fallback"]
    CSS --> CSSParse["Parse via itemprop<br/>& data-testid"]
    CSSParse -->|Found| Result
    CSSParse -->|Empty| Error["❌ Parse Error"]
```

## 🗝️ Cache Key Strategy

| Tool | Cache Key Pattern | Default TTL |
|------|-------------------|-------------|
| Search | `search:{location}:{checkin}:{checkout}:{adults}:{cursor}` | 15 min (900s) |
| Detail | `detail:{id}` | 1 hour (3600s) |
| Reviews | `reviews:{id}:{cursor\|"first"}` | 1 hour (3600s) |
| Calendar | `calendar:{id}` | 30 min (1800s) |
