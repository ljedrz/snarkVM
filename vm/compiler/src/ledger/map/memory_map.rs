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

use crate::ledger::map::{BatchOperation, Map, MapRead};
use console::network::prelude::*;
use indexmap::IndexMap;

use core::{borrow::Borrow, hash::Hash};
use indexmap::map;
use parking_lot::{Mutex, RwLock};
use std::{
    borrow::Cow,
    mem,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

#[derive(Clone)]
pub struct MemoryMap<
    K: Copy + Clone + PartialEq + Eq + Hash + Serialize + for<'de> Deserialize<'de> + Sync,
    V: Clone + PartialEq + Eq + Serialize + for<'de> Deserialize<'de> + Sync,
> {
    pub(super) map: Arc<RwLock<IndexMap<K, V>>>,
    batch_in_progress: Arc<AtomicBool>,
    atomic_batch: Arc<Mutex<Vec<BatchOperation>>>,
}

impl<
    'a,
    K: 'a + Copy + Clone + PartialEq + Eq + Hash + Serialize + for<'de> Deserialize<'de> + Send + Sync,
    V: 'a + Clone + PartialEq + Eq + Serialize + for<'de> Deserialize<'de> + Send + Sync,
> Map<'a, K, V> for MemoryMap<K, V>
{
    ///
    /// Creates a new instance of a `Map`.
    ///
    fn new(shared_batch_ops: Arc<Mutex<Vec<BatchOperation>>>) -> Self {
        Self { map: Default::default(), batch_in_progress: Default::default(), atomic_batch: shared_batch_ops }
    }

    ///
    /// Inserts the given key-value pair into the map.
    ///
    fn insert(&self, key: K, value: V) -> Result<()> {
        if self.batch_in_progress.load(Ordering::SeqCst) {
            self.atomic_batch
                .lock()
                .push(BatchOperation::Put(bincode::serialize(&key).unwrap(), bincode::serialize(&value).unwrap()));
        } else {
            self.map.write().insert(key, value);
        }

        Ok(())
    }

    ///
    /// Removes the key-value pair for the given key from the map.
    ///
    fn remove<Q>(&self, key: &Q) -> Result<()>
    where
        K: Borrow<Q>,
        Q: PartialEq + Eq + Hash + Serialize + ?Sized,
    {
        if self.batch_in_progress.load(Ordering::SeqCst) {
            self.atomic_batch.lock().push(BatchOperation::Delete(bincode::serialize(&key).unwrap()));
        } else {
            self.map.write().remove(key);
        }

        Ok(())
    }

    ///
    /// Begins an atomic operation. Any further calls to `insert` and `remove` will be queued
    /// without an actual write taking place until `finish_atomic` is called.
    ///
    fn start_atomic(&self) {
        assert!(!self.batch_in_progress.swap(true, Ordering::SeqCst));
        assert!(self.atomic_batch.lock().is_empty());
    }

    ///
    /// Finishes an atomic operation, performing all the queued writes.
    ///
    fn finish_atomic(&self) {
        let operations = mem::take(&mut *self.atomic_batch.lock());

        {
            let mut locked_map = self.map.write();

            // We performed the serialization when queuing operations, so deserialization can be trusted.
            for op in operations {
                match op {
                    BatchOperation::Put(k, v) => {
                        locked_map.insert(bincode::deserialize(&k).unwrap(), bincode::deserialize(&v).unwrap())
                    }
                    BatchOperation::Delete(k) => locked_map.remove::<K>(&bincode::deserialize(&k).unwrap()),
                };
            }
        }

        assert!(self.batch_in_progress.swap(false, Ordering::SeqCst));
    }
}

impl<
    'a,
    K: 'a + Copy + Clone + PartialEq + Eq + Hash + Serialize + for<'de> Deserialize<'de> + Sync,
    V: 'a + Clone + PartialEq + Eq + Serialize + for<'de> Deserialize<'de> + Sync,
> MapRead<'a, K, V> for MemoryMap<K, V>
{
    type Iterator = core::iter::Map<map::IntoIter<K, V>, fn((K, V)) -> (Cow<'a, K>, Cow<'a, V>)>;
    type Keys = core::iter::Map<map::IntoKeys<K, V>, fn(K) -> Cow<'a, K>>;
    type Values = core::iter::Map<map::IntoValues<K, V>, fn(V) -> Cow<'a, V>>;

    ///
    /// Returns `true` if the given key exists in the map.
    ///
    fn contains_key<Q>(&self, key: &Q) -> Result<bool>
    where
        K: Borrow<Q>,
        Q: PartialEq + Eq + Hash + Serialize + ?Sized,
    {
        Ok(self.map.read().contains_key(key))
    }

    ///
    /// Returns the value for the given key from the map, if it exists.
    ///
    fn get<Q>(&'a self, key: &Q) -> Result<Option<Cow<'a, V>>>
    where
        K: Borrow<Q>,
        Q: PartialEq + Eq + Hash + Serialize + ?Sized,
    {
        Ok(self.map.read().get(key).cloned().map(Cow::Owned))
    }

    ///
    /// Returns an iterator visiting each key-value pair in the map.
    ///
    fn iter(&'a self) -> Self::Iterator {
        self.map.read().clone().into_iter().map(|(k, v)| (Cow::Owned(k), Cow::Owned(v)))
    }

    ///
    /// Returns an iterator over each key in the map.
    ///
    fn keys(&'a self) -> Self::Keys {
        self.map.read().clone().into_keys().map(Cow::Owned)
    }

    ///
    /// Returns an iterator over each value in the map.
    ///
    fn values(&'a self) -> Self::Values {
        self.map.read().clone().into_values().map(Cow::Owned)
    }
}

impl<
    K: Copy + Clone + PartialEq + Eq + Hash + Serialize + for<'de> Deserialize<'de> + Sync,
    V: Clone + PartialEq + Eq + Serialize + for<'de> Deserialize<'de> + Sync,
> Deref for MemoryMap<K, V>
{
    type Target = Arc<RwLock<IndexMap<K, V>>>;

    fn deref(&self) -> &Self::Target {
        &self.map
    }
}
