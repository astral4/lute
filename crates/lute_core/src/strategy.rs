//! How a map turns a key's hash into an entry index.

use crate::cow::CowSlice;
use crate::kernel::{
    PACKED_MAX, PACKED_SHIFTS, PACKED_SLOTS, bucket_count, bucket_shift, slot_count,
};

/// Tables produced by a construction strategy and embedded in generated code.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub enum BakedStrategy {
    /// A bit window of the hash indexes `table`.
    Packed {
        /// One entry index per slot.
        table: [u8; PACKED_SLOTS],
        /// Which bit window of the hash selects the slot.
        shift: u32,
    },
    /// The hash selects a bucket and the bucket's pilot selects a slot. Slots past the entries are remapped back.
    Pilots {
        /// One pilot per bucket.
        pilots: &'static [u16],
        /// Entry indices for the overflow slots.
        remap: &'static [u16],
    },
}

/// The [`Tables::bucket_shift`] value marking a map with no pilot table.
/// Shifting by 64 is invalid, so this doesn't collide with a real shift.
pub(crate) const NO_PILOTS: u32 = 64;

#[derive(Clone)]
pub(crate) struct Tables {
    /// The slot-to-entry table in the packed strategy.
    pub(crate) packed: [u8; PACKED_SLOTS],
    /// The window shift in the packed strategy.
    pub(crate) packed_shift: u32,

    /// [`bucket_shift`] of the bucket count in the pilot strategy, or [`NO_PILOTS`] in the packed strategy.
    pub(crate) bucket_shift: u32,
    /// One pilot per bucket in the pilot strategy.
    pub(crate) pilots: CowSlice<u16>,
    /// Entry indices for the overflow slots in the pilot strategy.
    pub(crate) remap: CowSlice<u16>,
    /// The entry count plus `remap.len()` in the pilot strategy.
    pub(crate) slots: u32,
}

impl Tables {
    /// Builds the packed strategy.
    pub(crate) const fn packed(table: [u8; PACKED_SLOTS], shift: u32) -> Self {
        const NO_TABLE: CowSlice<u16> = CowSlice::Borrowed(&[]);

        Self {
            packed: table,
            packed_shift: shift,
            bucket_shift: NO_PILOTS,
            pilots: NO_TABLE,
            remap: NO_TABLE,
            // Only the pilot strategy reads this.
            slots: 0,
        }
    }

    /// Builds the pilot strategy.
    ///
    /// # Panics
    ///
    /// Panics if `pilots` has less than two entries or its length is not a power of two.
    pub(crate) const fn pilots(
        pilots: CowSlice<u16>,
        remap: CowSlice<u16>,
        entries: usize,
    ) -> Self {
        assert!(
            pilots.len() >= 2 && pilots.len().is_power_of_two(),
            "pilot table length must be a power of two of at least 2"
        );

        let bucket_shift = bucket_shift(pilots.len());
        #[expect(
            clippy::cast_possible_truncation,
            reason = "`slot_count(MAX_LEN)` is 66191, which fits in `u32`"
        )]
        let slots = (entries + remap.len()) as u32;

        Self {
            packed: [0; PACKED_SLOTS],
            packed_shift: 0,
            bucket_shift,
            pilots,
            remap,
            slots,
        }
    }

    /// Returns whether the packed strategy is being used.
    pub(crate) const fn is_packed(&self) -> bool {
        self.bucket_shift == NO_PILOTS
    }

    /// Adopts tables from generated code, verifying that they describe a map of `entries` entries.
    /// The parts must come from an actual construction.
    pub(crate) const fn from_baked(strategy: BakedStrategy, entries: usize) -> Self {
        match strategy {
            BakedStrategy::Packed { table, shift } => {
                assert!(
                    entries <= PACKED_MAX,
                    "packed strategy used for an entry count that requires a pilot table"
                );
                if entries != 0 {
                    assert!(shift < PACKED_SHIFTS, "packed window shift out of range");
                    assert!(
                        packed_targets_valid(&table, entries),
                        "packed value out of range"
                    );
                }
                Self::packed(table, shift)
            }
            BakedStrategy::Pilots { pilots, remap } => {
                assert!(!pilots.is_empty(), "pilot strategy without a pilot table");
                assert!(
                    pilots.len() == bucket_count(entries),
                    "pilot table length must match the bucket count for the entry count"
                );
                assert!(
                    remap.len() == slot_count(entries) - entries,
                    "remap length must match the slot slack for the entry count"
                );
                assert!(
                    remap_targets_valid(remap, entries),
                    "remap value out of range"
                );
                Self::pilots(
                    CowSlice::Borrowed(pilots),
                    CowSlice::Borrowed(remap),
                    entries,
                )
            }
        }
    }
}

/// Returns whether every packed slot holds a valid index into `entries` entries.
const fn packed_targets_valid(table: &[u8; PACKED_SLOTS], entries: usize) -> bool {
    let mut i = 0;
    while i < table.len() {
        if table[i] as usize >= entries {
            return false;
        }
        i += 1;
    }
    true
}

/// Returns whether every remap value is a valid index into `entries` entries.
const fn remap_targets_valid(remap: &[u16], entries: usize) -> bool {
    let mut i = 0;
    while i < remap.len() {
        if remap[i] as usize >= entries {
            return false;
        }
        i += 1;
    }
    true
}
