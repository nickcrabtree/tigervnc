/* Copyright (C) 2025 TigerVNC Team
 * 
 * This is free software; you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation; either version 2 of the License, or
 * (at your option) any later version.
 * 
 * This software is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 * 
 * You should have received a copy of the GNU General Public License
 * along with this software; if not, write to the Free Software
 * Foundation, Inc., 59 Temple Place - Suite 330, Boston, MA  02111-1307,
 * USA.
 */

#ifndef __RFB_CACHE_COORDINATOR_PROTOCOL_H__
#define __RFB_CACHE_COORDINATOR_PROTOCOL_H__

#include <stdint.h>
#include <vector>
#include <string>
#include <cstring>
#include <arpa/inet.h>

namespace rfb {
namespace cache {

// =============================================================================
// Multi-Viewer Cache Coordination Protocol
// =============================================================================
//
// This protocol enables multiple VNC viewer processes on the same machine to
// share a single PersistentCache directory without conflicts. The first viewer
// becomes the "master" and subsequent viewers become "slaves".
//
// Message Format:
//   [4 bytes] Message length (network byte order, includes type + payload)
//   [1 byte]  Message type
//   [N bytes] Payload (type-specific)
//
// Threading: The coordinator runs a background thread for IPC. Callbacks to
// GlobalClientPersistentCache are synchronized via mutex.

// Protocol version - increment when wire format changes
static const uint16_t COORDINATOR_PROTOCOL_VERSION = 1;

// Message types
enum class CoordMsgType : uint8_t {
  // Handshake
  HELLO         = 0x01,  // Slave -> Master: announce connection
  WELCOME       = 0x02,  // Master -> Slave: send index snapshot
  
  // Write coordination
  WRITE_REQ     = 0x03,  // Slave -> Master: request cache entry write
  WRITE_ACK     = 0x04,  // Master -> Slave: confirm write with index entry
  WRITE_NACK    = 0x05,  // Master -> Slave: write failed
  
  // Index synchronization
  INDEX_UPDATE  = 0x06,  // Master -> Slave: broadcast new index entries
  
  // Keepalive
  PING          = 0x07,  // Either direction
  PONG          = 0x08,  // Response to PING
  
  // Lifecycle
  MASTER_EXIT   = 0x09,  // Master -> Slave: graceful shutdown, trigger election
  SLAVE_EXIT    = 0x0A,  // Slave -> Master: graceful disconnect
  
  // Query (optional optimization)
  QUERY_INDEX   = 0x0B,  // Slave -> Master: check if hash exists
  QUERY_RESP    = 0x0C,  // Master -> Slave: response to query
};

// =============================================================================
// Index Entry (Wire Format)
// =============================================================================
// This mirrors GlobalClientPersistentCache::IndexEntry but in a serializable form.

#pragma pack(push, 1)
struct WireIndexEntry {
  uint8_t  hash[16];        // Content hash
  uint16_t shardId;         // Shard file number
  uint32_t payloadOffset;   // Offset within shard
  uint32_t payloadSize;     // Size of pixel data
  uint16_t width;
  uint16_t height;
  uint16_t stridePixels;
  uint64_t canonicalHash;   // Server's canonical hash
  uint64_t actualHash;      // Client's actual hash (may differ if lossy)
  uint8_t  qualityCode;     // Depth + lossy flag
  uint8_t  flags;           // Bit 0: isCold
  
  // PixelFormat (VNC wire format, 16 bytes)
  uint8_t  pf_bpp;
  uint8_t  pf_depth;
  uint8_t  pf_bigEndian;
  uint8_t  pf_trueColour;
  uint16_t pf_redMax;
  uint16_t pf_greenMax;
  uint16_t pf_blueMax;
  uint8_t  pf_redShift;
  uint8_t  pf_greenShift;
  uint8_t  pf_blueShift;
  uint8_t  pf_padding[3];
};
#pragma pack(pop)

static_assert(sizeof(WireIndexEntry) == 16 + 2 + 4 + 4 + 2 + 2 + 2 + 8 + 8 + 1 + 1 + 16,
              "WireIndexEntry size mismatch");

// =============================================================================
// Message Payloads
// =============================================================================

// HELLO payload (Slave -> Master)
#pragma pack(push, 1)
struct HelloPayload {
  uint16_t protocolVersion;
  uint32_t pid;             // Slave's PID for debugging
  uint8_t  reserved[10];
};
#pragma pack(pop)

// WELCOME payload (Master -> Slave)
// Variable length: fixed header + N index entries
#pragma pack(push, 1)
struct WelcomeHeader {
  uint16_t protocolVersion;
  uint32_t masterPid;
  uint32_t entryCount;      // Number of WireIndexEntry following
  uint16_t currentShardId;  // Current shard being written to
  uint8_t  reserved[6];
};
#pragma pack(pop)

// WRITE_REQ payload (Slave -> Master)
// Variable length: header + pixel payload
#pragma pack(push, 1)
struct WriteReqHeader {
  WireIndexEntry entry;     // Metadata (payloadOffset will be filled by master)
  uint32_t payloadLength;   // Length of pixel data following
  // Followed by: uint8_t payload[payloadLength]
};
#pragma pack(pop)

// WRITE_ACK payload (Master -> Slave)
#pragma pack(push, 1)
struct WriteAckPayload {
  WireIndexEntry entry;     // Complete entry with final shardId/offset
  uint32_t requestId;       // Echo back for correlation (optional)
};
#pragma pack(pop)

// INDEX_UPDATE payload (Master -> Slaves)
// Variable length: header + N index entries
#pragma pack(push, 1)
struct IndexUpdateHeader {
  uint32_t entryCount;      // Number of WireIndexEntry following
  uint32_t sequenceNum;     // For detecting missed updates
};
#pragma pack(pop)

// QUERY_INDEX payload (Slave -> Master)
#pragma pack(push, 1)
struct QueryIndexPayload {
  uint8_t  hash[16];
  uint16_t width;
  uint16_t height;
};
#pragma pack(pop)

// QUERY_RESP payload (Master -> Slave)
#pragma pack(push, 1)
struct QueryRespPayload {
  uint8_t  found;           // 0 = not found, 1 = found
  WireIndexEntry entry;     // Valid only if found == 1
};
#pragma pack(pop)

// =============================================================================
// Message Buffer Helpers
// =============================================================================

class CoordMessage {
public:
  CoordMessage() : type_(CoordMsgType::PING) {}
  
  explicit CoordMessage(CoordMsgType type) : type_(type) {}
  
  CoordMsgType type() const { return type_; }
  const std::vector<uint8_t>& payload() const { return payload_; }
  std::vector<uint8_t>& payload() { return payload_; }
  
  // Serialize to wire format
  std::vector<uint8_t> serialize() const {
    uint32_t totalLen = 1 + payload_.size();  // type + payload
    std::vector<uint8_t> buf(4 + totalLen);
    
    uint32_t netLen = htonl(totalLen);
    memcpy(buf.data(), &netLen, 4);
    buf[4] = static_cast<uint8_t>(type_);
    if (!payload_.empty()) {
      memcpy(buf.data() + 5, payload_.data(), payload_.size());
    }
    return buf;
  }
  
  // Parse from wire format (returns bytes consumed, 0 if incomplete, -1 on error)
  static int parse(const uint8_t* data, size_t len, CoordMessage& out) {
    if (len < 5) return 0;  // Need at least length + type
    
    uint32_t netLen;
    memcpy(&netLen, data, 4);
    uint32_t msgLen = ntohl(netLen);
    
    if (msgLen < 1 || msgLen > 64 * 1024 * 1024) return -1;  // Sanity check
    
    size_t totalSize = 4 + msgLen;
    if (len < totalSize) return 0;  // Incomplete
    
    out.type_ = static_cast<CoordMsgType>(data[4]);
    out.payload_.assign(data + 5, data + totalSize);
    
    return static_cast<int>(totalSize);
  }
  
  // Convenience: append raw struct to payload
  template<typename T>
  void appendStruct(const T& s) {
    const uint8_t* p = reinterpret_cast<const uint8_t*>(&s);
    payload_.insert(payload_.end(), p, p + sizeof(T));
  }
  
  // Convenience: read struct from payload at offset
  template<typename T>
  bool readStruct(size_t offset, T& out) const {
    if (offset + sizeof(T) > payload_.size()) return false;
    memcpy(&out, payload_.data() + offset, sizeof(T));
    return true;
  }
  
private:
  CoordMsgType type_;
  std::vector<uint8_t> payload_;
};

// =============================================================================
// File Paths
// =============================================================================

inline std::string getCoordinatorSocketPath(const std::string& cacheDir) {
  return cacheDir + "/coordinator.sock";
}

inline std::string getCoordinatorLockPath(const std::string& cacheDir) {
  return cacheDir + "/coordinator.lock";
}

inline std::string getCoordinatorPidPath(const std::string& cacheDir) {
  return cacheDir + "/coordinator.pid";
}

}  // namespace cache
}  // namespace rfb

#endif // __RFB_CACHE_COORDINATOR_PROTOCOL_H__
