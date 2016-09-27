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
extern crate bit_vec;

use std::cmp;
use std::f64;
use std::hash::{Hash, Hasher, SipHasher};
use bit_vec::BitVec;

#[cfg(test)]
use rand::Rng;

/// Bloom filter structure
pub struct Bloom {
    bitmap: BitVec,
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
        let bitmap = BitVec::from_elem(bitmap_bits as usize, false);
        let sips = [Bloom::sip_new(), Bloom::sip_new()];
        Bloom {
            bitmap: bitmap,
            bitmap_bits: bitmap_bits,
            k_num: k_num,
            sips: sips,
        }
    }

    pub fn from_bytes(bytes: &[u8], k_num: u32) -> Bloom {
        let bitmap_size = bytes.len();
        let bitmap_bits = (bitmap_size as u64) * 8u64;
        let bitmap = BitVec::from_bytes(&bytes);
        let sips = [Bloom::sip_new(), Bloom::sip_new()];
        Bloom {
            bitmap: bitmap,
            bitmap_bits: bitmap_bits,
            k_num: k_num,
            sips: sips,
        }
    }

    pub fn to_bytes(&self) -> (Vec<u8>, u32) {
        (self.bitmap.to_bytes(), self.k_num)
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
            self.bitmap.set(bit_offset, true);
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
            if self.bitmap.get(bit_offset).unwrap() == false {
                return false;
            }
        }
        true
    }

    /// Record the presence of an item in the set,
    /// and return the previous state of this item.
    pub fn check_and_set<T>(&mut self, item: T) -> bool
        where T: Hash
    {
        let mut hashes = [0u64, 0u64];
        let mut found = true;
        for k_i in 0..self.k_num {
            let bit_offset = (self.bloom_hash(&mut hashes, &item, k_i) % self.bitmap_bits) as usize;
            if self.bitmap.get(bit_offset).unwrap() == false {
                found = false;
                self.bitmap.set(bit_offset, true);
            }
        }
        found
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

    /// Clear all of the bits in the filter, removing all keys from the set
    pub fn clear(&mut self) {
        self.bitmap.clear()
    }

    fn sip_new() -> SipHasher {
        SipHasher::new()
    }
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
fn bloom_test_check_and_set() {
    let mut bloom = Bloom::new(10, 80);
    let key: &Vec<u8> = &rand::thread_rng().gen_iter::<u8>().take(16).collect();
    assert!(bloom.check_and_set(key) == false);
    assert!(bloom.check_and_set(key.clone()) == true);
}

#[test]
fn bloom_test_clear() {
    let mut bloom = Bloom::new(10, 80);
    let key: &Vec<u8> = &rand::thread_rng().gen_iter::<u8>().take(16).collect();
    bloom.set(&key);
    assert!(bloom.check(&key) == true);
    bloom.clear();
    assert!(bloom.check(&key) == false);
}

#[test]
fn bloom_recreate() {
    let key: Vec<u8> = vec![0, 5, 8, 10];
    let (bytes, k_num) = {
        let mut bloom = Bloom::new(16, 1000);
        bloom.set(&key);
        assert!(bloom.check(&key));

        bloom.to_bytes()
    };
    
    let bloom = Bloom::from_bytes(&bytes, k_num);

    assert!(bloom.check(&key));
}
