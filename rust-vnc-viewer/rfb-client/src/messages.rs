//! Application-facing message types for communication between the client and application.

use bytes::Bytes;
use rfb_common::Rect;
use rfb_protocol::messages::PixelFormat;

/// Events sent from the VNC client to the application.
#[derive(Debug, Clone)]
pub enum ServerEvent {
    /// Successfully connected to the server.
    Connected {
        /// Framebuffer width in pixels.
        width: u16,
        /// Framebuffer height in pixels.
        height: u16,
        /// Server name/description.
        name: String,
        /// Negotiated pixel format.
        pixel_format: PixelFormat,
    },

    /// Framebuffer has been updated.
    ///
    /// The `damage` vector contains all rectangles that were updated.
    /// The application should redraw these regions.
    FramebufferUpdated {
        /// List of rectangles that were updated.
        damage: Vec<Rect>,
    },

    /// Desktop size changed.
    ///
    /// This can happen if the server's screen resolution changes.
    DesktopResized {
        /// New width in pixels.
        width: u16,
        /// New height in pixels.
        height: u16,
    },

    // TODO: Add CursorUpdated variant once Cursor type is implemented in rfb-common
    /// Server sent a bell notification.
    Bell,

    /// Server sent clipboard/cut text data.
    ServerCutText {
        /// Clipboard data (typically UTF-8 text).
        text: Bytes,
    },

    /// Connection has been closed (gracefully or due to error).
    ConnectionClosed,

    /// An error occurred.
    ///
    /// After this event, the client may attempt to reconnect (if configured)
    /// or shut down.
    Error {
        /// The error message.
        message: String,
    },
}

/// Commands sent from the application to the VNC client.
#[derive(Debug, Clone)]
pub enum ClientCommand {
    /// Request a framebuffer update.
    RequestUpdate {
        /// If true, only send updates for changed regions.
        /// If false, send the entire specified rectangle.
        incremental: bool,
        /// Rectangle to update. If None, update the entire screen.
        rect: Option<Rect>,
    },

    /// Send pointer (mouse) event.
    Pointer {
        /// X coordinate in pixels.
        x: u16,
        /// Y coordinate in pixels.
        y: u16,
        /// Button mask (bit 0 = left, bit 1 = middle, bit 2 = right).
        buttons: u8,
    },

    /// Send keyboard event.
    Key {
        /// X11 keysym value.
        key: u32,
        /// True if key was pressed, false if released.
        down: bool,
    },

    /// Send clipboard/cut text to server.
    ClientCutText {
        /// Text data to send (typically UTF-8).
        text: Bytes,
    },

    /// Close the connection.
    Close,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_event_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<ServerEvent>();
    }

    #[test]
    fn test_client_command_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<ClientCommand>();
    }

    #[test]
    fn test_client_command_clone() {
        let cmd = ClientCommand::Key {
            key: 0x61,
            down: true,
        };
        let cmd2 = cmd.clone();
        assert!(matches!(
            cmd2,
            ClientCommand::Key {
                key: 0x61,
                down: true
            }
        ));
    }
}
