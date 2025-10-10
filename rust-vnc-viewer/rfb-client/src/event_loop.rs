//! Event loop coordination: read loop, write loop, and reconnection logic.

use crate::{errors::RfbClientError, messages::{ClientCommand, ServerEvent}, Config};
use tokio::task::JoinHandle;

// Stub - to be implemented
pub async fn spawn(
    _config: Config,
    _commands: flume::Receiver<ClientCommand>,
    _events: flume::Sender<ServerEvent>,
) -> Result<JoinHandle<()>, RfbClientError> {
    unimplemented!("event_loop::spawn - Phase 4 implementation pending")
}
