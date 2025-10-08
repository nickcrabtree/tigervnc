//! RFB connection state machine.
//!
//! This module manages the connection state and lifecycle for RFB (Remote Framebuffer)
//! protocol connections. It handles the state transitions from initial connection through
//! handshake to normal operation.
//!
//! # Connection Lifecycle
//!
//! The typical connection flow is:
//!
//! 1. **Disconnected** - No connection established
//! 2. **ProtocolVersion** - Negotiating RFB protocol version
//! 3. **Security** - Negotiating security type
//! 4. **SecurityResult** - Waiting for authentication result  
//! 5. **ClientInit** - Sending client initialization
//! 6. **ServerInit** - Receiving server initialization
//! 7. **Normal** - Normal operation (sending/receiving messages)
//! 8. **Closing** - Connection being closed
//! 9. **Closed** - Connection fully closed
//!
//! # Examples
//!
//! ```no_run
//! use rfb_protocol::{TcpSocket, connection::{RfbConnection, ConnectionState}};
//!
//! # async fn example() -> anyhow::Result<()> {
//! let socket = TcpSocket::connect("localhost", 5900).await?;
//! let (reader, writer) = tokio::io::split(socket);
//! let mut conn = RfbConnection::new(reader, writer);
//!
//! // Check initial state
//! assert_eq!(conn.state(), ConnectionState::Disconnected);
//!
//! // Perform handshake (would transition through multiple states)
//! // conn.handshake().await?;
//! # Ok(())
//! # }
//! ```

use crate::io::{RfbInStream, RfbOutStream};
use tokio::io::{AsyncRead, AsyncWrite};
use std::fmt;

/// Connection state for the RFB protocol state machine.
///
/// The connection progresses through these states during the handshake,
/// and may transition to `Closing` or `Closed` at any point due to errors
/// or explicit disconnection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectionState {
    /// No connection established yet.
    Disconnected,
    
    /// Negotiating protocol version (ProtocolVersion handshake).
    ProtocolVersion,
    
    /// Negotiating security type.
    Security,
    
    /// Waiting for security handshake result.
    SecurityResult,
    
    /// Sending ClientInit message.
    ClientInit,
    
    /// Receiving ServerInit message.
    ServerInit,
    
    /// Normal operation - exchanging client/server messages.
    Normal,
    
    /// Connection is being closed gracefully.
    Closing,
    
    /// Connection is fully closed.
    Closed,
    
    /// Invalid state (error condition).
    Invalid,
}

impl fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disconnected => write!(f, "Disconnected"),
            Self::ProtocolVersion => write!(f, "ProtocolVersion"),
            Self::Security => write!(f, "Security"),
            Self::SecurityResult => write!(f, "SecurityResult"),
            Self::ClientInit => write!(f, "ClientInit"),
            Self::ServerInit => write!(f, "ServerInit"),
            Self::Normal => write!(f, "Normal"),
            Self::Closing => write!(f, "Closing"),
            Self::Closed => write!(f, "Closed"),
            Self::Invalid => write!(f, "Invalid"),
        }
    }
}

/// RFB connection manager.
///
/// Manages an RFB protocol connection, including state transitions,
/// I/O streams, and connection parameters.
///
/// # Type Parameters
///
/// * `R` - The reader type implementing [`AsyncRead`]
/// * `W` - The writer type implementing [`AsyncWrite`]
///
/// # Examples
///
/// ```no_run
/// use rfb_protocol::{TcpSocket, connection::RfbConnection};
///
/// # async fn example() -> anyhow::Result<()> {
/// let socket = TcpSocket::connect("192.168.1.100", 5900).await?;
/// let (reader, writer) = tokio::io::split(socket);
/// let conn = RfbConnection::new(reader, writer);
/// 
/// println!("Connected to: {}", conn.peer_address());
/// # Ok(())
/// # }
/// ```
pub struct RfbConnection<R, W> {
    /// Input stream for reading from the server.
    instream: RfbInStream<R>,
    
    /// Output stream for writing to the server.
    outstream: RfbOutStream<W>,
    
    /// Current connection state.
    state: ConnectionState,
    
    /// Peer address (for logging/display).
    peer_address: String,
    
    /// Server name/description.
    server_name: Option<String>,
    
    /// Framebuffer width (set after ServerInit).
    width: Option<u16>,
    
    /// Framebuffer height (set after ServerInit).
    height: Option<u16>,
}

impl<R: AsyncRead + Unpin, W: AsyncWrite + Unpin> RfbConnection<R, W> {
    /// Create a new RFB connection from separate reader and writer.
    ///
    /// The connection starts in the `Disconnected` state. Use
    /// [`begin_handshake()`](Self::begin_handshake) to start the
    /// RFB protocol handshake.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use rfb_protocol::{TcpSocket, connection::RfbConnection};
    ///
    /// # async fn example() -> anyhow::Result<()> {
    /// let socket = TcpSocket::connect("localhost", 5900).await?;
    /// let (reader, writer) = tokio::io::split(socket);
    /// let mut conn = RfbConnection::new(reader, writer);
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            instream: RfbInStream::new(reader),
            outstream: RfbOutStream::new(writer),
            state: ConnectionState::Disconnected,
            peer_address: String::new(),
            server_name: None,
            width: None,
            height: None,
        }
    }
    
    /// Set the peer address (for display purposes).
    pub fn set_peer_address(&mut self, address: String) {
        self.peer_address = address;
    }

    /// Get the current connection state.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use rfb_protocol::{TcpSocket, connection::{RfbConnection, ConnectionState}};
    /// # async fn example() -> anyhow::Result<()> {
    /// # let socket = TcpSocket::connect("localhost", 5900).await?;
    /// let (reader, writer) = tokio::io::split(socket);
    /// let conn = RfbConnection::new(reader, writer);
    /// assert_eq!(conn.state(), ConnectionState::Disconnected);
    /// # Ok(())
    /// # }
    /// ```
    pub fn state(&self) -> ConnectionState {
        self.state
    }

    /// Check if the connection is in a specific state.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use rfb_protocol::{TcpSocket, connection::{RfbConnection, ConnectionState}};
    /// # async fn example() -> anyhow::Result<()> {
    /// # let socket = TcpSocket::connect("localhost", 5900).await?;
    /// let (reader, writer) = tokio::io::split(socket);
    /// let conn = RfbConnection::new(reader, writer);
    /// if conn.is_state(ConnectionState::Normal) {
    ///     // Connection is ready for normal operation
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn is_state(&self, state: ConnectionState) -> bool {
        self.state == state
    }

    /// Check if the connection is active (not closed or closing).
    ///
    /// Returns `true` if the connection state is anything except
    /// `Closing`, `Closed`, or `Invalid`.
    pub fn is_active(&self) -> bool {
        !matches!(
            self.state,
            ConnectionState::Closing | ConnectionState::Closed | ConnectionState::Invalid
        )
    }

    /// Check if the connection is ready for normal operation.
    ///
    /// Returns `true` only when in the `Normal` state.
    pub fn is_ready(&self) -> bool {
        self.state == ConnectionState::Normal
    }

    /// Get the peer address.
    ///
    /// Returns the endpoint string (e.g., "192.168.1.100:5900").
    pub fn peer_address(&self) -> &str {
        &self.peer_address
    }

    /// Get the server name, if available.
    ///
    /// Returns `Some(&str)` after receiving ServerInit, `None` before.
    pub fn server_name(&self) -> Option<&str> {
        self.server_name.as_deref()
    }

    /// Get the framebuffer dimensions, if available.
    ///
    /// Returns `Some((width, height))` after receiving ServerInit, `None` before.
    pub fn dimensions(&self) -> Option<(u16, u16)> {
        self.width.and_then(|w| self.height.map(|h| (w, h)))
    }

    /// Begin the RFB protocol handshake.
    ///
    /// This transitions the connection from `Disconnected` to `ProtocolVersion`
    /// state. Actual protocol negotiation will be implemented in future tasks.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection is not in the `Disconnected` state.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use rfb_protocol::{TcpSocket, connection::{RfbConnection, ConnectionState}};
    /// # async fn example() -> anyhow::Result<()> {
    /// # let socket = TcpSocket::connect("localhost", 5900).await?;
    /// let (reader, writer) = tokio::io::split(socket);
    /// let mut conn = RfbConnection::new(reader, writer);
    /// conn.begin_handshake()?;
    /// assert_eq!(conn.state(), ConnectionState::ProtocolVersion);
    /// # Ok(())
    /// # }
    /// ```
    pub fn begin_handshake(&mut self) -> anyhow::Result<()> {
        if self.state != ConnectionState::Disconnected {
            anyhow::bail!(
                "Cannot begin handshake from state: {}",
                self.state
            );
        }
        
        self.state = ConnectionState::ProtocolVersion;
        Ok(())
    }

    /// Transition to a new connection state.
    ///
    /// This performs basic validation to ensure state transitions are valid.
    ///
    /// # Errors
    ///
    /// Returns an error if the state transition is invalid (e.g., trying to
    /// go from `Closed` to `Normal`).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use rfb_protocol::{TcpSocket, connection::{RfbConnection, ConnectionState}};
    /// # async fn example() -> anyhow::Result<()> {
    /// # let socket = TcpSocket::connect("localhost", 5900).await?;
    /// let (reader, writer) = tokio::io::split(socket);
    /// let mut conn = RfbConnection::new(reader, writer);
    /// conn.transition_to(ConnectionState::ProtocolVersion)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn transition_to(&mut self, new_state: ConnectionState) -> anyhow::Result<()> {
        // Validate state transition
        match (self.state, new_state) {
            // Can't transition from closed states
            (ConnectionState::Closed, _) => {
                anyhow::bail!("Cannot transition from Closed state");
            }
            (ConnectionState::Invalid, _) => {
                anyhow::bail!("Cannot transition from Invalid state");
            }
            
            // Can always close
            (_, ConnectionState::Closing | ConnectionState::Closed | ConnectionState::Invalid) => {},
            
            // Normal forward progression
            (ConnectionState::Disconnected, ConnectionState::ProtocolVersion) => {},
            (ConnectionState::ProtocolVersion, ConnectionState::Security) => {},
            (ConnectionState::Security, ConnectionState::SecurityResult) => {},
            (ConnectionState::SecurityResult, ConnectionState::ClientInit) => {},
            (ConnectionState::ClientInit, ConnectionState::ServerInit) => {},
            (ConnectionState::ServerInit, ConnectionState::Normal) => {},
            
            // Stay in Normal state
            (ConnectionState::Normal, ConnectionState::Normal) => {},
            
            // Invalid transition
            _ => {
                anyhow::bail!(
                    "Invalid state transition: {} -> {}",
                    self.state,
                    new_state
                );
            }
        }
        
        self.state = new_state;
        Ok(())
    }

    /// Set the server name (called after ServerInit).
    pub fn set_server_name(&mut self, name: String) {
        self.server_name = Some(name);
    }

    /// Set the framebuffer dimensions (called after ServerInit).
    pub fn set_dimensions(&mut self, width: u16, height: u16) {
        self.width = Some(width);
        self.height = Some(height);
    }

    /// Get a reference to the input stream.
    ///
    /// This allows reading from the connection directly if needed.
    pub fn instream(&mut self) -> &mut RfbInStream<R> {
        &mut self.instream
    }

    /// Get a reference to the output stream.
    ///
    /// This allows writing to the connection directly if needed.
    pub fn outstream(&mut self) -> &mut RfbOutStream<W> {
        &mut self.outstream
    }

    /// Close the connection.
    ///
    /// Transitions to the `Closing` state. The actual socket close
    /// will be handled when the connection is dropped.
    pub fn close(&mut self) {
        if self.state != ConnectionState::Closed {
            self.state = ConnectionState::Closing;
        }
    }

    /// Mark the connection as closed.
    ///
    /// This should be called after the socket has been fully closed.
    pub fn mark_closed(&mut self) {
        self.state = ConnectionState::Closed;
    }
}

// Note: We can't implement Drop with trait bounds on R and W
// because it requires those bounds on the struct itself.
// The connection cleanup will happen naturally when the streams are dropped.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TcpSocket, VncSocket};
    use tokio::net::TcpListener;

    async fn create_test_connection() -> (RfbConnection<tokio::io::ReadHalf<TcpSocket>, tokio::io::WriteHalf<TcpSocket>>, u16) {
        // Start a test server
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        
        // Spawn server task that just accepts and holds the connection
        tokio::spawn(async move {
            let (_socket, _addr) = listener.accept().await.unwrap();
            // Hold connection open
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
        });
        
        // Connect client
        let socket = TcpSocket::connect("127.0.0.1", port).await.unwrap();
        let peer_addr = socket.peer_endpoint();
        let (reader, writer) = tokio::io::split(socket);
        let mut conn = RfbConnection::new(reader, writer);
        conn.set_peer_address(peer_addr);
        
        (conn, port)
    }

    #[tokio::test]
    async fn test_initial_state() {
        let (conn, _port) = create_test_connection().await;
        assert_eq!(conn.state(), ConnectionState::Disconnected);
        assert!(!conn.is_ready());
        assert!(conn.is_active());
    }

    #[tokio::test]
    async fn test_begin_handshake() {
        let (mut conn, _port) = create_test_connection().await;
        
        conn.begin_handshake().unwrap();
        assert_eq!(conn.state(), ConnectionState::ProtocolVersion);
        
        // Can't begin handshake twice
        assert!(conn.begin_handshake().is_err());
    }

    #[tokio::test]
    async fn test_state_transitions() {
        let (mut conn, _port) = create_test_connection().await;
        
        // Valid progression
        assert!(conn.transition_to(ConnectionState::ProtocolVersion).is_ok());
        assert!(conn.transition_to(ConnectionState::Security).is_ok());
        assert!(conn.transition_to(ConnectionState::SecurityResult).is_ok());
        assert!(conn.transition_to(ConnectionState::ClientInit).is_ok());
        assert!(conn.transition_to(ConnectionState::ServerInit).is_ok());
        assert!(conn.transition_to(ConnectionState::Normal).is_ok());
        
        assert!(conn.is_ready());
    }

    #[tokio::test]
    async fn test_invalid_transitions() {
        let (mut conn, _port) = create_test_connection().await;
        
        // Can't skip states
        assert!(conn.transition_to(ConnectionState::Normal).is_err());
        
        // Can't go backwards
        conn.transition_to(ConnectionState::ProtocolVersion).unwrap();
        assert!(conn.transition_to(ConnectionState::Disconnected).is_err());
    }

    #[tokio::test]
    async fn test_close_from_any_state() {
        let (mut conn, _port) = create_test_connection().await;
        
        // Can close from initial state
        assert!(conn.transition_to(ConnectionState::Closing).is_ok());
        
        // Reset for another test - progress through states to Normal
        let (mut conn, _port) = create_test_connection().await;
        conn.transition_to(ConnectionState::ProtocolVersion).unwrap();
        conn.transition_to(ConnectionState::Security).unwrap();
        conn.transition_to(ConnectionState::SecurityResult).unwrap();
        conn.transition_to(ConnectionState::ClientInit).unwrap();
        conn.transition_to(ConnectionState::ServerInit).unwrap();
        conn.transition_to(ConnectionState::Normal).unwrap();
        
        // Can close from Normal
        assert!(conn.transition_to(ConnectionState::Closing).is_ok());
    }

    #[tokio::test]
    async fn test_closed_state_is_final() {
        let (mut conn, _port) = create_test_connection().await;
        
        conn.transition_to(ConnectionState::Closed).unwrap();
        
        // Can't transition from Closed
        assert!(conn.transition_to(ConnectionState::Normal).is_err());
        assert!(conn.transition_to(ConnectionState::ProtocolVersion).is_err());
        
        assert!(!conn.is_active());
    }

    #[tokio::test]
    async fn test_peer_address() {
        let (conn, port) = create_test_connection().await;
        
        let addr = conn.peer_address();
        assert!(addr.contains("127.0.0.1"));
        assert!(addr.contains(&port.to_string()));
    }

    #[tokio::test]
    async fn test_server_name() {
        let (mut conn, _port) = create_test_connection().await;
        
        assert_eq!(conn.server_name(), None);
        
        conn.set_server_name("Test Server".to_string());
        assert_eq!(conn.server_name(), Some("Test Server"));
    }

    #[tokio::test]
    async fn test_dimensions() {
        let (mut conn, _port) = create_test_connection().await;
        
        assert_eq!(conn.dimensions(), None);
        
        conn.set_dimensions(1920, 1080);
        assert_eq!(conn.dimensions(), Some((1920, 1080)));
    }

    #[tokio::test]
    async fn test_is_state() {
        let (mut conn, _port) = create_test_connection().await;
        
        assert!(conn.is_state(ConnectionState::Disconnected));
        assert!(!conn.is_state(ConnectionState::Normal));
        
        conn.transition_to(ConnectionState::ProtocolVersion).unwrap();
        assert!(conn.is_state(ConnectionState::ProtocolVersion));
        assert!(!conn.is_state(ConnectionState::Disconnected));
    }

    #[tokio::test]
    async fn test_close_methods() {
        let (mut conn, _port) = create_test_connection().await;
        
        conn.close();
        assert_eq!(conn.state(), ConnectionState::Closing);
        assert!(!conn.is_active());
        
        conn.mark_closed();
        assert_eq!(conn.state(), ConnectionState::Closed);
    }
}
