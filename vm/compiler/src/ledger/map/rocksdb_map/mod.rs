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

mod iter;

use crate::ledger::map::{Map, MapReader};
use console::network::prelude::*;
use snarkvm_utilities::sync::Mutex;

use std::{
    borrow::{Borrow, Cow},
    collections::HashMap,
    hash::Hash,
    marker::PhantomData,
    sync::Arc,
};

pub const PREFIX_LEN: usize = 4; // N::NETWORK_ID (u16) + DataID (u16)

///
/// An instance of a RocksDB database.
///
#[derive(Clone)]
pub struct RocksDB {
    rocksdb: Arc<rocksdb::DB>,
    context: Vec<u8>,
    batches: Arc<Mutex<HashMap<usize, rocksdb::WriteBatch>>>,
    // _phantom: PhantomData<A>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum DataID {
    BlockHeaders = 0,
    BlockHeights,
    BlockTransactions,
    Commitments,
    LedgerRoots,
    Records,
    SerialNumbers,
    Transactions,
    Transitions,
    Shares,
    #[cfg(test)]
    Test,
}

impl From<u16> for DataID {
    fn from(id: u16) -> Self {
        match id {
            0 => Self::BlockHeaders,
            1 => Self::BlockHeights,
            2 => Self::BlockTransactions,
            3 => Self::Commitments,
            4 => Self::LedgerRoots,
            5 => Self::Records,
            6 => Self::SerialNumbers,
            7 => Self::Transactions,
            8 => Self::Transitions,
            9 => Self::Shares,
            x => panic!("Unexpected map id: {}", x),
        }
    }
}

#[derive(Clone)]
pub struct DataMap<K: Serialize + DeserializeOwned, V: Serialize + DeserializeOwned> {
    pub(super) storage: RocksDB,
    pub(super) context: Vec<u8>,
    pub(super) _phantom: PhantomData<(K, V)>,
}

impl<K: Serialize + DeserializeOwned, V: Serialize + DeserializeOwned> DataMap<K, V> {
    #[inline]
    fn create_prefixed_key<Q>(&self, key: &Q) -> Result<Vec<u8>>
    where
        K: Borrow<Q>,
        Q: Serialize + ?Sized,
    {
        let mut raw_key = self.context.clone();
        bincode::serialize_into(&mut raw_key, &key)?;

        Ok(raw_key)
    }

    fn get_raw<'a, Q>(&'a self, key: &Q) -> Result<Option<rocksdb::DBPinnableSlice<'a>>>
    where
        K: Borrow<Q>,
        Q: Serialize + ?Sized,
    {
        let raw_key = self.create_prefixed_key(key)?;
        match self.storage.rocksdb.get_pinned(&raw_key)? {
            Some(data) => Ok(Some(data)),
            None => Ok(None),
        }
    }
}

impl<
    K: Clone + PartialEq + Eq + Hash + Serialize + for<'de> Deserialize<'de>,
    V: Clone + PartialEq + Eq + Serialize + for<'de> Deserialize<'de>,
> FromIterator<(K, V)> for DataMap<K, V>
{
    /// Initializes a new `DataMap` from the given iterator.
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        todo!()
    }
}

impl<
    'a,
    K: 'a + Clone + PartialEq + Eq + Hash + Serialize + for<'de> Deserialize<'de>,
    V: 'a + Clone + PartialEq + Eq + Serialize + for<'de> Deserialize<'de>,
> Map<'a, K, V> for DataMap<K, V>
{
    ///
    /// Inserts the given key-value pair into the map.
    ///
    fn insert(&mut self, key: K, value: V) -> Result<()> {
        let raw_key = self.create_prefixed_key(&key)?;
        let raw_value = bincode::serialize(&value)?;
        self.storage.rocksdb.put(&raw_key, &raw_value)?;

        Ok(())
    }

    ///
    /// Removes the key-value pair for the given key from the map.
    ///
    fn remove<Q>(&mut self, key: &Q) -> Result<()>
    where
        K: Borrow<Q>,
        Q: PartialEq + Eq + Hash + Serialize + ?Sized,
    {
        let raw_key = self.create_prefixed_key(key)?;
        self.storage.rocksdb.delete(&raw_key)?;

        Ok(())
    }
}

impl<
    'a,
    K: 'a + Clone + PartialEq + Eq + Hash + Serialize + for<'de> Deserialize<'de>,
    V: 'a + Clone + PartialEq + Eq + Serialize + for<'de> Deserialize<'de>,
> MapReader<'a, K, V> for DataMap<K, V>
{
    type Iterator = iter::Iter<'a, K, V>;
    type Keys = iter::Keys<'a, K>;
    type Values = iter::Values<'a, V>;

    ///
    /// Returns `true` if the given key exists in the map.
    ///
    fn contains_key<Q>(&self, key: &Q) -> Result<bool>
    where
        K: Borrow<Q>,
        Q: PartialEq + Eq + Hash + Serialize + ?Sized,
    {
        self.get_raw(key).map(|v| v.is_some())
    }

    ///
    /// Returns the value for the given key from the map, if it exists.
    ///
    fn get<Q>(&'a self, key: &Q) -> Result<Option<Cow<'a, V>>>
    where
        K: Borrow<Q>,
        Q: PartialEq + Eq + Hash + Serialize + ?Sized,
    {
        match self.get_raw(key) {
            Ok(Some(bytes)) => Ok(Some(Cow::Owned(bincode::deserialize(&bytes)?))),
            Ok(None) => Ok(None),
            Err(e) => Err(e),
        }
    }

    ///
    /// Returns an iterator visiting each key-value pair in the map.
    ///
    fn iter(&'a self) -> Self::Iterator {
        iter::Iter::new(self.storage.rocksdb.prefix_iterator(&self.context))
    }

    ///
    /// Returns an iterator over each key in the map.
    ///
    fn keys(&'a self) -> Self::Keys {
        iter::Keys::new(self.storage.rocksdb.prefix_iterator(&self.context))
    }

    ///
    /// Returns an iterator over each value in the map.
    ///
    fn values(&'a self) -> Self::Values {
        iter::Values::new(self.storage.rocksdb.prefix_iterator(&self.context))
    }
}
