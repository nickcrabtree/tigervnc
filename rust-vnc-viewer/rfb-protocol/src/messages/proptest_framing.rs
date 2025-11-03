//! Property tests for message framing.
//!
//! These tests verify that RFB message parsing is robust against fragmentation
//! at arbitrary byte boundaries, which is critical for correct operation over
//! real network streams.

#[cfg(test)]
mod tests {
    use super::super::server::*;
    use super::super::types::*;
    use crate::io::{RfbInStream, RfbOutStream};
    use proptest::prelude::*;

    /// A fragmenting reader that splits reads at a specific boundary.
    ///
    /// This simulates network fragmentation by only allowing reads up to
    /// a specific position, then requiring a second read for the rest.
    struct FragmentingReader {
        data: Vec<u8>,
        pos: usize,
        boundary: usize,
    }

    impl FragmentingReader {
        fn new(data: Vec<u8>, boundary: usize) -> Self {
            let boundary = boundary.min(data.len());
            Self {
                data,
                pos: 0,
                boundary,
            }
        }
    }

    impl tokio::io::AsyncRead for FragmentingReader {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            if self.pos >= self.data.len() {
                return std::task::Poll::Ready(Ok(()));
            }

            // Only read up to boundary on first pass, or remaining data after
            let available = if self.pos < self.boundary {
                (self.boundary - self.pos).min(buf.remaining())
            } else {
                (self.data.len() - self.pos).min(buf.remaining())
            };

            if available == 0 {
                return std::task::Poll::Ready(Ok(()));
            }

            let data = &self.data[self.pos..self.pos + available];
            buf.put_slice(data);
            self.pos += available;

            std::task::Poll::Ready(Ok(()))
        }
    }

    // Property test strategies
    fn arbitrary_pixel_format() -> impl Strategy<Value = PixelFormat> {
        (
            prop::sample::select(vec![8u8, 16, 24, 32]),
            prop::sample::select(vec![8u8, 16, 24]),
            prop::bool::ANY,
            prop::sample::select(vec![15u16, 31, 63, 127, 255]),
        )
            .prop_map(|(bpp, depth, big_endian, max)| PixelFormat {
                bits_per_pixel: bpp,
                depth,
                big_endian: if big_endian { 1 } else { 0 },
                true_color: 1,
                red_max: max,
                green_max: max,
                blue_max: max,
                red_shift: 0,
                green_shift: (bpp / 3) as u8,
                blue_shift: (2 * bpp / 3) as u8,
            })
    }

    fn arbitrary_server_init() -> impl Strategy<Value = ServerInit> {
        (
            1u16..=7680,
            1u16..=4320,
            arbitrary_pixel_format(),
            "[a-zA-Z0-9 ]{0,100}",
        )
            .prop_map(|(width, height, pf, name)| ServerInit {
                framebuffer_width: width,
                framebuffer_height: height,
                pixel_format: pf,
                name,
            })
    }

    fn arbitrary_rectangle() -> impl Strategy<Value = Rectangle> {
        (
            0u16..=1920,
            0u16..=1080,
            1u16..=640,
            1u16..=480,
            prop::sample::select(vec![ENCODING_RAW, ENCODING_COPYRECT, ENCODING_ZRLE]),
        )
            .prop_map(|(x, y, w, h, enc)| Rectangle {
                x,
                y,
                width: w,
                height: h,
                encoding: enc,
            })
    }

    proptest! {
        /// Test ServerInit parsing with fragmentation at every possible byte boundary.
        #[test]
        fn test_server_init_fragmentation(
            server_init in arbitrary_server_init(),
            boundary in 0usize..100
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                // Serialize the message
                let mut buffer = Vec::new();
                let mut out_stream = RfbOutStream::new(&mut buffer);
                server_init.write_to(&mut out_stream).unwrap();
                out_stream.flush().await.unwrap();

                // Parse with fragmentation at boundary
                let boundary = boundary.min(buffer.len());
                let reader = FragmentingReader::new(buffer, boundary);
                let mut in_stream = RfbInStream::new(reader);

                let parsed = ServerInit::read_from(&mut in_stream).await.unwrap();
                prop_assert_eq!(server_init, parsed);
                Ok(())
            })?;
        }

        /// Test FramebufferUpdate header parsing with fragmentation.
        #[test]
        fn test_framebuffer_update_fragmentation(
            rectangles in prop::collection::vec(arbitrary_rectangle(), 0..10),
            boundary in 0usize..500
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let update = FramebufferUpdate { rectangles };

                // Serialize
                let mut buffer = Vec::new();
                let mut out_stream = RfbOutStream::new(&mut buffer);
                update.write_to(&mut out_stream);
                out_stream.flush().await.unwrap();

                // Parse with fragmentation (skip message type byte)
                let boundary = boundary.min(buffer.len() - 1);
                let reader = FragmentingReader::new(buffer[1..].to_vec(), boundary);
                let mut in_stream = RfbInStream::new(reader);

                let parsed = FramebufferUpdate::read_from(&mut in_stream).await.unwrap();
                prop_assert_eq!(update, parsed);
                Ok(())
            })?;
        }

        /// Test SetColorMapEntries parsing with fragmentation.
        #[test]
        fn test_colormap_fragmentation(
            first_color in 0u16..=255,
            num_colors in 0usize..20,
            boundary in 0usize..500
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let colors: Vec<_> = (0..num_colors)
                    .map(|i| ColorMapEntry {
                        red: (i * 1000) as u16,
                        green: (i * 2000) as u16,
                        blue: (i * 3000) as u16,
                    })
                    .collect();

                let msg = SetColorMapEntries {
                    first_color,
                    colors,
                };

                // Serialize
                let mut buffer = Vec::new();
                let mut out_stream = RfbOutStream::new(&mut buffer);
                msg.write_to(&mut out_stream);
                out_stream.flush().await.unwrap();

                // Parse with fragmentation (skip message type)
                let boundary = boundary.min(buffer.len() - 1);
                let reader = FragmentingReader::new(buffer[1..].to_vec(), boundary);
                let mut in_stream = RfbInStream::new(reader);

                let parsed = SetColorMapEntries::read_from(&mut in_stream).await.unwrap();
                prop_assert_eq!(msg, parsed);
                Ok(())
            })?;
        }

        /// Test Bell parsing (simplest case - no body).
        #[test]
        fn test_bell_always_succeeds(boundary in 0usize..10) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let bell = Bell;

                // Serialize
                let mut buffer = Vec::new();
                let mut out_stream = RfbOutStream::new(&mut buffer);
                bell.write_to(&mut out_stream);
                out_stream.flush().await.unwrap();

                // Parse with fragmentation (skip message type)
                let boundary = boundary.min(buffer.len() - 1);
                let reader = FragmentingReader::new(buffer[1..].to_vec(), boundary);
                let mut in_stream = RfbInStream::new(reader);

                let parsed = Bell::read_from(&mut in_stream).await.unwrap();
                prop_assert_eq!(bell, parsed);
                Ok(())
            })?;
        }

        /// Test ServerCutText parsing with various text lengths and boundaries.
        #[test]
        fn test_server_cut_text_fragmentation(
            text in "[\\x20-\\x7E]{0,200}",
            boundary in 0usize..300
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let msg = ServerCutText { text };

                // Serialize
                let mut buffer = Vec::new();
                let mut out_stream = RfbOutStream::new(&mut buffer);
                msg.write_to(&mut out_stream);
                out_stream.flush().await.unwrap();

                // Parse with fragmentation (skip message type)
                let boundary = boundary.min(buffer.len() - 1);
                let reader = FragmentingReader::new(buffer[1..].to_vec(), boundary);
                let mut in_stream = RfbInStream::new(reader);

                let parsed = ServerCutText::read_from(&mut in_stream).await.unwrap();
                prop_assert_eq!(msg, parsed);
                Ok(())
            })?;
        }
    }
}
