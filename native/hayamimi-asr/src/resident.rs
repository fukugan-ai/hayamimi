//! The recognizers kept in memory, and the rule that drops one.
//!
//! The Japanese tier is always kept and does not count. Every other tier
//! counts, and when a new one arrives at the cap the one used least recently
//! is dropped. A cap of `None` keeps them all.

use std::num::NonZeroUsize;

use crate::route::Tier;

struct Entry<T> {
    tier: Tier,
    value: T,
    /// The tick of the last use; larger is more recent.
    used_at: u64,
}

/// The resident recognizers, at most `cap` of them besides the always-resident tier.
pub struct Residents<T> {
    cap: Option<NonZeroUsize>,
    entries: Vec<Entry<T>>,
    tick: u64,
}

impl<T> Residents<T> {
    /// An empty set. `None` places no limit.
    pub fn new(cap: Option<NonZeroUsize>) -> Residents<T> {
        Residents {
            cap,
            entries: Vec::new(),
            tick: 0,
        }
    }

    fn next_tick(&mut self) -> u64 {
        self.tick += 1;
        self.tick
    }

    /// The value of a resident tier, which counts as a use.
    pub fn get(&mut self, tier: Tier) -> Option<&T> {
        let tick = self.next_tick();
        let entry = self.entries.iter_mut().find(|entry| entry.tier == tier)?;
        entry.used_at = tick;
        Some(&entry.value)
    }

    /// Whether the tier is resident; not a use.
    pub fn contains(&self, tier: Tier) -> bool {
        self.entries.iter().any(|entry| entry.tier == tier)
    }

    /// The resident tiers in tier order.
    pub fn tiers(&self) -> Vec<Tier> {
        let mut tiers: Vec<Tier> = self.entries.iter().map(|entry| entry.tier).collect();
        tiers.sort();
        tiers
    }

    /// Drops every resident tier.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// How many resident tiers count against the cap.
    pub fn counted(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| !entry.tier.is_always_resident())
            .count()
    }

    /// The counted tier used least recently.
    fn least_recently_used(&self) -> Option<Tier> {
        self.entries
            .iter()
            .filter(|entry| !entry.tier.is_always_resident())
            .min_by_key(|entry| entry.used_at)
            .map(|entry| entry.tier)
    }

    /// Whether adding `incoming` would pass the cap.
    fn is_full_for(&self, incoming: Tier) -> bool {
        let Some(cap) = self.cap else {
            return false;
        };
        !incoming.is_always_resident() && self.counted() >= cap.get()
    }

    /// Adds a freshly loaded tier as the most recently used one and returns
    /// the tier it displaced, if any. A tier that is already resident is replaced.
    pub fn insert(&mut self, tier: Tier, value: T) -> Option<Tier> {
        self.entries.retain(|entry| entry.tier != tier);
        let dropped = if self.is_full_for(tier) {
            self.least_recently_used()
        } else {
            None
        };
        if let Some(victim) = dropped {
            self.entries.retain(|entry| entry.tier != victim);
        }
        let used_at = self.next_tick();
        self.entries.push(Entry {
            tier,
            value,
            used_at,
        });
        dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cap(n: usize) -> Option<NonZeroUsize> {
        NonZeroUsize::new(n)
    }

    #[test]
    fn nothing_is_dropped_below_the_cap() {
        let mut set = Residents::new(cap(3));
        assert_eq!(set.insert(Tier::Rz, 0), None);
        assert_eq!(set.insert(Tier::Pz, 1), None);
        assert_eq!(set.insert(Tier::Sv, 2), None);
        assert_eq!(set.insert(Tier::V3, 3), None);
        assert_eq!(set.counted(), 3);
    }

    #[test]
    fn the_least_recently_used_tier_is_dropped_at_the_cap() {
        let mut set = Residents::new(cap(2));
        set.insert(Tier::Pz, 1);
        set.insert(Tier::Sv, 2);
        assert_eq!(set.insert(Tier::V3, 3), Some(Tier::Pz));
        assert_eq!(set.tiers(), [Tier::Sv, Tier::V3]);
    }

    #[test]
    fn a_use_makes_a_tier_the_most_recent() {
        let mut set = Residents::new(cap(2));
        set.insert(Tier::Pz, 1);
        set.insert(Tier::Sv, 2);
        assert_eq!(set.get(Tier::Pz), Some(&1));
        assert_eq!(set.insert(Tier::V3, 3), Some(Tier::Sv));
        assert_eq!(set.tiers(), [Tier::Pz, Tier::V3]);
    }

    #[test]
    fn the_japanese_tier_is_never_dropped_and_does_not_count() {
        let mut set = Residents::new(cap(1));
        assert_eq!(set.insert(Tier::Rz, 0), None);
        assert_eq!(set.insert(Tier::Pz, 1), None);
        assert_eq!(set.counted(), 1);
        assert_eq!(set.insert(Tier::Sv, 2), Some(Tier::Pz));
        assert!(set.contains(Tier::Rz));
        assert_eq!(set.tiers(), [Tier::Rz, Tier::Sv]);
    }

    #[test]
    fn the_japanese_tier_arrives_at_a_full_set_without_dropping_anything() {
        let mut set = Residents::new(cap(1));
        set.insert(Tier::Pz, 1);
        assert_eq!(set.insert(Tier::Rz, 0), None);
        assert_eq!(set.tiers(), [Tier::Rz, Tier::Pz]);
    }

    #[test]
    fn no_cap_keeps_every_tier() {
        let mut set = Residents::new(None);
        for (index, tier) in Tier::ALL.into_iter().enumerate() {
            assert_eq!(set.insert(tier, index), None);
        }
        assert_eq!(set.tiers().len(), Tier::ALL.len());
    }

    #[test]
    fn loading_a_resident_tier_again_replaces_it_without_dropping_another() {
        let mut set = Residents::new(cap(2));
        set.insert(Tier::Pz, 1);
        set.insert(Tier::Sv, 2);
        assert_eq!(set.insert(Tier::Pz, 10), None);
        assert_eq!(set.get(Tier::Pz), Some(&10));
        assert_eq!(set.counted(), 2);
    }

    #[test]
    fn an_empty_set_has_nothing_to_drop_or_get() {
        let mut set: Residents<u8> = Residents::new(cap(1));
        assert_eq!(set.get(Tier::Pz), None);
        assert!(set.tiers().is_empty());
        assert_eq!(set.counted(), 0);
    }

    #[test]
    fn five_languages_fit_three_slots_and_a_cap_of_two_drops_one() {
        // Japanese, English, Mandarin, Korean and Cantonese use rz, v3, pz and sv.
        let route = [Tier::Rz, Tier::V3, Tier::Pz, Tier::Sv, Tier::Sv];
        let mut dropped_at_three = 0;
        let mut dropped_at_two = 0;
        let (mut three, mut two) = (Residents::new(cap(3)), Residents::new(cap(2)));
        for (index, tier) in route.into_iter().enumerate() {
            if !three.contains(tier) && three.insert(tier, index).is_some() {
                dropped_at_three += 1;
            }
            if !two.contains(tier) && two.insert(tier, index).is_some() {
                dropped_at_two += 1;
            }
        }
        assert_eq!(dropped_at_three, 0);
        assert_eq!(dropped_at_two, 1);
    }
}
