//! PersistentCache end-to-end round-trip without a live server.
use std::io::Cursor;
use std::sync::{Arc, Mutex};
use rfb_encodings::{PersistentCachedPixels, PersistentClientCache, PersistentCachedRectDecoder, PersistentCachedRectInitDecoder, PixelFormat, RfbInStream, ENCODING_PERSISTENT_CACHED_RECT, ENCODING_PERSISTENT_CACHED_RECT_INIT, ENCODING_RAW};
use rfb_encodings::Decoder;
use rfb_pixelbuffer::{ManagedPixelBuffer, PixelFormat as LocalPixelFormat};
use rfb_protocol::messages::types::Rectangle;

fn pf_rgb888() -> PixelFormat {
    PixelFormat { bits_per_pixel: 32, depth: 24, big_endian: 0, true_color: 1, red_max: 255, green_max: 255, blue_max: 255, red_shift: 16, green_shift: 8, blue_shift: 0 }
}

#[tokio::test]
async fn persistent_cache_round_trip() {
    let pf = pf_rgb888();
    let pc = Arc::new(Mutex::new(PersistentClientCache::new(1)));
    let misses: Arc<Mutex<Vec<[u8;16]>>> = Arc::new(Mutex::new(Vec::new()));
    let ref_dec = PersistentCachedRectDecoder::new_with_miss_reporter(pc.clone(), misses.clone());
    let init_dec = PersistentCachedRectInitDecoder::new(pc.clone());

    // First reference — expect a miss
    let miss_id = [0x11u8; 16];
    let mut payload = Vec::new();
    payload.extend_from_slice(&miss_id);
    payload.extend_from_slice(&0u16.to_be_bytes());
    payload.extend_from_slice(&0u16.to_be_bytes());
    payload.extend_from_slice(&8u16.to_be_bytes());
    payload.extend_from_slice(&8u16.to_be_bytes());
    let mut stream = RfbInStream::new(Cursor::new(payload));
    let rect_ref = Rectangle { x: 0, y: 0, width: 8, height: 8, encoding: ENCODING_PERSISTENT_CACHED_RECT };
    let mut buf = ManagedPixelBuffer::new(8, 8, LocalPixelFormat::rgb888());
    ref_dec.decode(&mut stream, &rect_ref, &pf, &mut buf).await.unwrap();
    let v = misses.lock().unwrap();
    assert_eq!(v.len(), 1);
    assert_eq!(v[0], miss_id);
    drop(v);
    misses.lock().unwrap().clear();

    // Init payload — populate cache for the same id via RAW
    let w = 8u16; let h = 8u16; let bpp = 4usize; let stride_pixels = w as usize;
    let block = vec![0x7Fu8; (h as usize) * stride_pixels * bpp];
    let mut payload2 = Vec::new();
    payload2.extend_from_slice(&miss_id);
    payload2.extend_from_slice(&ENCODING_RAW.to_be_bytes());
    payload2.extend_from_slice(&block);
    let mut stream2 = RfbInStream::new(Cursor::new(payload2));
    let rect_init = Rectangle { x: 0, y: 0, width: w, height: h, encoding: ENCODING_PERSISTENT_CACHED_RECT_INIT };
    init_dec.decode(&mut stream2, &rect_init, &pf, &mut buf).await.unwrap();

    // Second reference — expect a hit (no new misses)
    let mut payload3 = Vec::new();
    payload3.extend_from_slice(&miss_id);
    payload3.extend_from_slice(&0u16.to_be_bytes());
    payload3.extend_from_slice(&0u16.to_be_bytes());
    payload3.extend_from_slice(&w.to_be_bytes());
    payload3.extend_from_slice(&h.to_be_bytes());
    let mut stream3 = RfbInStream::new(Cursor::new(payload3));
    ref_dec.decode(&mut stream3, &rect_ref, &pf, &mut buf).await.unwrap();
    assert!(misses.lock().unwrap().is_empty());

    // Trigger eviction and expose ids
    let second_id = [0x22u8; 16];
    let entry = PersistentCachedPixels { id: second_id, pixels: vec![0xAAu8; (h as usize)*stride_pixels*bpp], format: *buf.format(), width: w as u32, height: h as u32, stride_pixels, last_used: std::time::Instant::now() };
    pc.lock().unwrap().insert(entry);
    let _evicted = pc.lock().unwrap().take_evicted_ids();
}
