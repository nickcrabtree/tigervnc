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

Integration status

- PersistentCache viewer (GlobalClientPersistentCache) uses ArcCache for
  pixel caching with eviction callback feeding the client→server eviction
  message.

- DecodeManager uses BandwidthStats to track and report bandwidth usage
  for both ContentCache and PersistentCache.

Planned work

- Migrate ContentCache to use ArcCache for both server-side hash cache
  and client-side pixel cache to eliminate duplicate ARC implementations.
- Add unit tests for ArcCache behavior and BandwidthStats accounting.
