use std::collections::HashSet;
use std::hash::Hash;
use std::{any, fmt};

use itertools::Itertools as _;
use segment::index::field_index::CardinalityEstimation;
use segment::types::{PointIdType, ReshardingCondition};
use smallvec::SmallVec;

use crate::shards::shard::ShardId;

const HASH_RING_SHARD_SCALE: u32 = 100;

#[derive(Clone, PartialEq, Debug)]
pub enum HashRing<T = ShardId> {
    /// Single hashring
    Single(Inner<T>),

    /// Two hashrings when transitioning during resharding
    /// Depending on the current resharding state, points may be in either or both shards.
    Resharding { old: Inner<T>, new: Inner<T> },
}

impl<T: Hash + Copy + PartialEq> HashRing<T> {
    /// Create a new single hashring.
    ///
    /// The hashring is created with a fair distribution of points and `HASH_RING_SHARD_SCALE` scale.
    pub fn single() -> Self {
        Self::Single(Inner::fair(HASH_RING_SHARD_SCALE))
    }

    /// Create a new resharding hashring, with resharding shard already added into `new` hashring.
    ///
    /// The hashring is created with a fair distribution of points and `HASH_RING_SHARD_SCALE` scale.
    pub fn resharding(shard: T) -> Self {
        let mut ring = Self::Resharding {
            old: Inner::fair(HASH_RING_SHARD_SCALE),
            new: Inner::fair(HASH_RING_SHARD_SCALE),
        };

        ring.add_resharding(shard);

        ring
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Self::Single(ring) => ring.is_empty(),
            Self::Resharding { old, new } => old.is_empty() && new.is_empty(),
        }
    }

    pub fn is_resharding(&self) -> bool {
        matches!(self, Self::Resharding { .. })
    }

    pub fn add(&mut self, shard: T) {
        match self {
            Self::Single(ring) => ring.add(shard),
            Self::Resharding { old, new } => {
                if new.get(&shard).is_none() {
                    old.add(shard);
                    new.add(shard);
                }
            }
        }
    }

    pub fn add_resharding(&mut self, shard: T) {
        if let Self::Single(ring) = self {
            let (old, new) = (ring.clone(), ring.clone());
            *self = Self::Resharding { old, new };
        }

        let Self::Resharding { new, .. } = self else {
            unreachable!();
        };

        new.add(shard);
    }

    pub fn commit_resharding(&mut self) -> bool {
        let Self::Resharding { new, .. } = self else {
            log::warn!("committing resharding hashring, but hashring is not in resharding mode");
            return false;
        };

        *self = Self::Single(new.clone());
        true
    }

    pub fn remove_resharding(&mut self, shard: T) -> bool
    where
        T: fmt::Display,
    {
        let Self::Resharding { old, new } = self else {
            log::warn!("removing resharding shard, but hashring is not in resharding mode");
            return false;
        };

        let mut old = old.clone();
        let mut new = new.clone();

        let removed_from_old = old.remove(&shard);
        let removed_from_new = new.remove(&shard);

        let removed_resharding = match (removed_from_old, removed_from_new) {
            (false, true) => true,

            (true, true) => {
                log::error!(
                    "removing resharding shard, \
                     but {shard} is not resharding shard"
                );

                false
            }

            (true, false) => {
                log::error!(
                    "removing resharding shard, \
                     but shard {shard} only exists in the old hashring"
                );

                false
            }

            (false, false) => {
                log::warn!(
                    "removing resharding shard, \
                     but shard {shard} does not exist in the hashring"
                );

                false
            }
        };

        if old == new {
            log::debug!(
                "switching hashring into single mode, \
                 because all resharding shards were removed",
            );

            *self = Self::Single(old);
        }

        removed_resharding
    }

    pub fn get<U: Hash>(&self, key: &U) -> ShardIds<T>
    where
        T: PartialEq,
    {
        match self {
            Self::Single(ring) => ring.get(key).into_iter().cloned().collect(),
            Self::Resharding { old, new } => old
                .get(key)
                .into_iter()
                .chain(new.get(key))
                // Both hash rings may return the same shard ID, take it once
                .dedup()
                .cloned()
                .collect(),
        }
    }

    /// Check whether the given point is in the given shard
    ///
    /// In case of resharding, the new hashring is checked.
    pub fn is_in_shard<U: Hash>(&self, key: &U, shard: T) -> bool
    where
        T: PartialEq,
    {
        let ring = match self {
            Self::Resharding { new, .. } => new,
            Self::Single(ring) => ring,
        };

        ring.get(key) == Some(&shard)
    }
}

impl<T: Hash + Copy + PartialEq + Eq> HashRing<T> {
    /// Get unique nodes from the hashring
    pub fn unique_nodes(&self) -> HashSet<T> {
        match self {
            Self::Single(ring) => ring.unique_nodes(),
            Self::Resharding { new, .. } => new.unique_nodes(),
        }
    }
}

/// List type for shard IDs
///
/// Uses a `SmallVec` putting two IDs on the stack. That's the maximum number of shards we expect
/// with the current resharding implementation.
pub type ShardIds<T = ShardId> = SmallVec<[T; 2]>;

#[derive(Clone, PartialEq, Debug)]
pub enum Inner<T> {
    Raw(hashring::HashRing<T>),

    Fair {
        ring: hashring::HashRing<(T, u32)>,
        scale: u32,
    },
}

impl<T: Hash + Copy> Inner<T> {
    pub fn raw() -> Self {
        Self::Raw(hashring::HashRing::new())
    }

    /// Constructs a HashRing that tries to give all shards equal space on the ring.
    /// The higher the `scale` - the more equal the distribution of points on the shards will be,
    /// but shard search might be slower.
    pub fn fair(scale: u32) -> Self {
        Self::Fair {
            ring: hashring::HashRing::new(),
            scale,
        }
    }

    pub fn add(&mut self, shard: T) {
        match self {
            Inner::Raw(ring) => ring.add(shard),
            Inner::Fair { ring, scale } => {
                for i in 0..*scale {
                    ring.add((shard, i))
                }
            }
        }
    }

    pub fn remove(&mut self, shard: &T) -> bool {
        match self {
            Inner::Raw(ring) => ring.remove(shard).is_some(),
            Inner::Fair { ring, scale } => {
                let mut removed = false;
                for i in 0..*scale {
                    if ring.remove(&(*shard, i)).is_some() {
                        removed = true;
                    }
                }
                removed
            }
        }
    }

    pub fn get<U: Hash>(&self, key: &U) -> Option<&T> {
        match self {
            Inner::Raw(ring) => ring.get(key),
            Inner::Fair { ring, .. } => ring.get(key).map(|(shard, _)| shard),
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Inner::Raw(ring) => ring.is_empty(),
            Inner::Fair { ring, .. } => ring.is_empty(),
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Inner::Raw(ring) => ring.len(),
            Inner::Fair { ring, scale } => ring.len() / *scale as usize,
        }
    }
}

impl<T: Hash + Copy + PartialEq + Eq> Inner<T> {
    /// Get unique nodes from the hashring
    pub fn unique_nodes(&self) -> HashSet<T> {
        match self {
            Inner::Raw(ring) => ring.clone().into_iter().collect(),
            Inner::Fair { ring, .. } => ring.clone().into_iter().map(|(node, _)| node).collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Filter<T = ShardId> {
    ring: Inner<T>,
    filter: T,
}

impl<T> Filter<T> {
    pub fn new(ring: Inner<T>, filter: T) -> Self {
        Self { ring, filter }
    }

    pub fn check(&self, point_id: PointIdType) -> bool
    where
        T: Hash + PartialEq + Copy,
    {
        self.ring.get(&point_id) == Some(&self.filter)
    }
}

impl<T> ReshardingCondition for Filter<T>
where
    T: fmt::Debug + Hash + PartialEq + Copy + 'static,
{
    fn check(&self, point_id: PointIdType) -> bool {
        self.check(point_id)
    }

    fn estimate_cardinality(&self, points: usize) -> CardinalityEstimation {
        CardinalityEstimation {
            primary_clauses: vec![],
            min: 0,
            exp: points / self.ring.len(),
            max: points,
        }
    }

    fn eq(&self, other: &dyn ReshardingCondition) -> bool {
        match other.as_any().downcast_ref::<Self>() {
            Some(other) => self == other,
            None => false,
        }
    }

    fn as_any(&self) -> &dyn any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_non_seq_keys() {
        let mut ring = Inner::fair(100);
        ring.add(5);
        ring.add(7);
        ring.add(8);
        ring.add(20);

        for i in 0..20 {
            match ring.get(&i) {
                None => panic!("Key {i} has no shard"),
                Some(x) => assert!([5, 7, 8, 20].contains(x)),
            }
        }
    }

    #[test]
    fn test_repartition() {
        let mut ring = Inner::fair(100);

        ring.add(1);
        ring.add(2);
        ring.add(3);

        let mut pre_split = Vec::new();
        let mut post_split = Vec::new();

        for i in 0..100 {
            match ring.get(&i) {
                None => panic!("Key {i} has no shard"),
                Some(x) => pre_split.push(*x),
            }
        }

        ring.add(4);

        for i in 0..100 {
            match ring.get(&i) {
                None => panic!("Key {i} has no shard"),
                Some(x) => post_split.push(*x),
            }
        }

        assert_ne!(pre_split, post_split);

        for (x, y) in pre_split.iter().zip(post_split.iter()) {
            if x != y {
                assert_eq!(*y, 4);
            }
        }
    }
}
