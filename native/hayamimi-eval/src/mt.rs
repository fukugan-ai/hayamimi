//! The Mersenne Twister and the shuffle of Python's `random` module, so that
//! the FLEURS sentences chosen for a given seed are the ones the Python
//! evaluation chose.
//!
//! Only what that needs is here: seeding with a non-negative integer below
//! 2^32, `getrandbits` up to 32 bits, and `shuffle`.

const N: usize = 624;
const M: usize = 397;
const UPPER_MASK: u32 = 0x8000_0000;
const LOWER_MASK: u32 = 0x7fff_ffff;
const MATRIX_A: u32 = 0x9908_b0df;

pub struct Mt19937 {
    state: [u32; N],
    index: usize,
}

impl Mt19937 {
    fn init_genrand(seed: u32) -> Mt19937 {
        let mut state = [0_u32; N];
        state[0] = seed;
        for i in 1..N {
            let previous = state[i - 1];
            state[i] = 1_812_433_253_u32
                .wrapping_mul(previous ^ (previous >> 30))
                .wrapping_add(i as u32);
        }
        Mt19937 { state, index: N }
    }

    /// The generator `random.Random(seed)` starts as, for `seed < 2^32`.
    /// CPython seeds an integer through `init_by_array` with the integer's
    /// 32-bit words, which for such a seed is the one word `seed`.
    pub fn from_python_seed(seed: u32) -> Mt19937 {
        let mut generator = Mt19937::init_genrand(19_650_218);
        let key = [seed];
        let mt = &mut generator.state;
        let mut i = 1;
        let mut j = 0;
        for _ in 0..N.max(key.len()) {
            let previous = mt[i - 1];
            mt[i] = (mt[i] ^ (previous ^ (previous >> 30)).wrapping_mul(1_664_525))
                .wrapping_add(key[j])
                .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= N {
                mt[0] = mt[N - 1];
                i = 1;
            }
            if j >= key.len() {
                j = 0;
            }
        }
        for _ in 0..N - 1 {
            let previous = mt[i - 1];
            mt[i] = (mt[i] ^ (previous ^ (previous >> 30)).wrapping_mul(1_566_083_941))
                .wrapping_sub(i as u32);
            i += 1;
            if i >= N {
                mt[0] = mt[N - 1];
                i = 1;
            }
        }
        mt[0] = 0x8000_0000;
        generator
    }

    fn regenerate(&mut self) {
        let mix = |upper: u32, lower: u32, far: u32| {
            let y = (upper & UPPER_MASK) | (lower & LOWER_MASK);
            far ^ (y >> 1) ^ if y & 1 == 1 { MATRIX_A } else { 0 }
        };
        for k in 0..N - M {
            self.state[k] = mix(self.state[k], self.state[k + 1], self.state[k + M]);
        }
        for k in N - M..N - 1 {
            self.state[k] = mix(self.state[k], self.state[k + 1], self.state[k + M - N]);
        }
        self.state[N - 1] = mix(self.state[N - 1], self.state[0], self.state[M - 1]);
        self.index = 0;
    }

    /// The next 32 random bits.
    pub fn next_u32(&mut self) -> u32 {
        if self.index >= N {
            self.regenerate();
        }
        let mut y = self.state[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^ (y >> 18)
    }

    /// `k` random bits (`1..=32`), the high bits of one output.
    pub fn getrandbits(&mut self, k: u32) -> u32 {
        self.next_u32() >> (32 - k)
    }

    /// A value in `0..n` (`n > 0`) by rejection, as `Random._randbelow`.
    fn randbelow(&mut self, n: usize) -> usize {
        let k = usize::BITS - n.leading_zeros();
        loop {
            let r = self.getrandbits(k) as usize;
            if r < n {
                return r;
            }
        }
    }

    /// Shuffles in place exactly as `Random.shuffle`.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.randbelow(i + 1);
            items.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `random.random()` is `(a * 2^26 + b) / 2^53` from two outputs.
    fn random_f64(generator: &mut Mt19937) -> f64 {
        let a = f64::from(generator.next_u32() >> 5);
        let b = f64::from(generator.next_u32() >> 6);
        (a * 67_108_864.0 + b) / 9_007_199_254_740_992.0
    }

    #[test]
    fn seed_zero_starts_the_sequence_python_gives() {
        let mut generator = Mt19937::from_python_seed(0);
        let first = random_f64(&mut generator);
        let second = random_f64(&mut generator);
        assert!((first - 0.844_421_851_525_048_1).abs() < 1e-15, "{first}");
        assert!((second - 0.757_954_402_940_302_5).abs() < 1e-15, "{second}");
    }

    #[test]
    fn a_reference_vector_of_the_raw_generator() {
        // init_genrand(5489) first output of the reference implementation.
        let mut generator = Mt19937::init_genrand(5489);
        assert_eq!(generator.next_u32(), 3_499_211_612);
    }

    #[test]
    fn getrandbits_keeps_the_high_bits() {
        let mut one = Mt19937::from_python_seed(1);
        let mut other = Mt19937::from_python_seed(1);
        assert_eq!(one.getrandbits(8), other.next_u32() >> 24);
    }

    #[test]
    fn a_shuffle_keeps_every_element() {
        let mut items: Vec<u32> = (0..100).collect();
        Mt19937::from_python_seed(0).shuffle(&mut items);
        let mut sorted = items.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..100).collect::<Vec<u32>>());
        assert_ne!(items, (0..100).collect::<Vec<u32>>());
    }

    #[test]
    fn the_same_seed_shuffles_the_same_way() {
        let mut one: Vec<u32> = (0..50).collect();
        let mut other = one.clone();
        Mt19937::from_python_seed(7).shuffle(&mut one);
        Mt19937::from_python_seed(7).shuffle(&mut other);
        assert_eq!(one, other);
    }

    #[test]
    fn shuffling_nothing_or_one_changes_nothing() {
        let mut empty: [u32; 0] = [];
        Mt19937::from_python_seed(0).shuffle(&mut empty);
        let mut one = [9_u32];
        Mt19937::from_python_seed(0).shuffle(&mut one);
        assert_eq!(one, [9]);
    }
}
