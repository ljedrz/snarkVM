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

use super::*;

use std::borrow::Cow;

/// An iterator over all key-value pairs in a data map.
pub struct Iter<'a, K, V> {
    db_iter: rocksdb::DBIterator<'a>,
    _phantom: PhantomData<(K, V)>,
}

impl<'a, K: 'a + DeserializeOwned + Clone, V: 'a + DeserializeOwned + Clone> Iter<'a, K, V> {
    pub(super) fn new(db_iter: rocksdb::DBIterator<'a>) -> Self {
        Self { db_iter, _phantom: PhantomData }
    }
}

impl<'a, K: 'a + DeserializeOwned + Clone, V: 'a + DeserializeOwned + Clone> Iterator for Iter<'a, K, V> {
    type Item = (Cow<'a, K>, Cow<'a, V>);

    fn next(&mut self) -> Option<Self::Item> {
        let (key, value) = self.db_iter.next()?;
        let key = bincode::deserialize(&key[PREFIX_LEN..]).ok()?;
        let value = bincode::deserialize(&value).ok()?;

        Some((Cow::Owned(key), Cow::Owned(value)))
    }
}

/// An iterator over the keys of a prefix.
pub struct Keys<'a, K> {
    db_iter: rocksdb::DBIterator<'a>,
    _phantom: PhantomData<K>,
}

impl<'a, K: 'a + DeserializeOwned> Keys<'a, K> {
    pub(crate) fn new(db_iter: rocksdb::DBIterator<'a>) -> Self {
        Self { db_iter, _phantom: PhantomData }
    }
}

impl<'a, K: 'a + DeserializeOwned + Clone> Iterator for Keys<'a, K> {
    type Item = Cow<'a, K>;

    fn next(&mut self) -> Option<Self::Item> {
        let (key, _) = self.db_iter.next()?;
        let key = bincode::deserialize(&key[PREFIX_LEN..]).ok()?;

        Some(Cow::Owned(key))
    }
}

/// An iterator over the values of a prefix.
pub struct Values<'a, V> {
    db_iter: rocksdb::DBIterator<'a>,
    _phantom: PhantomData<V>,
}

impl<'a, V: 'a + DeserializeOwned> Values<'a, V> {
    pub(crate) fn new(db_iter: rocksdb::DBIterator<'a>) -> Self {
        Self { db_iter, _phantom: PhantomData }
    }
}

impl<'a, V: 'a + DeserializeOwned + Clone> Iterator for Values<'a, V> {
    type Item = Cow<'a, V>;

    fn next(&mut self) -> Option<Self::Item> {
        let (_, value) = self.db_iter.next()?;
        let value = bincode::deserialize(&value).ok()?;

        Some(Cow::Owned(value))
    }
}
