//! A copy-on-write entity store with draw order.
//!
//! Entities live in chunks (`Arc<Vec<Arc<Entity>>>`) in draw order, and a sharded handle index
//! maps each handle to the stable id of its chunk. Cloning a store (an undo snapshot) copies
//! only the chunk and shard pointers; an edit copies one chunk and one shard.

use std::collections::HashMap;
use std::sync::Arc;

use crate::{Entity, Handle};

const CHUNK_MAX: usize = 1024;
const SHARDS: usize = 64;

#[derive(Clone, Debug, Default)]
struct Chunk {
    id: u32,
    items: Vec<Arc<Entity>>,
}

#[derive(Clone, Debug)]
pub struct EntityStore {
    chunks: Vec<Arc<Chunk>>,
    shards: Vec<Arc<HashMap<Handle, u32>>>,
    len: usize,
    next_chunk: u32,
}

impl Default for EntityStore {
    fn default() -> Self {
        EntityStore { chunks: Vec::new(), shards: (0..SHARDS).map(|_| Arc::new(HashMap::new())).collect(), len: 0, next_chunk: 0 }
    }
}

impl PartialEq for EntityStore {
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len && self.iter().zip(other.iter()).all(|(a, b)| a == b)
    }
}

fn shard_of(h: Handle) -> usize {
    // Fibonacci hashing spreads sequential handles across shards.
    ((h.0.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 58) as usize) % SHARDS
}

impl EntityStore {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Iterate in draw order.
    pub fn iter(&self) -> impl Iterator<Item = &Arc<Entity>> + '_ {
        self.chunks.iter().flat_map(|c| c.items.iter())
    }
    pub fn handles(&self) -> Vec<Handle> {
        self.iter().map(|e| e.handle).collect()
    }
    pub fn contains(&self, h: Handle) -> bool {
        self.chunk_id(h).is_some()
    }
    fn chunk_id(&self, h: Handle) -> Option<u32> {
        self.shards.get(shard_of(h)).and_then(|s| s.get(&h).copied())
    }
    fn chunk_pos(&self, id: u32) -> Option<usize> {
        self.chunks.iter().position(|c| c.id == id)
    }
    pub fn get(&self, h: Handle) -> Option<&Arc<Entity>> {
        let pos = self.chunk_pos(self.chunk_id(h)?)?;
        self.chunks.get(pos)?.items.iter().find(|e| e.handle == h)
    }
    fn set_index(&mut self, h: Handle, chunk: Option<u32>) {
        if let Some(shard) = self.shards.get_mut(shard_of(h)) {
            let m = Arc::make_mut(shard);
            match chunk {
                Some(c) => {
                    m.insert(h, c);
                }
                None => {
                    m.remove(&h);
                }
            }
        }
    }
    fn new_chunk(&mut self) -> Arc<Chunk> {
        let id = self.next_chunk;
        self.next_chunk = self.next_chunk.wrapping_add(1);
        Arc::new(Chunk { id, items: Vec::with_capacity(64) })
    }
    /// Append on top of the draw order. Replaces an entity with the same handle in place.
    pub fn push(&mut self, e: Entity) {
        if self.contains(e.handle) {
            self.replace(e);
            return;
        }
        let needs_new = self.chunks.last().is_none_or(|c| c.items.len() >= CHUNK_MAX);
        if needs_new {
            let c = self.new_chunk();
            self.chunks.push(c);
        }
        let h = e.handle;
        let Some(last) = self.chunks.last_mut() else { return };
        let chunk = Arc::make_mut(last);
        chunk.items.push(Arc::new(e));
        let id = chunk.id;
        self.set_index(h, Some(id));
        self.len += 1;
    }
    /// Insert at the bottom of the draw order.
    pub fn push_front(&mut self, e: Entity) {
        if self.contains(e.handle) {
            self.replace(e);
            return;
        }
        let needs_new = self.chunks.first().is_none_or(|c| c.items.len() >= CHUNK_MAX);
        if needs_new {
            let c = self.new_chunk();
            self.chunks.insert(0, c);
        }
        let h = e.handle;
        let Some(first) = self.chunks.first_mut() else { return };
        let chunk = Arc::make_mut(first);
        chunk.items.insert(0, Arc::new(e));
        let id = chunk.id;
        self.set_index(h, Some(id));
        self.len += 1;
    }
    /// Replace the entity with the same handle, keeping its draw position. Returns false if absent.
    pub fn replace(&mut self, e: Entity) -> bool {
        let Some(id) = self.chunk_id(e.handle) else { return false };
        let Some(pos) = self.chunk_pos(id) else { return false };
        let Some(c) = self.chunks.get_mut(pos) else { return false };
        let chunk = Arc::make_mut(c);
        if let Some(slot) = chunk.items.iter_mut().find(|x| x.handle == e.handle) {
            *slot = Arc::new(e);
            true
        } else {
            false
        }
    }
    pub fn remove(&mut self, h: Handle) -> Option<Arc<Entity>> {
        let id = self.chunk_id(h)?;
        let pos = self.chunk_pos(id)?;
        let c = self.chunks.get_mut(pos)?;
        let chunk = Arc::make_mut(c);
        let i = chunk.items.iter().position(|x| x.handle == h)?;
        let e = chunk.items.remove(i);
        let empty = chunk.items.is_empty();
        if empty {
            self.chunks.remove(pos);
        }
        self.set_index(h, None);
        self.len = self.len.saturating_sub(1);
        Some(e)
    }
    /// Modify an entity in place through a closure (copy-on-write).
    pub fn modify<F: FnOnce(&mut Entity)>(&mut self, h: Handle, f: F) -> bool {
        let Some(id) = self.chunk_id(h) else { return false };
        let Some(pos) = self.chunk_pos(id) else { return false };
        let Some(c) = self.chunks.get_mut(pos) else { return false };
        let chunk = Arc::make_mut(c);
        match chunk.items.iter_mut().find(|x| x.handle == h) {
            Some(slot) => {
                f(Arc::make_mut(slot));
                true
            }
            None => false,
        }
    }
    pub fn bring_to_front(&mut self, h: Handle) -> bool {
        match self.remove(h) {
            Some(e) => {
                self.push(Arc::unwrap_or_clone(e));
                true
            }
            None => false,
        }
    }
    pub fn send_to_back(&mut self, h: Handle) -> bool {
        match self.remove(h) {
            Some(e) => {
                self.push_front(Arc::unwrap_or_clone(e));
                true
            }
            None => false,
        }
    }
    /// True when `other` shares every chunk with `self` (same draw order, same entities). Holding
    /// a clone of a store makes this exact: any edit to the original copies the touched chunk.
    pub fn same_as(&self, other: &EntityStore) -> bool {
        self.len == other.len && self.chunks.len() == other.chunks.len() && self.chunks.iter().zip(&other.chunks).all(|(a, b)| Arc::ptr_eq(a, b))
    }
    /// The chunks in draw order as (identity key, entities). A key is stable for as long as any
    /// clone of this store is alive and changes whenever the chunk is edited (copy-on-write), so
    /// caches holding a clone can reuse per-chunk work.
    pub fn chunk_slices(&self) -> impl Iterator<Item = (usize, &[Arc<Entity>])> + '_ {
        self.chunks.iter().map(|c| (Arc::as_ptr(c) as usize, c.items.as_slice()))
    }
    /// The entities of `self` that are not (by identity) in `before`: the ones an edit replaced or
    /// added. Chunks shared with `before` are skipped without looking inside; a changed chunk is
    /// compared with the chunk of the same id in `before`, position by position, falling back to
    /// that chunk's set of entities when positions have shifted. Never misses a changed entity
    /// (it may report one that only moved between chunks), and costs one pointer comparison per
    /// entity of a changed chunk instead of a handle lookup per entity.
    pub fn changed_since<'a>(&'a self, before: &EntityStore) -> Vec<&'a Arc<Entity>> {
        let old: HashMap<u32, &Chunk> = before.chunks.iter().map(|c| (c.id, c.as_ref())).collect();
        let shared: std::collections::HashSet<*const Chunk> = before.chunks.iter().map(Arc::as_ptr).collect();
        let mut out = Vec::new();
        for c in &self.chunks {
            if shared.contains(&Arc::as_ptr(c)) {
                continue;
            }
            let Some(prev) = old.get(&c.id) else {
                out.extend(c.items.iter());
                continue;
            };
            let mut set: Option<std::collections::HashSet<*const Entity>> = None;
            for (i, e) in c.items.iter().enumerate() {
                if prev.items.get(i).is_some_and(|p| Arc::ptr_eq(p, e)) {
                    continue;
                }
                let set = set.get_or_insert_with(|| prev.items.iter().map(Arc::as_ptr).collect());
                if !set.contains(&Arc::as_ptr(e)) {
                    out.push(e);
                }
            }
        }
        out
    }
    /// The handles that were in `before` and are not in `self`: what an edit erased. Only the chunks of `before` that `self` no
    /// longer shares are looked at, so an edit costs its changed chunks, not the drawing.
    pub fn removed_since(&self, before: &EntityStore) -> Vec<Handle> {
        let shared: std::collections::HashSet<*const Chunk> = self.chunks.iter().map(Arc::as_ptr).collect();
        let mut out = Vec::new();
        for c in &before.chunks {
            if shared.contains(&Arc::as_ptr(c)) {
                continue;
            }
            out.extend(c.items.iter().map(|e| e.handle).filter(|h| !self.contains(*h)));
        }
        out
    }
    /// The last entity in draw order (AutoCAD's "Last" selection).
    pub fn last(&self) -> Option<&Arc<Entity>> {
        self.chunks.last().and_then(|c| c.items.last())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EntityKind, Point};
    use aad_geom::Vec2;

    fn ent(h: u64) -> Entity {
        Entity::new(Handle(h), EntityKind::Point(Point { p: Vec2::new(h as f64, 0.0).to3(0.0), angle: 0.0 }))
    }

    #[test]
    fn push_get_remove() {
        let mut s = EntityStore::new();
        for h in 1..=3000 {
            s.push(ent(h));
        }
        assert_eq!(s.len(), 3000);
        assert!(s.get(Handle(1500)).is_some());
        assert!(s.remove(Handle(1500)).is_some());
        assert!(s.get(Handle(1500)).is_none());
        assert_eq!(s.len(), 2999);
        assert_eq!(s.iter().count(), 2999);
    }

    /// PERF-001: `changed_since` finds exactly the entities the old per-handle check found, across
    /// edits that replace, remove (shifting positions inside a chunk) and add entities.
    #[test]
    fn changed_since_matches_the_handle_lookup_definition() {
        let mut s = EntityStore::new();
        for h in 1..=5000 {
            s.push(ent(h));
        }
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = move |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        for round in 0..30 {
            let before = s.clone();
            for _ in 0..(1 + next(40)) {
                let h = Handle(1 + next(6000));
                match next(3) {
                    0 => {
                        s.modify(h, |e| e.common.layer = format!("L{round}"));
                    }
                    1 => {
                        s.remove(h);
                    }
                    _ => s.push(ent(6000 + round * 100 + next(100))),
                }
            }
            let mut want: Vec<Handle> = s.iter().filter(|e| before.get(e.handle).is_none_or(|old| !Arc::ptr_eq(old, e))).map(|e| e.handle).collect();
            let mut got: Vec<Handle> = s.changed_since(&before).iter().map(|e| e.handle).collect();
            want.sort_unstable_by_key(|h| h.0);
            got.sort_unstable_by_key(|h| h.0);
            assert_eq!(got, want, "round {round}");
        }
    }

    #[test]
    fn snapshots_are_independent() {
        let mut s = EntityStore::new();
        for h in 1..=10 {
            s.push(ent(h));
        }
        let snap = s.clone();
        s.remove(Handle(3));
        s.modify(Handle(4), |e| e.common.layer = "X".into());
        assert!(snap.get(Handle(3)).is_some());
        assert_eq!(snap.get(Handle(4)).map(|e| e.common.layer.as_str()), Some("0"));
        assert_eq!(s.get(Handle(4)).map(|e| e.common.layer.as_str()), Some("X"));
    }

    #[test]
    fn draw_order_moves() {
        let mut s = EntityStore::new();
        for h in 1..=5 {
            s.push(ent(h));
        }
        s.bring_to_front(Handle(1));
        assert_eq!(s.last().map(|e| e.handle), Some(Handle(1)));
        s.send_to_back(Handle(5));
        assert_eq!(s.iter().next().map(|e| e.handle), Some(Handle(5)));
        assert_eq!(s.len(), 5);
    }
}
