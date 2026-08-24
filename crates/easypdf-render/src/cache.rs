//! A memory-bounded LRU cache for rendered page tiles.

use std::collections::HashMap;

/// Quantized zoom level.
///
/// Zoom is bucketed so that small changes reuse cached tiles instead of
/// invalidating them. Caching every distinct float zoom would make the cache
/// useless during a pinch gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ZoomBucket(u16);

impl ZoomBucket {
    /// Buckets per unit of zoom. 8 gives ~12.5% granularity, which is below
    /// the threshold where users notice resampling.
    const STEPS_PER_UNIT: f32 = 8.0;

    /// Quantizes a continuous zoom factor into a bucket.
    ///
    /// Non-finite or non-positive input clamps to the smallest bucket rather
    /// than panicking — this is reachable from UI state and must not crash.
    #[must_use]
    pub fn from_zoom(zoom: f32) -> Self {
        if !zoom.is_finite() || zoom <= 0.0 {
            return Self(1);
        }
        let steps = (zoom * Self::STEPS_PER_UNIT).round();
        Self(steps.clamp(1.0, f32::from(u16::MAX)) as u16)
    }

    /// The representative zoom factor for this bucket.
    #[must_use]
    pub fn zoom(self) -> f32 {
        f32::from(self.0) / Self::STEPS_PER_UNIT
    }
}

/// Identifies a cached tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileKey {
    /// Zero-based page index.
    pub page: usize,
    /// Quantized zoom.
    pub zoom: ZoomBucket,
    /// Rotation in degrees clockwise (0, 90, 180, 270).
    pub rotation: i32,
}

/// A rendered tile: raw BGRA pixel data plus its dimensions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tile {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// BGRA pixel data, 4 bytes per pixel.
    pub pixels: Vec<u8>,
}

impl Tile {
    /// Memory footprint in bytes.
    #[must_use]
    pub fn size_bytes(&self) -> usize {
        self.pixels.len()
    }
}

/// LRU tile cache with a hard byte ceiling.
#[derive(Debug)]
pub struct TileCache {
    tiles: HashMap<TileKey, Tile>,
    /// Least-recently-used first.
    order: Vec<TileKey>,
    used_bytes: usize,
    budget_bytes: usize,
}

impl TileCache {
    /// Creates a cache limited to `budget_bytes`.
    #[must_use]
    pub fn new(budget_bytes: usize) -> Self {
        Self { tiles: HashMap::new(), order: Vec::new(), used_bytes: 0, budget_bytes }
    }

    /// Looks up a tile, marking it as recently used.
    pub fn get(&mut self, key: &TileKey) -> Option<&Tile> {
        if self.tiles.contains_key(key) {
            self.touch(key);
            self.tiles.get(key)
        } else {
            None
        }
    }

    /// Inserts a tile, evicting least-recently-used entries as needed.
    ///
    /// A tile larger than the entire budget is rejected rather than triggering
    /// a full eviction that would still not make room for it.
    pub fn insert(&mut self, key: TileKey, tile: Tile) -> bool {
        let size = tile.size_bytes();
        if size > self.budget_bytes {
            return false;
        }
        if let Some(existing) = self.tiles.remove(&key) {
            self.used_bytes -= existing.size_bytes();
            self.order.retain(|k| k != &key);
        }
        while self.used_bytes + size > self.budget_bytes {
            if !self.evict_one() {
                break;
            }
        }
        self.used_bytes += size;
        self.tiles.insert(key, tile);
        self.order.push(key);
        true
    }

    /// Current memory use in bytes.
    #[must_use]
    pub fn used_bytes(&self) -> usize {
        self.used_bytes
    }

    /// Number of cached tiles.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    /// Whether the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    /// Drops every cached tile.
    pub fn clear(&mut self) {
        self.tiles.clear();
        self.order.clear();
        self.used_bytes = 0;
    }

    fn touch(&mut self, key: &TileKey) {
        if let Some(pos) = self.order.iter().position(|k| k == key) {
            let key = self.order.remove(pos);
            self.order.push(key);
        }
    }

    fn evict_one(&mut self) -> bool {
        if self.order.is_empty() {
            return false;
        }
        let key = self.order.remove(0);
        if let Some(tile) = self.tiles.remove(&key) {
            self.used_bytes -= tile.size_bytes();
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(bytes: usize) -> Tile {
        Tile { width: 1, height: 1, pixels: vec![0; bytes] }
    }

    fn key(page: usize) -> TileKey {
        TileKey { page, zoom: ZoomBucket::from_zoom(1.0), rotation: 0 }
    }

    #[test]
    fn nearby_zoom_levels_share_a_bucket() {
        // The point of bucketing: a pinch gesture must not invalidate the cache
        // on every frame.
        assert_eq!(ZoomBucket::from_zoom(1.00), ZoomBucket::from_zoom(1.02));
        assert_ne!(ZoomBucket::from_zoom(1.0), ZoomBucket::from_zoom(2.0));
    }

    #[test]
    fn degenerate_zoom_values_do_not_panic() {
        // Reachable from UI state; must degrade rather than crash.
        assert_eq!(ZoomBucket::from_zoom(f32::NAN), ZoomBucket::from_zoom(0.0));
        assert_eq!(ZoomBucket::from_zoom(-5.0), ZoomBucket::from_zoom(0.0));
        let _ = ZoomBucket::from_zoom(f32::INFINITY);
        let _ = ZoomBucket::from_zoom(1e30);
    }

    #[test]
    fn cache_never_exceeds_its_budget() {
        // The property that keeps a 400 MB scan from becoming an OOM crash.
        let mut cache = TileCache::new(1000);
        for page in 0..50 {
            cache.insert(key(page), tile(100));
            assert!(cache.used_bytes() <= 1000, "budget exceeded at page {page}");
        }
    }

    #[test]
    fn eviction_removes_least_recently_used() {
        let mut cache = TileCache::new(300);
        cache.insert(key(0), tile(100));
        cache.insert(key(1), tile(100));
        cache.insert(key(2), tile(100));

        // Touch page 0 so page 1 becomes the eviction candidate.
        assert!(cache.get(&key(0)).is_some());
        cache.insert(key(3), tile(100));

        assert!(cache.get(&key(0)).is_some(), "recently used tile was evicted");
        assert!(cache.get(&key(1)).is_none(), "LRU tile should have been evicted");
    }

    #[test]
    fn reinserting_a_key_does_not_double_count_memory() {
        let mut cache = TileCache::new(1000);
        cache.insert(key(0), tile(100));
        cache.insert(key(0), tile(200));
        assert_eq!(cache.used_bytes(), 200);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn oversized_tile_is_rejected_without_flushing_the_cache() {
        let mut cache = TileCache::new(500);
        cache.insert(key(0), tile(100));
        assert!(!cache.insert(key(1), tile(5000)));
        assert!(cache.get(&key(0)).is_some(), "existing tiles must survive");
    }

    #[test]
    fn clear_resets_accounting() {
        let mut cache = TileCache::new(1000);
        cache.insert(key(0), tile(400));
        cache.clear();
        assert_eq!(cache.used_bytes(), 0);
        assert!(cache.is_empty());
    }
}
