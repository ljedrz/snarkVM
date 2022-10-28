// Copyright (C) 2019-2022 Aleo Systems Inc.
// This file is part of the snarkVM library.

// The snarkVM library is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// The snarkVM library is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with the snarkVM library. If not, see <https://www.gnu.org/licenses/>.

use std::mem;

use circular_queue::CircularQueue;

/// A single "sub-atomic" entry in a storage journal; it contains the key,
/// the old value and the new value.
pub struct Entry<K, V> {
    key: K,
    // If we could assert that the keys of intertions would always be unique
    // (i.e. that we never update values), we could instead just have a field
    // representing the operation (insert/delete) and the single corresponding
    // value (the new one for insert and the old one for delete). However, in
    // the presence of updates we need to store 2 values for every entry, which
    // is costly in terms of memory use, but also - more importantly - runtime,
    // as the old value needs to be queried for every operation.
    old_value: Option<V>,
    new_value: Option<V>,
}

impl<K, V> Entry<K, V> {
    fn new(key: K, old_value: Option<V>, new_value: Option<V>) -> Self {
        Self { key, old_value, new_value }
    }
}

/// A single atomic batch consisting of a number of entries.
pub struct Batch<K, V> {
    entries: Vec<Entry<K, V>>,
}

impl<K, V> Default for Batch<K, V> {
    fn default() -> Self {
        Self { entries: vec![] }
    }
}

/// A storage journal that can be used to perform rollbacks within the
/// capacity it is created with.
pub struct Journal<K, V> {
    log: CircularQueue<Batch<K, V>>,
    current_batch: Batch<K, V>,
}

impl<K, V> Journal<K, V> {
    /// Creates a new [Journal] with the given capacity.
    pub fn new(capacity: usize) -> Self {
        Self { log: CircularQueue::with_capacity(capacity), current_batch: Default::default() }
    }

    /// Returns the length of the [Journal], i.e. the number of batches
    /// that it contains.
    pub fn len(&self) -> usize {
        self.log.len()
    }

    /// Returns `true` if the [Journal] contains no batches.
    pub fn is_empty(&self) -> bool {
        self.log.is_empty()
    }

    /// Loads the given batches into the [Journal].
    pub fn load<I: Iterator<Item = Batch<K, V>>>(&mut self, batches: I) {
        // This operation can only be performed between atomic batches.
        assert!(self.current_batch.entries.is_empty());

        for batch in batches {
            for entry in batch.entries {
                self.insert_entry(entry);
            }
            self.finalize_batch();
        }
    }

    /// Inserts a single entry into the current batch.
    pub fn insert_entry(&mut self, entry: Entry<K, V>) {
        self.current_batch.entries.push(entry);
    }

    /// Inserts a batch into the log.
    fn insert_batch(&mut self, batch: Batch<K, V>) {
        self.log.push(batch);
    }

    /// Marks the current batch as complete, and prepares a new, empty
    /// batch for further entries.
    pub fn finalize_batch(&mut self) {
        self.log.push(mem::take(&mut self.current_batch));
    }

    /// Returns an iterator over the batches in the [Journal], starting from
    /// the most recent one.
    pub fn latest_batches(&self) -> impl Iterator<Item = &Batch<K, V>> {
        self.log.iter()
    }

    /// Truncates the journal to the given length.
    pub fn truncate(&mut self, len: usize) {
        // This operation can only be performed between atomic batches.
        assert!(self.current_batch.entries.is_empty());

        let new_log = CircularQueue::with_capacity(self.log.capacity());
        let mut old_log = mem::replace(&mut self.log, new_log);

        for batch in old_log.asc_iter_mut().take(len) {
            self.insert_batch(mem::take(batch));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_ops() {
        let mut journal: Journal<u8, String> = Journal::new(64);

        // Insert 10 new records into the current batch.
        for (k, v) in (0..10).map(|n| (n, n.to_string())) {
            journal.insert_entry(Entry::new(k, None, Some(v)));
        }

        // There shouldn't be any entries yet.
        assert!(journal.is_empty());

        // Finalize the current batch.
        journal.finalize_batch();

        // There should be 1 finalized batch now.
        assert_eq!(journal.log.len(), 1);
        // That batch should contain 10 entries.
        assert_eq!(journal.latest_batches().next().unwrap().entries.len(), 10);

        // Remove 5 records.
        for k in 0..10 {
            // Remove every second entry.
            if k % 2 == 1 {
                let old_v = k.to_string();
                journal.insert_entry(Entry::new(k, Some(old_v), None));
            }
        }

        // Finalize the current batch.
        journal.finalize_batch();

        // There should be 2 finalized batches now.
        assert_eq!(journal.log.len(), 2);
        // The last batch should contain 5 entries.
        assert_eq!(journal.latest_batches().next().unwrap().entries.len(), 5);

        // Go through the current batches of operations in reverse, and list their
        // entries in reverse as well, showing what a rollback would look like.
        println!("Journal state check 1:");
        for (i, batch) in journal.latest_batches().enumerate() {
            println!("  batch -{}:", i + 1);
            for entry in batch.entries.iter().rev() {
                println!("{}: {:?} -> {:?}", entry.key, entry.new_value, entry.old_value);
            }
        }

        // Truncate the journal, simulating a rollback by a single batch.
        journal.truncate(1);

        // There should be no finalized batches now.
        assert_eq!(journal.len(), 1);

        // Insert 10 new records into the current batch.
        for (k, v) in (10..20).map(|n| (n, n.to_string())) {
            journal.insert_entry(Entry::new(k, None, Some(v)));
        }

        // Finalize the current batch.
        journal.finalize_batch();

        // There should be 1 finalized batch now.
        assert_eq!(journal.log.len(), 2);
        // The last batch should contain 10 entries.
        assert_eq!(journal.latest_batches().next().unwrap().entries.len(), 10);

        // Go through the current batches of operations in reverse, and list their
        // entries in reverse as well, showing what a rollback would look like.
        println!("Journal state check 2:");
        for (i, batch) in journal.latest_batches().enumerate() {
            println!("  batch -{}:", i + 1);
            for entry in batch.entries.iter().rev() {
                println!("{}: {:?} -> {:?}", entry.key, entry.new_value, entry.old_value);
            }
        }
    }
}
