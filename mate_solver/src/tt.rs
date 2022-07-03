use std::mem::MaybeUninit;

/// 置換表。
pub struct Tt<V> {
    pub present: Vec<u64>,
    pub table: Vec<MaybeUninit<(u64, V)>>,
}

impl<V: Copy> Tt<V> {
    /// size は 2 ベキでなければならない。
    pub fn new(size: usize) -> Self {
        assert_eq!(size % 8, 0);
        assert!(size.is_power_of_two());
        Self {
            present: vec![0; size / 8],
            table: vec![MaybeUninit::uninit(); size],
        }
    }
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.table.len()
    }
    pub fn fetch(&self, key: u64) -> Option<V> {
        let len = self.len();
        let index = key % len as u64;
        if (self.present[index as usize / 8] & 1 << (index % 8)) == 0 {
            return None;
        }
        let &(thiskey, value) = unsafe { self.table[index as usize].assume_init_ref() };
        if thiskey == key {
            Some(value)
        } else {
            None
        }
    }
    pub fn insert(&mut self, key: u64, value: V) {
        let len = self.len();
        let index = key % len as u64;
        self.present[index as usize / 8] |= 1 << (index % 8);
        self.table[index as usize].write((key, value));
    }
}

/// df-pn 用の置換表。
pub type DfPnTable = Tt<(u32, u32)>;

/// 最短手順探索用の置換表。
pub type EvalTable = Tt<crate::eval::value::Value>;
