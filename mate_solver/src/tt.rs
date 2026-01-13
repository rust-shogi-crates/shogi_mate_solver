use std::mem::MaybeUninit;

/// 置換表。1 バケットにつき 4 個のエントリーがある。
pub struct Tt<V> {
    pub sizes: Vec<u8>,
    pub table: Vec<MaybeUninit<(u64, V)>>,
}

impl<V: Copy> Tt<V> {
    /// size は 2 ベキでなければならない。
    pub fn new(size: usize) -> Self {
        assert_eq!(size % 2, 0);
        assert!(size.is_power_of_two());
        Self {
            sizes: vec![0; size / 2], // size for index, 4 bits for each entry
            table: vec![MaybeUninit::uninit(); 4 * size],
        }
    }
    #[inline(always)]
    pub fn size(&self) -> usize {
        self.table.len() / 4
    }
    pub fn fetch(&self, key: u64) -> Option<V> {
        let len = self.size();
        let index = key % len as u64;
        let size = ((self.sizes[index as usize / 2] >> (4 * (index % 2))) & 0x7) as usize;
        debug_assert!(size <= 4);
        for i in 0..size {
            let &(thiskey, value) = unsafe { self.table[4 * index as usize + i].assume_init_ref() };
            if thiskey == key {
                return Some(value);
            }
        }
        None
    }
    pub fn insert(&mut self, key: u64, value: V) {
        let len = self.size();
        let index = key % len as u64;
        let size = ((self.sizes[index as usize / 2] >> (4 * (index % 2))) & 0x7) as usize;
        debug_assert!(size <= 4);
        for i in 0..size {
            let &(thiskey, _) = unsafe { self.table[4 * index as usize + i].assume_init_ref() };
            if thiskey == key {
                // Replace this entry
                self.table[4 * index as usize + i].write((key, value));
                return;
            }
        }
        let mut pos = 3;
        if size <= 3 {
            let newsize = (size + 1) as u8;
            self.sizes[index as usize / 2] &= if index.is_multiple_of(2) { 0xf0 } else { 0x0f };
            self.sizes[index as usize / 2] |= newsize << (4 * if index.is_multiple_of(2) { 0 } else { 1 });
            pos = size;
        }

        self.table[4 * index as usize + pos].write((key, value));
    }

    pub fn clear(&mut self) {
        for v in &mut self.sizes {
            *v = 0;
        }
    }
}

/// df-pn 用の置換表。
pub type DfPnTable = Tt<(u32, u32)>;

/// 最短手順探索用の置換表。
pub type EvalTable = Tt<(crate::eval::value::Value, Option<shogi_core::Move>)>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tt_insertion_works_0() {
        let size = 1 << 16;
        let key0 = 5;
        let value0 = 3;
        let key1 = 5 + 2 * size as u64;
        let value1 = 100;
        let mut tt = Tt::new(size);

        assert_eq!(tt.fetch(key0), None);
        assert_eq!(tt.fetch(key1), None);

        tt.insert(key0, value0);
        assert_eq!(tt.fetch(key0), Some(value0));
        assert_eq!(tt.fetch(key1), None);

        tt.insert(key1, value1);
        assert_eq!(tt.fetch(key0), Some(value0));
        assert_eq!(tt.fetch(key1), Some(value1));
    }

    #[test]
    fn tt_update_works_0() {
        let size = 1 << 16;
        let key = 5;
        let value0 = 3;
        let value1 = 100;
        let mut tt = Tt::new(size);

        tt.insert(key, value0);
        tt.insert(key, value1);
        assert_eq!(tt.fetch(key), Some(value1))
    }
}
