//! PersistentCache - Disk-backable, content-hash addressed cache for rectangles.

use crate::arc_cache::ArcCache;
use rfb_pixelbuffer::PixelFormat;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

const PERSISTENT_CACHE_SCAFFOLD_MAGIC: &str = "RVPCACHE-SCAFFOLD-V1";
const PERSISTENT_CACHE_BINARY_MAGIC: &[u8] = b"RVPCACHE-BIN-V2\0";

#[derive(Debug, Clone)]
pub struct PersistentCachedPixels {
    pub id: [u8; 16],
    pub pixels: Vec<u8>,
    pub format: PixelFormat,
    pub width: u32,
    pub height: u32,
    /// Stride in pixels (CRITICAL: pixels, not bytes)
    pub stride_pixels: usize,
    pub last_used: Instant,
}

impl PersistentCachedPixels {
    pub fn bytes(&self) -> usize {
        self.pixels.len()
    }
}

#[derive(Debug)]
pub struct PersistentClientCache {
    map: HashMap<[u8; 16], PersistentCachedPixels>,
    max_size_mb: usize,
    current_bytes: usize,
    cache_path: Option<PathBuf>,
    /// ARC eviction core tracking resident and ghost entries by cache ID.
    arc: ArcCache<[u8; 16]>,
    /// Aggregate statistics similar to the C++ GlobalClientPersistentCache.
    cache_hits: u64,
    cache_misses: u64,
    evictions: u64,
}

impl PersistentClientCache {
    pub fn new(max_size_mb: usize) -> Self {
        Self::new_with_path(max_size_mb, None)
    }

    pub fn new_with_path(max_size_mb: usize, cache_path: Option<PathBuf>) -> Self {
        let max_bytes = max_size_mb.saturating_mul(1024 * 1024);
        Self {
            map: HashMap::new(),
            max_size_mb,
            current_bytes: 0,
            cache_path,
            arc: ArcCache::new(max_bytes),
            cache_hits: 0,
            cache_misses: 0,
            evictions: 0,
        }
    }

    pub fn cache_path(&self) -> Option<&Path> {
        self.cache_path.as_deref()
    }

    pub fn load_from_disk(&mut self) -> io::Result<usize> {
        let Some(path) = self.cache_path.as_ref() else {
            return Ok(0);
        };

        let data = match std::fs::read(path) {
            Ok(data) => data,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(err) => return Err(err),
        };

        if data.starts_with(PERSISTENT_CACHE_BINARY_MAGIC) {
            match Self::decode_binary_entries(&data[PERSISTENT_CACHE_BINARY_MAGIC.len()..]) {
                Ok(entries) => {
                    self.reset_runtime_state();
                    let mut restored = 0usize;
                    for entry in entries {
                        self.insert(entry);
                        restored = restored.saturating_add(1);
                    }
                    Ok(restored)
                }
                Err(_) => {
                    self.reset_runtime_state();
                    Ok(0)
                }
            }
        } else if data.starts_with(PERSISTENT_CACHE_SCAFFOLD_MAGIC.as_bytes()) {
            self.reset_runtime_state();
            Ok(0)
        } else {
            self.reset_runtime_state();
            Ok(0)
        }
    }

    pub fn save_to_disk(&self) -> io::Result<()> {
        let Some(path) = self.cache_path.as_ref() else {
            return Ok(());
        };

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut w = BufWriter::new(File::create(path)?);
        w.write_all(PERSISTENT_CACHE_BINARY_MAGIC)?;
        Self::write_u64(&mut w, self.map.len() as u64)?;

        let mut ids: Vec<[u8; 16]> = self.map.keys().copied().collect();
        ids.sort();

        for id in ids {
            let e = self
                .map
                .get(&id)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing cache entry"))?;
            w.write_all(&e.id)?;
            Self::write_u32(&mut w, e.width)?;
            Self::write_u32(&mut w, e.height)?;
            Self::write_u64(&mut w, e.stride_pixels as u64)?;
            Self::write_pf(&mut w, &e.format)?;
            Self::write_u64(&mut w, e.pixels.len() as u64)?;
            w.write_all(&e.pixels)?;
        }

        w.flush()
    }

    pub fn lookup(&mut self, id: &[u8; 16]) -> Option<&PersistentCachedPixels> {
        if let Some(entry) = self.map.get(id) {
            // Notify ARC of a resident hit so it can adapt between T1/T2.
            self.arc.on_hit(id);
            self.cache_hits = self.cache_hits.saturating_add(1);
            Some(entry)
        } else {
            self.cache_misses = self.cache_misses.saturating_add(1);
            None
        }
    }

    /// Insert or replace an entry in the client cache.
    ///
    /// This integrates with the shared ARC core for eviction. The ARC operates
    /// purely on cache IDs and byte sizes; this layer owns the actual payloads.
    pub fn insert(&mut self, entry: PersistentCachedPixels) {
        let id = entry.id;
        let size = entry.bytes();

        // Remove any existing resident entry for this id from both the map and ARC.
        if let Some(old) = self.map.remove(&id) {
            self.current_bytes = self.current_bytes.saturating_sub(old.bytes());
            let _ = self.arc.remove_resident(&id);
        }

        // Let ARC decide which entries to evict to make room for this one.
        let evicted_ids = self.arc.insert_resident(id, size);
        for evicted_id in evicted_ids {
            if let Some(old) = self.map.remove(&evicted_id) {
                self.current_bytes = self.current_bytes.saturating_sub(old.bytes());
                self.evictions = self.evictions.saturating_add(1);
            }
        }

        self.current_bytes = self.current_bytes.saturating_add(size);
        self.map.insert(id, entry);
    }

    /// Current cache usage in bytes.
    pub fn current_bytes(&self) -> usize {
        self.current_bytes
    }

    /// Configured capacity in megabytes.
    pub fn max_size_mb(&self) -> usize {
        self.max_size_mb
    }

    /// Snapshot of high-level statistics for logging.
    pub fn stats(&self) -> PersistentCacheStats {
        let (t1, t2, b1, b2) = self.arc.list_lengths();
        PersistentCacheStats {
            total_entries: self.map.len(),
            total_bytes: self.current_bytes,
            cache_hits: self.cache_hits,
            cache_misses: self.cache_misses,
            evictions: self.evictions,
            t1_size: t1,
            t2_size: t2,
            b1_size: b1,
            b2_size: b2,
        }
    }

    pub fn take_evicted_ids(&mut self) -> Vec<[u8; 16]> {
        self.arc.take_pending_evictions()
    }

    fn reset_runtime_state(&mut self) {
        self.map.clear();
        self.current_bytes = 0;
        self.arc = ArcCache::new(self.max_size_mb.saturating_mul(1024 * 1024));
        self.cache_hits = 0;
        self.cache_misses = 0;
        self.evictions = 0;
    }

    fn decode_binary_entries(data: &[u8]) -> io::Result<Vec<PersistentCachedPixels>> {
        let mut r = BufReader::new(std::io::Cursor::new(data));
        let count = Self::read_u64(&mut r)? as usize;
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            let mut id = [0u8; 16];
            r.read_exact(&mut id)?;
            let width = Self::read_u32(&mut r)?;
            let height = Self::read_u32(&mut r)?;
            let stride_pixels = usize::try_from(Self::read_u64(&mut r)?)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "stride overflow"))?;
            let format = Self::read_pf(&mut r)?;
            let n = usize::try_from(Self::read_u64(&mut r)?)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "pixel length overflow"))?;
            let mut pixels = vec![0u8; n];
            r.read_exact(&mut pixels)?;
            out.push(PersistentCachedPixels {
                id,
                pixels,
                format,
                width,
                height,
                stride_pixels,
                last_used: Instant::now(),
            });
        }
        Ok(out)
    }

    fn write_u32<W: Write>(w: &mut W, v: u32) -> io::Result<()> {
        w.write_all(&v.to_be_bytes())
    }
    fn write_u64<W: Write>(w: &mut W, v: u64) -> io::Result<()> {
        w.write_all(&v.to_be_bytes())
    }
    fn read_u32<R: Read>(r: &mut R) -> io::Result<u32> {
        let mut b = [0u8; 4];
        r.read_exact(&mut b)?;
        Ok(u32::from_be_bytes(b))
    }
    fn read_u64<R: Read>(r: &mut R) -> io::Result<u64> {
        let mut b = [0u8; 8];
        r.read_exact(&mut b)?;
        Ok(u64::from_be_bytes(b))
    }

    fn write_pf<W: Write>(w: &mut W, pf: &PixelFormat) -> io::Result<()> {
        w.write_all(&[
            pf.bits_per_pixel,
            pf.depth,
            u8::from(pf.big_endian),
            u8::from(pf.true_color),
        ])?;
        w.write_all(&pf.red_max.to_be_bytes())?;
        w.write_all(&pf.green_max.to_be_bytes())?;
        w.write_all(&pf.blue_max.to_be_bytes())?;
        w.write_all(&[pf.red_shift, pf.green_shift, pf.blue_shift])?;
        Ok(())
    }

    fn read_pf<R: Read>(r: &mut R) -> io::Result<PixelFormat> {
        let mut h = [0u8; 4];
        let mut rm = [0u8; 2];
        let mut gm = [0u8; 2];
        let mut bm = [0u8; 2];
        let mut sh = [0u8; 3];
        r.read_exact(&mut h)?;
        r.read_exact(&mut rm)?;
        r.read_exact(&mut gm)?;
        r.read_exact(&mut bm)?;
        r.read_exact(&mut sh)?;
        Ok(PixelFormat {
            bits_per_pixel: h[0],
            depth: h[1],
            big_endian: h[2] != 0,
            true_color: h[3] != 0,
            red_max: u16::from_be_bytes(rm),
            green_max: u16::from_be_bytes(gm),
            blue_max: u16::from_be_bytes(bm),
            red_shift: sh[0],
            green_shift: sh[1],
            blue_shift: sh[2],
        })
    }
}

/// Lightweight statistics snapshot used by the viewer for end-of-run logs.
#[derive(Debug, Default, Clone, Copy)]
pub struct PersistentCacheStats {
    pub total_entries: usize,
    pub total_bytes: usize,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub evictions: u64,
    pub t1_size: usize,
    pub t2_size: usize,
    pub b1_size: usize,
    pub b2_size: usize,
}

impl Default for PersistentClientCache {
    fn default() -> Self {
        Self::new(0)
    }
}
