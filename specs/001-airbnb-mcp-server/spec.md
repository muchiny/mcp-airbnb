# Feature Specification: Airbnb MCP Server

**Feature Branch**: `001-airbnb-mcp-server`
**Created**: 2026-02-23
**Status**: Implemented

## User Scenarios & Testing

### User Story 1 - Search Airbnb Listings (Priority: P1)

As an AI assistant user, I want to search Airbnb listings by location, dates, and guest count so that I can find suitable accommodation options.

**Acceptance Scenarios**:
1. **Given** a location "Paris, France", **When** I search with 2 adults and dates June 1-5, **Then** I receive a list of available listings with prices, ratings, and links.
2. **Given** a search with no results, **When** the search completes, **Then** I receive a clear "no listings found" message.
3. **Given** invalid parameters (missing location), **When** I attempt to search, **Then** I receive a validation error.

### User Story 2 - View Listing Details (Priority: P1)

As an AI assistant user, I want to get detailed information about a specific Airbnb listing so that I can evaluate it.

**Acceptance Scenarios**:
1. **Given** a valid listing ID, **When** I request details, **Then** I receive description, amenities, house rules, photos, and host info.
2. **Given** an invalid listing ID, **When** I request details, **Then** I receive a clear error message.

### User Story 3 - Read Reviews (Priority: P2)

As an AI assistant user, I want to read reviews for an Airbnb listing so that I can assess quality.

**Acceptance Scenarios**:
1. **Given** a listing ID, **When** I request reviews, **Then** I receive ratings summary and individual reviews.
2. **Given** many reviews, **When** I paginate, **Then** I receive the next page of reviews.

### User Story 4 - Check Price Calendar (Priority: P2)

As an AI assistant user, I want to see the price calendar for a listing so that I can find the best dates.

**Acceptance Scenarios**:
1. **Given** a listing ID, **When** I request the calendar, **Then** I receive daily prices, availability, and minimum night requirements.

## Requirements

### Functional Requirements

- **FR-001**: System MUST provide an `airbnb_search` tool that accepts location, dates, guests, and price filters
- **FR-002**: System MUST provide an `airbnb_listing_details` tool that returns full listing information
- **FR-003**: System MUST provide an `airbnb_reviews` tool with pagination support
- **FR-004**: System MUST provide an `airbnb_price_calendar` tool showing daily prices and availability
- **FR-005**: System MUST validate search parameters before executing requests
- **FR-006**: System MUST cache responses to avoid excessive requests
- **FR-007**: System MUST rate-limit outbound requests (configurable, default 0.5 req/s)
- **FR-008**: System MUST communicate via MCP protocol over stdio (JSON-RPC)

### Key Entities

- **Listing**: Search result with id, name, location, price, rating, URL
- **ListingDetail**: Extended listing with description, amenities, photos, capacity
- **Review**: Author, date, rating, comment, host response
- **PriceCalendar**: Daily price, availability, minimum nights per listing
- **SearchParams**: Location (required), dates, guests, price range, cursor

## Success Criteria

- **SC-001**: All 4 MCP tools return well-formatted text responses
- **SC-002**: Search results parse correctly from Airbnb HTML/JSON
- **SC-003**: Rate limiting prevents more than 1 request per 2 seconds
- **SC-004**: Cache reduces repeated requests for the same data
- **SC-005**: `cargo test` passes, `cargo clippy` produces 0 warnings

## Acceptance Criteria

- All P1 stories implemented and tested
- All FR-xxx requirements satisfied
- 23+ unit tests passing
- Zero clippy warnings
- Manual test via Claude Code MCP integration
