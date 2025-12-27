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

#ifdef HAVE_CONFIG_H
#include <config.h>
#endif

#include <rfb/cache/CacheCoordinator.h>
#include <core/LogWriter.h>

#include <sys/socket.h>
#include <sys/un.h>
#include <sys/file.h>
#include <sys/stat.h>
#include <unistd.h>
#include <fcntl.h>
#include <poll.h>
#include <errno.h>
#include <signal.h>

#include <algorithm>
#include <chrono>
#include <condition_variable>

namespace rfb {
namespace cache {

static core::LogWriter vlog("CacheCoordinator");

// Timeout for operations in milliseconds
static const int CONNECT_TIMEOUT_MS = 2000;
static const int WRITE_REQUEST_TIMEOUT_MS = 5000;
static const int POLL_TIMEOUT_MS = 100;

// =============================================================================
// Helper Functions
// =============================================================================

static bool ensureDirectory(const std::string& dir) {
  struct stat st;
  if (stat(dir.c_str(), &st) == 0) {
    return S_ISDIR(st.st_mode);
  }
  // Create directory recursively
  std::string current;
  for (size_t i = 0; i < dir.length(); i++) {
    if (dir[i] == '/' && i > 0) {
      current = dir.substr(0, i);
      mkdir(current.c_str(), 0755);
    }
  }
  return mkdir(dir.c_str(), 0755) == 0 || errno == EEXIST;
}

static bool isProcessAlive(pid_t pid) {
  return kill(pid, 0) == 0 || errno == EPERM;
}

static pid_t readPidFile(const std::string& path) {
  FILE* f = fopen(path.c_str(), "r");
  if (!f) return 0;
  pid_t pid = 0;
  if (fscanf(f, "%d", &pid) != 1) pid = 0;
  fclose(f);
  return pid;
}

static bool writePidFile(const std::string& path, pid_t pid) {
  FILE* f = fopen(path.c_str(), "w");
  if (!f) return false;
  fprintf(f, "%d\n", pid);
  fclose(f);
  return true;
}

static bool setNonBlocking(int fd) {
  int flags = fcntl(fd, F_GETFL, 0);
  if (flags < 0) return false;
  return fcntl(fd, F_SETFL, flags | O_NONBLOCK) >= 0;
}

// =============================================================================
// CacheCoordinator Factory
// =============================================================================

std::unique_ptr<CacheCoordinator> CacheCoordinator::create(
    const std::string& cacheDir,
    IndexUpdateCallback indexUpdateCb,
    WriteRequestCallback writeRequestCb) {
  
#ifdef WIN32
  // Windows not supported yet - return standalone
  vlog.info("Multi-viewer coordination not supported on Windows; using standalone mode");
  return std::unique_ptr<CacheCoordinator>(new StandaloneCoordinator(cacheDir, 
      std::move(indexUpdateCb), std::move(writeRequestCb)));
#else
  
  if (!ensureDirectory(cacheDir)) {
    vlog.error("Failed to create cache directory %s", cacheDir.c_str());
    return std::unique_ptr<CacheCoordinator>(new StandaloneCoordinator(cacheDir,
        std::move(indexUpdateCb), std::move(writeRequestCb)));
  }
  
  std::string lockPath = getCoordinatorLockPath(cacheDir);
  std::string pidPath = getCoordinatorPidPath(cacheDir);
  std::string sockPath = getCoordinatorSocketPath(cacheDir);
  
  // Try to acquire the master lock
  int lockFd = open(lockPath.c_str(), O_RDWR | O_CREAT, 0644);
  if (lockFd < 0) {
    vlog.error("Failed to open lock file %s: %s", lockPath.c_str(), strerror(errno));
    return std::unique_ptr<CacheCoordinator>(new StandaloneCoordinator(cacheDir,
        std::move(indexUpdateCb), std::move(writeRequestCb)));
  }
  
  // Try non-blocking exclusive lock
  if (flock(lockFd, LOCK_EX | LOCK_NB) == 0) {
    // Got the lock - we're the master
    vlog.info("Acquired master lock; becoming coordinator master");
    
    // Check for stale socket from crashed previous master
    pid_t oldPid = readPidFile(pidPath);
    if (oldPid > 0 && !isProcessAlive(oldPid)) {
      vlog.info("Cleaning up stale socket from PID %d", oldPid);
      unlink(sockPath.c_str());
    }
    
    // Write our PID
    writePidFile(pidPath, getpid());
    
    std::unique_ptr<CacheCoordinator> master(new MasterCoordinator(cacheDir,
        std::move(indexUpdateCb), std::move(writeRequestCb)));
    // Transfer lock ownership to MasterCoordinator
    // (Note: lock is released in MasterCoordinator destructor)
    return master;
  }
  
  // Lock not acquired - become slave
  close(lockFd);
  
  // Check if master is actually alive
  pid_t masterPid = readPidFile(pidPath);
  if (masterPid > 0 && !isProcessAlive(masterPid)) {
    // Stale lock - try again to become master
    vlog.info("Stale master detected (PID %d dead); retrying lock acquisition", masterPid);
    
    // Brief sleep to avoid tight retry loop
    usleep(100000);  // 100ms
    
    lockFd = open(lockPath.c_str(), O_RDWR | O_CREAT, 0644);
    if (lockFd >= 0 && flock(lockFd, LOCK_EX | LOCK_NB) == 0) {
      unlink(sockPath.c_str());
      writePidFile(pidPath, getpid());
      vlog.info("Acquired master lock after stale cleanup");
      return std::unique_ptr<CacheCoordinator>(new MasterCoordinator(cacheDir,
          std::move(indexUpdateCb), std::move(writeRequestCb)));
    }
    if (lockFd >= 0) close(lockFd);
  }
  
  vlog.info("Another viewer is master; becoming coordinator slave");
  return std::unique_ptr<CacheCoordinator>(new SlaveCoordinator(cacheDir,
      std::move(indexUpdateCb), std::move(writeRequestCb)));
  
#endif
}

// =============================================================================
// MasterCoordinator Implementation
// =============================================================================

MasterCoordinator::MasterCoordinator(const std::string& cacheDir,
                                     IndexUpdateCallback indexUpdateCb,
                                     WriteRequestCallback writeRequestCb)
  : CacheCoordinator(cacheDir, std::move(indexUpdateCb), std::move(writeRequestCb)),
    lockFd_(-1), listenFd_(-1), running_(false), stopRequested_(false) {
  memset(&stats_, 0, sizeof(stats_));
  
  // Re-acquire lock (we know we can get it)
  std::string lockPath = getCoordinatorLockPath(cacheDir);
  lockFd_ = open(lockPath.c_str(), O_RDWR | O_CREAT, 0644);
  if (lockFd_ >= 0) {
    flock(lockFd_, LOCK_EX | LOCK_NB);
  }
}

MasterCoordinator::~MasterCoordinator() {
  stop();
  
  if (lockFd_ >= 0) {
    flock(lockFd_, LOCK_UN);
    close(lockFd_);
  }
  
  // Clean up socket and pid file
  std::string sockPath = getCoordinatorSocketPath(cacheDir_);
  std::string pidPath = getCoordinatorPidPath(cacheDir_);
  unlink(sockPath.c_str());
  unlink(pidPath.c_str());
}

bool MasterCoordinator::start() {
  if (running_) return true;
  
  std::string sockPath = getCoordinatorSocketPath(cacheDir_);
  
  // Remove any existing socket
  unlink(sockPath.c_str());
  
  // Create Unix domain socket
  listenFd_ = socket(AF_UNIX, SOCK_STREAM, 0);
  if (listenFd_ < 0) {
    vlog.error("Failed to create socket: %s", strerror(errno));
    return false;
  }
  
  struct sockaddr_un addr;
  memset(&addr, 0, sizeof(addr));
  addr.sun_family = AF_UNIX;
  strncpy(addr.sun_path, sockPath.c_str(), sizeof(addr.sun_path) - 1);
  
  if (bind(listenFd_, (struct sockaddr*)&addr, sizeof(addr)) < 0) {
    vlog.error("Failed to bind socket %s: %s", sockPath.c_str(), strerror(errno));
    close(listenFd_);
    listenFd_ = -1;
    return false;
  }
  
  if (listen(listenFd_, 10) < 0) {
    vlog.error("Failed to listen: %s", strerror(errno));
    close(listenFd_);
    listenFd_ = -1;
    return false;
  }
  
  setNonBlocking(listenFd_);
  
  running_ = true;
  stopRequested_ = false;
  serverThread_.reset(new std::thread(&MasterCoordinator::serverThread, this));
  
  vlog.info("Master coordinator started on %s", sockPath.c_str());
  return true;
}

void MasterCoordinator::stop() {
  if (!running_) return;
  
  stopRequested_ = true;
  
  // Send MASTER_EXIT to all connected slaves
  {
    std::lock_guard<std::mutex> lock(clientsMutex_);
    CoordMessage exitMsg(CoordMsgType::MASTER_EXIT);
    for (int fd : clientFds_) {
      sendMessage(fd, exitMsg);
    }
  }
  
  if (serverThread_ && serverThread_->joinable()) {
    serverThread_->join();
  }
  serverThread_.reset();
  
  // Close all client connections
  {
    std::lock_guard<std::mutex> lock(clientsMutex_);
    for (int fd : clientFds_) {
      close(fd);
    }
    clientFds_.clear();
    clientBuffers_.clear();
  }
  
  if (listenFd_ >= 0) {
    close(listenFd_);
    listenFd_ = -1;
  }
  
  running_ = false;
  vlog.info("Master coordinator stopped");
}

void MasterCoordinator::serverThread() {
  std::vector<struct pollfd> fds;
  
  while (!stopRequested_) {
    fds.clear();
    
    // Add listen socket
    struct pollfd listenPfd;
    listenPfd.fd = listenFd_;
    listenPfd.events = POLLIN;
    listenPfd.revents = 0;
    fds.push_back(listenPfd);
    
    // Add client sockets
    {
      std::lock_guard<std::mutex> lock(clientsMutex_);
      for (int clientFd : clientFds_) {
        struct pollfd pfd;
        pfd.fd = clientFd;
        pfd.events = POLLIN;
        pfd.revents = 0;
        fds.push_back(pfd);
      }
    }
    
    int ret = poll(fds.data(), fds.size(), POLL_TIMEOUT_MS);
    if (ret < 0) {
      if (errno == EINTR) continue;
      vlog.error("poll() failed: %s", strerror(errno));
      break;
    }
    
    if (ret == 0) continue;  // Timeout
    
    // Check listen socket for new connections
    if (fds[0].revents & POLLIN) {
      int clientFd = accept(listenFd_, nullptr, nullptr);
      if (clientFd >= 0) {
        setNonBlocking(clientFd);
        std::lock_guard<std::mutex> lock(clientsMutex_);
        clientFds_.push_back(clientFd);
        clientBuffers_[clientFd] = std::vector<uint8_t>();
        vlog.debug("Accepted slave connection (fd=%d)", clientFd);
      }
    }
    
    // Check client sockets for data
    for (size_t i = 1; i < fds.size(); i++) {
      if (fds[i].revents & (POLLIN | POLLERR | POLLHUP)) {
        handleClient(fds[i].fd);
      }
    }
  }
}

void MasterCoordinator::handleClient(int clientFd) {
  // Read available data
  uint8_t buf[8192];
  ssize_t n = recv(clientFd, buf, sizeof(buf), 0);
  
  if (n <= 0) {
    if (n == 0 || (errno != EAGAIN && errno != EWOULDBLOCK)) {
      vlog.debug("Client disconnected (fd=%d)", clientFd);
      removeClient(clientFd);
      return;
    }
    return;
  }
  
  // Append to buffer
  std::vector<uint8_t>& buffer = clientBuffers_[clientFd];
  buffer.insert(buffer.end(), buf, buf + n);
  
  // Parse complete messages
  while (!buffer.empty()) {
    CoordMessage msg;
    int consumed = CoordMessage::parse(buffer.data(), buffer.size(), msg);
    
    if (consumed < 0) {
      vlog.error("Invalid message from client (fd=%d)", clientFd);
      removeClient(clientFd);
      return;
    }
    
    if (consumed == 0) break;  // Incomplete message
    
    buffer.erase(buffer.begin(), buffer.begin() + consumed);
    
    // Handle message
    switch (msg.type()) {
      case CoordMsgType::HELLO:
        sendWelcome(clientFd);
        break;
        
      case CoordMsgType::WRITE_REQ:
        handleWriteRequest(clientFd, msg);
        break;
        
      case CoordMsgType::SLAVE_EXIT:
        handleSlaveExit(clientFd);
        return;
        
      case CoordMsgType::PING: {
        CoordMessage pong(CoordMsgType::PONG);
        sendMessage(clientFd, pong);
        break;
      }
      
      case CoordMsgType::QUERY_INDEX: {
        QueryIndexPayload query;
        if (msg.readStruct(0, query)) {
          // TODO: Implement index query
          QueryRespPayload resp;
          resp.found = 0;
          CoordMessage respMsg(CoordMsgType::QUERY_RESP);
          respMsg.appendStruct(resp);
          sendMessage(clientFd, respMsg);
        }
        break;
      }
      
      default:
        vlog.debug("Unknown message type %d from client", (int)msg.type());
        break;
    }
  }
}

bool MasterCoordinator::sendMessage(int fd, const CoordMessage& msg) {
  std::vector<uint8_t> data = msg.serialize();
  size_t sent = 0;
  while (sent < data.size()) {
    ssize_t n = send(fd, data.data() + sent, data.size() - sent, MSG_NOSIGNAL);
    if (n < 0) {
      if (errno == EINTR) continue;
      return false;
    }
    sent += n;
  }
  return true;
}

bool MasterCoordinator::sendWelcome(int clientFd) {
  CoordMessage msg(CoordMsgType::WELCOME);
  
  WelcomeHeader header;
  memset(&header, 0, sizeof(header));
  header.protocolVersion = COORDINATOR_PROTOCOL_VERSION;
  header.masterPid = getpid();
  header.entryCount = 0;  // TODO: Send current index snapshot
  header.currentShardId = 0;
  
  msg.appendStruct(header);
  
  // TODO: Append index entries
  
  bool ok = sendMessage(clientFd, msg);
  if (ok) {
    std::lock_guard<std::mutex> lock(statsMutex_);
    stats_.connectedSlaves++;
  }
  return ok;
}

void MasterCoordinator::handleWriteRequest(int clientFd, const CoordMessage& msg) {
  WriteReqHeader reqHeader;
  if (!msg.readStruct(0, reqHeader)) {
    vlog.error("Invalid WRITE_REQ from client");
    return;
  }
  
  // Extract payload
  size_t payloadOffset = sizeof(WriteReqHeader);
  if (msg.payload().size() < payloadOffset + reqHeader.payloadLength) {
    vlog.error("WRITE_REQ payload truncated");
    return;
  }
  
  std::vector<uint8_t> payload(
      msg.payload().begin() + payloadOffset,
      msg.payload().begin() + payloadOffset + reqHeader.payloadLength);
  
  // Call the write callback to actually write to shard
  WireIndexEntry resultEntry;
  bool success = false;
  if (writeRequestCallback_) {
    success = writeRequestCallback_(reqHeader.entry, payload, resultEntry);
  }
  
  if (success) {
    // Send ACK to requesting slave
    CoordMessage ackMsg(CoordMsgType::WRITE_ACK);
    WriteAckPayload ack;
    ack.entry = resultEntry;
    ack.requestId = 0;
    ackMsg.appendStruct(ack);
    sendMessage(clientFd, ackMsg);
    
    // Broadcast INDEX_UPDATE to all other slaves
    {
      std::lock_guard<std::mutex> lock(clientsMutex_);
      for (int fd : clientFds_) {
        if (fd != clientFd) {
          CoordMessage updateMsg(CoordMsgType::INDEX_UPDATE);
          IndexUpdateHeader updateHeader;
          updateHeader.entryCount = 1;
          updateHeader.sequenceNum = 0;  // TODO: Track sequence
          updateMsg.appendStruct(updateHeader);
          updateMsg.appendStruct(resultEntry);
          sendMessage(fd, updateMsg);
        }
      }
    }
    
    // Update stats
    {
      std::lock_guard<std::mutex> lock(statsMutex_);
      stats_.writeRequestsRecv++;
      stats_.bytesWrittenForSlaves += payload.size();
      stats_.indexUpdatesSent++;
    }
  } else {
    // Send NACK
    CoordMessage nackMsg(CoordMsgType::WRITE_NACK);
    sendMessage(clientFd, nackMsg);
  }
}

void MasterCoordinator::handleSlaveExit(int clientFd) {
  vlog.debug("Slave announced exit (fd=%d)", clientFd);
  removeClient(clientFd);
}

void MasterCoordinator::removeClient(int clientFd) {
  std::lock_guard<std::mutex> lock(clientsMutex_);
  close(clientFd);
  clientFds_.erase(std::remove(clientFds_.begin(), clientFds_.end(), clientFd), 
                   clientFds_.end());
  clientBuffers_.erase(clientFd);
  
  std::lock_guard<std::mutex> statsLock(statsMutex_);
  if (stats_.connectedSlaves > 0) stats_.connectedSlaves--;
}

bool MasterCoordinator::requestWrite(const WireIndexEntry&,
                                     const std::vector<uint8_t>&,
                                     WireIndexEntry&) {
  // Master doesn't need to request writes - it writes directly
  return false;
}

void MasterCoordinator::broadcastIndexUpdate(const std::vector<WireIndexEntry>& entries) {
  if (entries.empty()) return;
  
  CoordMessage msg(CoordMsgType::INDEX_UPDATE);
  IndexUpdateHeader header;
  header.entryCount = entries.size();
  header.sequenceNum = 0;  // TODO: Track sequence
  msg.appendStruct(header);
  
  for (const auto& entry : entries) {
    msg.appendStruct(entry);
  }
  
  std::lock_guard<std::mutex> lock(clientsMutex_);
  for (int fd : clientFds_) {
    sendMessage(fd, msg);
  }
  
  {
    std::lock_guard<std::mutex> statsLock(statsMutex_);
    stats_.indexUpdatesSent++;
  }
}

bool MasterCoordinator::queryIndex(const uint8_t*, uint16_t, uint16_t, WireIndexEntry&) {
  // Master has direct access to index
  return false;
}

CacheCoordinator::Stats MasterCoordinator::getStats() const {
  std::lock_guard<std::mutex> lock(statsMutex_);
  return stats_;
}

// =============================================================================
// SlaveCoordinator Implementation
// =============================================================================

SlaveCoordinator::SlaveCoordinator(const std::string& cacheDir,
                                   IndexUpdateCallback indexUpdateCb,
                                   WriteRequestCallback writeRequestCb)
  : CacheCoordinator(cacheDir, std::move(indexUpdateCb), std::move(writeRequestCb)),
    socketFd_(-1), running_(false), stopRequested_(false),
    writeAckReceived_(false), lastWriteSuccess_(false) {
  memset(&stats_, 0, sizeof(stats_));
}

SlaveCoordinator::~SlaveCoordinator() {
  stop();
}

bool SlaveCoordinator::start() {
  if (running_) return true;
  
  if (!connectToMaster()) {
    return false;
  }
  
  running_ = true;
  stopRequested_ = false;
  readerThread_.reset(new std::thread(&SlaveCoordinator::readerThread, this));
  
  vlog.info("Slave coordinator connected to master");
  return true;
}

void SlaveCoordinator::stop() {
  if (!running_) return;
  
  stopRequested_ = true;
  
  // Send SLAVE_EXIT to master
  {
    std::lock_guard<std::mutex> lock(socketMutex_);
    if (socketFd_ >= 0) {
      CoordMessage exitMsg(CoordMsgType::SLAVE_EXIT);
      sendMessage(exitMsg);
    }
  }
  
  if (readerThread_ && readerThread_->joinable()) {
    readerThread_->join();
  }
  readerThread_.reset();
  
  {
    std::lock_guard<std::mutex> lock(socketMutex_);
    if (socketFd_ >= 0) {
      close(socketFd_);
      socketFd_ = -1;
    }
  }
  
  running_ = false;
  vlog.info("Slave coordinator stopped");
}

bool SlaveCoordinator::connectToMaster() {
  std::string sockPath = getCoordinatorSocketPath(cacheDir_);
  
  socketFd_ = socket(AF_UNIX, SOCK_STREAM, 0);
  if (socketFd_ < 0) {
    vlog.error("Failed to create socket: %s", strerror(errno));
    return false;
  }
  
  struct sockaddr_un addr;
  memset(&addr, 0, sizeof(addr));
  addr.sun_family = AF_UNIX;
  strncpy(addr.sun_path, sockPath.c_str(), sizeof(addr.sun_path) - 1);
  
  // Set timeout for connect
  struct timeval tv;
  tv.tv_sec = CONNECT_TIMEOUT_MS / 1000;
  tv.tv_usec = (CONNECT_TIMEOUT_MS % 1000) * 1000;
  setsockopt(socketFd_, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));
  setsockopt(socketFd_, SOL_SOCKET, SO_SNDTIMEO, &tv, sizeof(tv));
  
  if (connect(socketFd_, (struct sockaddr*)&addr, sizeof(addr)) < 0) {
    vlog.error("Failed to connect to master at %s: %s", sockPath.c_str(), strerror(errno));
    close(socketFd_);
    socketFd_ = -1;
    return false;
  }
  
  // Send HELLO
  CoordMessage hello(CoordMsgType::HELLO);
  HelloPayload helloPayload;
  helloPayload.protocolVersion = COORDINATOR_PROTOCOL_VERSION;
  helloPayload.pid = getpid();
  memset(helloPayload.reserved, 0, sizeof(helloPayload.reserved));
  hello.appendStruct(helloPayload);
  
  if (!sendMessage(hello)) {
    close(socketFd_);
    socketFd_ = -1;
    return false;
  }
  
  return true;
}

void SlaveCoordinator::readerThread() {
  uint8_t buf[8192];
  
  while (!stopRequested_) {
    struct pollfd pfd;
    pfd.fd = socketFd_;
    pfd.events = POLLIN;
    pfd.revents = 0;
    
    int ret = poll(&pfd, 1, POLL_TIMEOUT_MS);
    if (ret < 0) {
      if (errno == EINTR) continue;
      vlog.error("poll() failed: %s", strerror(errno));
      break;
    }
    
    if (ret == 0) continue;  // Timeout
    
    if (pfd.revents & (POLLERR | POLLHUP)) {
      handleMasterExit();
      break;
    }
    
    if (pfd.revents & POLLIN) {
      ssize_t n = recv(socketFd_, buf, sizeof(buf), 0);
      if (n <= 0) {
        if (n == 0 || (errno != EAGAIN && errno != EWOULDBLOCK)) {
          handleMasterExit();
          break;
        }
        continue;
      }
      
      std::lock_guard<std::mutex> lock(socketMutex_);
      recvBuffer_.insert(recvBuffer_.end(), buf, buf + n);
      
      // Parse complete messages
      while (!recvBuffer_.empty()) {
        CoordMessage msg;
        int consumed = CoordMessage::parse(recvBuffer_.data(), recvBuffer_.size(), msg);
        
        if (consumed < 0) {
          vlog.error("Invalid message from master");
          break;
        }
        
        if (consumed == 0) break;  // Incomplete
        
        recvBuffer_.erase(recvBuffer_.begin(), recvBuffer_.begin() + consumed);
        handleMessage(msg);
      }
    }
  }
}

void SlaveCoordinator::handleMessage(const CoordMessage& msg) {
  switch (msg.type()) {
    case CoordMsgType::WELCOME:
      handleWelcome(msg);
      break;
      
    case CoordMsgType::WRITE_ACK:
      handleWriteAck(msg);
      break;
      
    case CoordMsgType::WRITE_NACK:
      {
        std::lock_guard<std::mutex> lock(writeMutex_);
        lastWriteSuccess_ = false;
        writeAckReceived_ = true;
        writeCond_.notify_all();
      }
      break;
      
    case CoordMsgType::INDEX_UPDATE:
      handleIndexUpdate(msg);
      break;
      
    case CoordMsgType::MASTER_EXIT:
      handleMasterExit();
      break;
      
    case CoordMsgType::PONG:
      // Keepalive response - nothing to do
      break;
      
    default:
      vlog.debug("Unknown message type %d from master", (int)msg.type());
      break;
  }
}

void SlaveCoordinator::handleWelcome(const CoordMessage& msg) {
  WelcomeHeader header;
  if (!msg.readStruct(0, header)) {
    vlog.error("Invalid WELCOME message");
    return;
  }
  
  vlog.info("Connected to master (PID %d), protocol v%d, %u entries",
            header.masterPid, header.protocolVersion, header.entryCount);
  
  // Parse index entries
  if (header.entryCount > 0 && indexUpdateCallback_) {
    std::vector<WireIndexEntry> entries;
    size_t offset = sizeof(WelcomeHeader);
    for (uint32_t i = 0; i < header.entryCount; i++) {
      WireIndexEntry entry;
      if (!msg.readStruct(offset, entry)) break;
      entries.push_back(entry);
      offset += sizeof(WireIndexEntry);
    }
    if (!entries.empty()) {
      indexUpdateCallback_(entries);
    }
  }
}

void SlaveCoordinator::handleWriteAck(const CoordMessage& msg) {
  WriteAckPayload ack;
  if (!msg.readStruct(0, ack)) {
    vlog.error("Invalid WRITE_ACK message");
    return;
  }
  
  std::lock_guard<std::mutex> lock(writeMutex_);
  lastWriteAck_ = ack.entry;
  lastWriteSuccess_ = true;
  writeAckReceived_ = true;
  writeCond_.notify_all();
}

void SlaveCoordinator::handleIndexUpdate(const CoordMessage& msg) {
  IndexUpdateHeader header;
  if (!msg.readStruct(0, header)) {
    vlog.error("Invalid INDEX_UPDATE message");
    return;
  }
  
  std::vector<WireIndexEntry> entries;
  size_t offset = sizeof(IndexUpdateHeader);
  for (uint32_t i = 0; i < header.entryCount; i++) {
    WireIndexEntry entry;
    if (!msg.readStruct(offset, entry)) break;
    entries.push_back(entry);
    offset += sizeof(WireIndexEntry);
  }
  
  if (!entries.empty() && indexUpdateCallback_) {
    indexUpdateCallback_(entries);
    
    std::lock_guard<std::mutex> lock(statsMutex_);
    stats_.indexUpdatesRecv++;
  }
}

void SlaveCoordinator::handleMasterExit() {
  vlog.info("Master exited; attempting election...");
  
  // Close current connection
  {
    std::lock_guard<std::mutex> lock(socketMutex_);
    if (socketFd_ >= 0) {
      close(socketFd_);
      socketFd_ = -1;
    }
  }
  
  // Wake up any pending write requests
  {
    std::lock_guard<std::mutex> lock(writeMutex_);
    lastWriteSuccess_ = false;
    writeAckReceived_ = true;
    writeCond_.notify_all();
  }
  
  // TODO: Implement election - for now, fall back to standalone
  running_ = false;
}

bool SlaveCoordinator::attemptElection() {
  // TODO: Implement master election
  return false;
}

bool SlaveCoordinator::sendMessage(const CoordMessage& msg) {
  std::vector<uint8_t> data = msg.serialize();
  size_t sent = 0;
  while (sent < data.size()) {
    ssize_t n = send(socketFd_, data.data() + sent, data.size() - sent, MSG_NOSIGNAL);
    if (n < 0) {
      if (errno == EINTR) continue;
      return false;
    }
    sent += n;
  }
  return true;
}

bool SlaveCoordinator::requestWrite(const WireIndexEntry& entry,
                                    const std::vector<uint8_t>& payload,
                                    WireIndexEntry& resultEntry) {
  if (!running_ || socketFd_ < 0) return false;
  
  // Build WRITE_REQ message
  CoordMessage msg(CoordMsgType::WRITE_REQ);
  WriteReqHeader header;
  header.entry = entry;
  header.payloadLength = payload.size();
  msg.appendStruct(header);
  msg.payload().insert(msg.payload().end(), payload.begin(), payload.end());
  
  // Send request
  {
    std::lock_guard<std::mutex> lock(socketMutex_);
    writeAckReceived_ = false;
    if (!sendMessage(msg)) {
      return false;
    }
  }
  
  // Wait for ACK/NACK with timeout
  {
    std::unique_lock<std::mutex> lock(writeMutex_);
    auto timeout = std::chrono::milliseconds(WRITE_REQUEST_TIMEOUT_MS);
    if (!writeCond_.wait_for(lock, timeout, [this] { return writeAckReceived_; })) {
      vlog.error("Write request timed out");
      return false;
    }
    
    if (lastWriteSuccess_) {
      resultEntry = lastWriteAck_;
      
      std::lock_guard<std::mutex> statsLock(statsMutex_);
      stats_.writeRequestsSent++;
      return true;
    }
  }
  
  return false;
}

void SlaveCoordinator::broadcastIndexUpdate(const std::vector<WireIndexEntry>&) {
  // Slaves don't broadcast - only master does
}

bool SlaveCoordinator::queryIndex(const uint8_t* hash, uint16_t width, uint16_t height,
                                  WireIndexEntry& /*entry*/) {
  if (!running_ || socketFd_ < 0) return false;
  
  CoordMessage msg(CoordMsgType::QUERY_INDEX);
  QueryIndexPayload query;
  memcpy(query.hash, hash, 16);
  query.width = width;
  query.height = height;
  msg.appendStruct(query);
  
  // TODO: Implement synchronous query with timeout
  // For now, return false (caller will fetch from server)
  return false;
}

CacheCoordinator::Stats SlaveCoordinator::getStats() const {
  std::lock_guard<std::mutex> lock(statsMutex_);
  return stats_;
}

}  // namespace cache
}  // namespace rfb
