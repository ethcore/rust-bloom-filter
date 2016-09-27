// (C)opyleft 2013-2015 Frank Denis

//! Bloom filter for Rust
//!
//! This is a simple but fast Bloom filter implementation, that requires only
//! 2 hash functions, generated with SipHash-2-4 using randomized keys.
//!

#![crate_name="bloomfilter"]
#![crate_type = "rlib"]
#![warn(non_camel_case_types, non_upper_case_globals, unused_qualifications)]

extern crate rand;

use std::cmp;
use std::f64;
use std::hash::{Hash, Hasher, SipHasher};

#[cfg(test)]
use rand::Rng;

struct BitVecJournal {
    elems: Vec<u64>,
    journal:  Vec<usize>,
}

impl BitVecJournal {
    // size in bytes
    pub fn new(size: usize) -> BitVecJournal {
        let extra = if size % 8 > 0  { 1 } else { 0 };
        BitVecJournal {
            elems: vec![0u64; size / 8 + extra],
            journal: Vec::new(),
        }
    }

    pub fn from_parts(parts: &[u64]) -> BitVecJournal {
        BitVecJournal {
            elems: parts.to_vec(),
            journal: Vec::new(),
        }
    }

    // set
    pub fn set(&mut self, index: usize) {
        println!("set {}", index);
        let e_index = index / 64;
        let bit_index = index % 64;
        let val = self.elems.get_mut(e_index).unwrap();
        *val |= 1u64 << bit_index;
        self.journal.push(e_index);
    }

    pub fn get(&self, index: usize) -> bool {
        let e_index = index / 64;
        let bit_index = index % 64;

        self.elems[e_index] & (1 << bit_index) != 0
    }

    pub fn drain(&mut self) -> Vec<(usize, u64)> {
        let journal = self.journal.drain(..).collect::<Vec<usize>>();
        journal.iter().map(|idx| (*idx, self.elems[*idx])).collect::<Vec<(usize, u64)>>()
    }
}

/// Bloom filter structure
pub struct Bloom {
    bitmap: BitVecJournal,
    bitmap_bits: u64,
    k_num: u32,
    sips: [SipHasher; 2],
}

impl Bloom {
    /// Create a new bloom filter structure.
    /// bitmap_size is the size in bytes (not bits) that will be allocated in memory
    /// items_count is an estimation of the maximum number of items to store.
    pub fn new(bitmap_size: usize, items_count: usize) -> Bloom {
        assert!(bitmap_size > 0 && items_count > 0);
        let bitmap_bits = (bitmap_size as u64) * 8u64;
        let k_num = Bloom::optimal_k_num(bitmap_bits, items_count);
        let bitmap = BitVecJournal::new(bitmap_bits as usize);
        let sips = [Bloom::sip_new(), Bloom::sip_new()];
        Bloom {
            bitmap: bitmap,
            bitmap_bits: bitmap_bits,
            k_num: k_num,
            sips: sips,
        }
    }

    pub fn from_parts(parts: &[u64], k_num: u32) -> Bloom {
        let bitmap_size = parts.len()*8;
        let bitmap_bits = (bitmap_size as u64) * 8u64;
        let bitmap = BitVecJournal::from_parts(parts);
        let sips = [Bloom::sip_new(), Bloom::sip_new()];
        Bloom {
            bitmap: bitmap,
            bitmap_bits: bitmap_bits,
            k_num: k_num,
            sips: sips,
        }
    }

    /// Create a new bloom filter structure.
    /// items_count is an estimation of the maximum number of items to store.
    /// fp_p is the wanted rate of false positives, in ]0.0, 1.0[
    pub fn new_for_fp_rate(items_count: usize, fp_p: f64) -> Bloom {
        let bitmap_size = Bloom::compute_bitmap_size(items_count, fp_p);
        Bloom::new(bitmap_size, items_count)
    }

    /// Compute a recommended bitmap size for items_count items
    /// and a fp_p rate of false positives.
    /// fp_p obviously has to be within the ]0.0, 1.0[ range.
    pub fn compute_bitmap_size(items_count: usize, fp_p: f64) -> usize {
        assert!(items_count > 0);
        assert!(fp_p > 0.0 && fp_p < 1.0);
        let log2 = f64::consts::LN_2;
        let log2_2 = log2 * log2;
        ((items_count as f64) * f64::ln(fp_p) / (-8.0 * log2_2)).ceil() as usize
    }

    /// Record the presence of an item.
    pub fn set<T>(&mut self, item: T)
        where T: Hash
    {
        let mut hashes = [0u64, 0u64];
        for k_i in 0..self.k_num {
            let bit_offset = (self.bloom_hash(&mut hashes, &item, k_i) % self.bitmap_bits) as usize;
            self.bitmap.set(bit_offset);
        }
    }

    /// Check if an item is present in the set.
    /// There can be false positives, but no false negatives.
    pub fn check<T>(&self, item: T) -> bool
        where T: Hash
    {
        let mut hashes = [0u64, 0u64];
        for k_i in 0..self.k_num {
            let bit_offset = (self.bloom_hash(&mut hashes, &item, k_i) % self.bitmap_bits) as usize;
            if !self.bitmap.get(bit_offset) {
                return false;
            }
        }
        true
    }

    /// Return the number of bits in the filter
    pub fn number_of_bits(&self) -> u64 {
        self.bitmap_bits
    }

    /// Return the number of hash functions used for `check` and `set`
    pub fn number_of_hash_functions(&self) -> u32 {
        self.k_num
    }

    fn optimal_k_num(bitmap_bits: u64, items_count: usize) -> u32 {
        let m = bitmap_bits as f64;
        let n = items_count as f64;
        let k_num = (m / n * f64::ln(2.0f64)).ceil() as u32;
        cmp::max(k_num, 1)
    }

    fn bloom_hash<T>(&self, hashes: &mut [u64; 2], item: &T, k_i: u32) -> u64
        where T: Hash
    {
        if k_i < 2 {
            let sip = &mut self.sips[k_i as usize].clone();
            item.hash(sip);
            let hash = sip.finish();
            hashes[k_i as usize] = hash;
            hash
        } else {
            hashes[0].wrapping_add((k_i as u64).wrapping_mul(hashes[1]) % 0xffffffffffffffc5)
        }
    }

    fn sip_new() -> SipHasher {
        SipHasher::new()
    }

    pub fn drain_journal(&mut self) -> BloomJournal {
        BloomJournal {
            entries: self.bitmap.drain(),
            hash_functions: self.k_num,
        }
    }
}

pub struct BloomJournal {
    pub hash_functions: u32,
    pub entries: Vec<(usize, u64)>,
}

#[test]
fn bloom_test_set() {
    let mut bloom = Bloom::new(10, 80);
    let key: &Vec<u8> = &rand::thread_rng().gen_iter::<u8>().take(16).collect();
    assert!(bloom.check(key) == false);
    bloom.set(&key);
    assert!(bloom.check(key.clone()) == true);
}

#[test]
fn bloom_journalling() {
    let initial = vec![0u64; 8];
    let mut bloom = Bloom::from_parts(&initial, 3);
    bloom.set(&vec![5u8, 4]);
    let drain = bloom.drain_journal();

    assert_eq!(3, drain.entries.len())
}