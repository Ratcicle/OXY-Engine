//! Seeded randomness and IDs derived from stable parts: the same parts give the same numbers
//! and IDs on every run and machine, unlike std's hashers. Not for secrets.
use crate::document::Id;

/// The largest seed. Seeds are the integers a JSON number keeps exactly, 0 to 2^53 − 1.
pub const MAX_SEED: u64 = (1 << 53) - 1;

const GOLDEN: u64 = 0x9e37_79b9_7f4a_7c15;

/// The SplitMix64 finalizer: every input bit reaches every output bit.
fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// FNV-1a fed piece by piece, for data too long to gather first, such as the runtime's state
/// (see `Runtime::state_hash`). [`hash`] is one with the parts' lengths written in.
#[derive(Clone, Debug)]
pub struct Hasher(u64);
impl Default for Hasher {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}
impl Hasher {
    pub fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    /// The hash of everything written so far, through the SplitMix64 finalizer.
    pub fn finish(&self) -> u64 {
        mix(self.0)
    }
}

/// A hash of `parts`: FNV-1a over each part after its length, so ("ab", "c") and ("a", "bc")
/// differ, then the SplitMix64 finalizer.
pub fn hash(parts: &[&[u8]]) -> u64 {
    let mut hasher = Hasher::default();
    for part in parts {
        hasher.write(&(part.len() as u64).to_le_bytes());
        hasher.write(part);
    }
    hasher.finish()
}

/// SplitMix64: a small generator whose numbers depend only on its seed.
#[derive(Clone, Debug)]
pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(GOLDEN);
        mix(self.0)
    }
    /// A number in [0, 1), from 53 random bits.
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1_u64 << 53) as f64
    }
    /// An integer from `min` to `max`, both included and every one as likely; `min` when `max`
    /// is not above it.
    pub fn int_inclusive(&mut self, min: i64, max: i64) -> i64 {
        if max <= min {
            return min;
        }
        let span = max.wrapping_sub(min) as u64;
        if span == u64::MAX {
            return self.next_u64() as i64;
        }
        let count = span + 1;
        // Multiply and keep the high half (Lemire), rejecting the few low halves that would
        // make some values more likely than others.
        let threshold = count.wrapping_neg() % count;
        loop {
            let product = u128::from(self.next_u64()) * u128::from(count);
            if product as u64 >= threshold {
                return min.wrapping_add((product >> 64) as i64);
            }
        }
    }
    /// A number in [min, max); `min` when `max` is not above it.
    pub fn range(&mut self, min: f64, max: f64) -> f64 {
        if min < max {
            let unit = self.unit();
            // Weighted rather than min + (max − min)·unit, which overflows on wide spans.
            (min * (1. - unit) + max * unit).clamp(min, max.next_down())
        } else {
            min
        }
    }
}

/// A version 8 UUID made from `parts`: the same parts give the same ID on every run, shaped
/// like the IDs the editor makes.
pub fn derived_id(parts: &[&[u8]]) -> Id {
    let half = |side: &'static [u8]| {
        let mut all = vec![side];
        all.extend_from_slice(parts);
        hash(&all)
    };
    let mut bytes = [0; 16];
    bytes[..8].copy_from_slice(&half(b"oxy.id.0").to_be_bytes());
    bytes[8..].copy_from_slice(&half(b"oxy.id.1").to_be_bytes());
    uuid::Builder::from_custom_bytes(bytes)
        .into_uuid()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draws(seed: u64) -> Vec<u64> {
        let mut rng = Rng::new(seed);
        (0..32).map(|_| rng.next_u64()).collect()
    }

    #[test]
    fn the_same_seed_repeats_the_numbers_and_another_one_changes_them() {
        assert_eq!(draws(7), draws(7));
        assert_ne!(draws(7), draws(8));
        assert_ne!(draws(0), draws(1));
        // Fixed, so that a change of algorithm shows: saved replays depend on these numbers.
        assert_eq!(Rng::new(0).next_u64(), 0xe220_a839_7b1d_cdaf);
        assert_ne!(hash(&[b"ab", b"c"]), hash(&[b"a", b"bc"]));
        assert_ne!(hash(&[b"a"]), hash(&[b"a", b""]));
        let mut rng = Rng::new(3);
        for _ in 0..10_000 {
            let unit = rng.unit();
            assert!((0. ..1.).contains(&unit), "{unit}");
            let value = rng.range(-2.5, 4.);
            assert!((-2.5..4.).contains(&value), "{value}");
        }
        assert_eq!(rng.range(1., 1.), 1.);
        assert_eq!(rng.range(2., 1.), 2.);
        let wide = rng.range(-f64::MAX, f64::MAX);
        assert!(wide.is_finite() && wide < f64::MAX);
        // The top of the range is never drawn, even when the span rounds up to it.
        assert!(rng.range(1e16, 1e16 + 2.) < 1e16 + 2.);
    }

    #[test]
    fn a_hasher_fed_in_pieces_hashes_as_one_write() {
        let mut whole = Hasher::default();
        whole.write(b"oxy.state");
        let mut pieces = Hasher::default();
        for piece in [&b"oxy"[..], b".", b"", b"state"] {
            pieces.write(piece);
        }
        assert_eq!(whole.finish(), pieces.finish());
        // `hash` is a hasher given each part's length first. Fixed, as state hashes compared
        // across processes and versions depend on it.
        let mut parts = Hasher::default();
        for part in [&b"ab"[..], b"c"] {
            parts.write(&(part.len() as u64).to_le_bytes());
            parts.write(part);
        }
        assert_eq!(parts.finish(), hash(&[b"ab", b"c"]));
        assert_eq!(hash(&[b"oxy"]), 0x94aa_51c5_8d1e_62c8);
    }

    #[test]
    fn integers_include_both_ends_and_come_out_alike() {
        let mut rng = Rng::new(42);
        let mut counts = [0_u32; 6];
        for _ in 0..10_000 {
            let face = rng.int_inclusive(1, 6);
            assert!((1..=6).contains(&face), "{face}");
            counts[face as usize - 1] += 1;
        }
        // 10 000 / 6 ≈ 1667 each; a fair die strays from it by about 37.
        for count in counts {
            assert!((1500..=1833).contains(&count), "{counts:?}");
        }
        let chi_square: f64 = counts
            .iter()
            .map(|&count| (f64::from(count) - 10_000. / 6.).powi(2) / (10_000. / 6.))
            .sum();
        // 20.5 is the 0.1 % critical value with 5 degrees of freedom.
        assert!(chi_square < 20.5, "{chi_square} {counts:?}");
        assert_eq!(rng.int_inclusive(5, 5), 5);
        assert_eq!(rng.int_inclusive(5, 4), 5);
        for _ in 0..100 {
            rng.int_inclusive(i64::MIN, i64::MAX);
            let negative = rng.int_inclusive(-3, -1);
            assert!((-3..=-1).contains(&negative));
            let edge = rng.int_inclusive(i64::MAX - 1, i64::MAX);
            assert!(edge >= i64::MAX - 1);
        }
    }

    #[test]
    fn derived_ids_are_stable_version_8_uuids() {
        let id = derived_id(&[b"oxy.spawn", b"owner", b"node", &0_u64.to_le_bytes(), b"o"]);
        assert_eq!(
            id,
            derived_id(&[b"oxy.spawn", b"owner", b"node", &0_u64.to_le_bytes(), b"o"])
        );
        assert_ne!(
            id,
            derived_id(&[b"oxy.spawn", b"owner", b"node", &1_u64.to_le_bytes(), b"o"])
        );
        assert_ne!(id, derived_id(&[b"oxy.spawn", b"owner", b"node2"]));
        // Lower-case hex with dashes at 8, 13, 18 and 23, as the headless report and the
        // editor recognize IDs; version 8 and the RFC variant.
        assert_eq!(id.len(), 36);
        for (i, c) in id.chars().enumerate() {
            match i {
                8 | 13 | 18 | 23 => assert_eq!(c, '-', "{id}"),
                _ => assert!(c.is_ascii_hexdigit() && !c.is_ascii_uppercase(), "{id}"),
            }
        }
        assert_eq!(&id[14..15], "8", "{id}");
        assert!("89ab".contains(&id[19..20]), "{id}");
        let parsed = uuid::Uuid::parse_str(&id).unwrap();
        assert_eq!(parsed.get_version_num(), 8);
    }
}
