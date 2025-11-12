# Shared cache utilities

This directory contains shared, cache-related utilities used by both
ContentCache (session-only) and PersistentCache (cross-session):

- ArcCache.h (header-only)
  - Template Adaptive Replacement Cache (ARC) with byte-based capacity.
  - Provides Stats and eviction callback for integration with protocol.

- BandwidthStats.{h,cxx}
  - Shared tracking helpers and CacheProtocolStats struct for accounting
    bytes (references/inits) and computing bandwidth savings.

- ProtocolHelpers.h
  - batchForSending<T>() to split large vectors into conservative batches
    for protocol messages.

- ServerHashSet.h (header-only)
  - Template utility for server-side tracking of client-known cache keys.
  - Used by both ContentCache (uint64_t IDs) and PersistentCache (hash vectors)
    to maintain server knowledge of what client has cached.
  - Provides add/remove/has operations with statistics tracking.

Integration status

- PersistentCache viewer (GlobalClientPersistentCache) uses ArcCache for
  pixel caching with eviction callback feeding the client→server eviction
  message.

- ContentCache viewer now uses ArcCache for the decoded pixel cache; stats
  are sourced from ArcCache::Stats.

- DecodeManager uses BandwidthStats to track and report bandwidth usage
  for both ContentCache and PersistentCache.

- EncodeManager uses ServerHashSet to track client-known PersistentCache
  hashes with eviction support (replaces raw unordered_set).

Planned work

- Migrate ContentCache server-side hash cache to ArcCache to eliminate
  the remaining custom ARC implementation.
- Add unit tests for ArcCache behavior and BandwidthStats accounting.
