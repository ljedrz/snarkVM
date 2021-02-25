// Copyright (C) 2019-2021 Aleo Systems Inc.
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

use snarkvm_errors::objects::StorageError;
use snarkvm_models::objects::{Storage, StorageBatchOp, StorageOp};
use snarkvm_storage::NUM_COLS;

use rocksdb::{ColumnFamily, ColumnFamilyDescriptor, IteratorMode, Options, WriteBatch, DB};
use std::path::Path;

fn convert_err(err: rocksdb::Error) -> StorageError {
    StorageError::Crate("rocksdb", err.to_string())
}

/// A low-level struct for storing state used by the system.
pub struct RocksDb {
    pub db: DB,
    pub cf_names: Vec<String>,
}

impl Storage for RocksDb {
    fn open(path: Option<&Path>, secondary_path: Option<&Path>) -> Result<Self, StorageError> {
        assert!(path.is_some(), "RocksDB must have an associated filesystem path!");
        let primary_path = path.unwrap();

        if let Some(secondary_path) = secondary_path {
            RocksDb::open_secondary_cf(primary_path, secondary_path, NUM_COLS)
        } else {
            RocksDb::open_cf(primary_path, NUM_COLS)
        }
    }

    fn get(&self, col: u32, key: &[u8]) -> Result<Option<Vec<u8>>, StorageError> {
        Ok(self.db.get_cf(self.get_cf_ref(col), key).map_err(convert_err)?)
    }

    fn get_col(&self, col: u32) -> Result<Vec<(Box<[u8]>, Box<[u8]>)>, StorageError> {
        Ok(self.db.iterator_cf(self.get_cf_ref(col), IteratorMode::Start).collect())
    }

    fn get_keys(&self, col: u32) -> Result<Vec<Box<[u8]>>, StorageError> {
        Ok(self
            .db
            .iterator_cf(self.get_cf_ref(col), IteratorMode::Start)
            .map(|(k, _v)| k)
            .collect())
    }

    fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&self, col: u32, key: K, value: V) -> Result<(), StorageError> {
        Ok(self.db.put_cf(self.get_cf_ref(col), key, value).map_err(convert_err)?)
    }

    fn batch(&self, transaction: StorageBatchOp) -> Result<(), StorageError> {
        let mut batch = WriteBatch::default();

        for operation in transaction.0 {
            match operation {
                StorageOp::Insert { col, key, value } => {
                    let cf = self.get_cf_ref(col);
                    batch.put_cf(cf, &key, value);
                }
                StorageOp::Delete { col, key } => {
                    let cf = self.get_cf_ref(col);
                    batch.delete_cf(cf, &key);
                }
            };
        }

        self.db.write(batch).map_err(convert_err)?;

        Ok(())
    }

    fn exists(&self, col: u32, key: &[u8]) -> bool {
        match self.db.get_cf(self.get_cf_ref(col), key) {
            Ok(val) => val.is_some(),
            Err(_) => false,
        }
    }

    fn destroy(&self) -> Result<(), StorageError> {
        let path = self.db.path();

        let mut storage_opts = Options::default();
        storage_opts.create_missing_column_families(true);
        storage_opts.create_if_missing(true);

        Ok(DB::destroy(&storage_opts, path).map_err(convert_err)?)
    }
}

impl Drop for RocksDb {
    fn drop(&mut self) {
        let _ = self.destroy();
    }
}

impl RocksDb {
    /// Opens storage from the given path with its given names. If storage does not exists,
    /// it creates a new storage file at the given path with its given names, and opens it.
    /// If RocksDB fails to open, returns [StorageError](snarkvm_errors::storage::StorageError).
    pub fn open_cf<P: AsRef<Path>>(path: P, num_cfs: u32) -> Result<Self, StorageError> {
        let mut cfs = Vec::with_capacity(num_cfs as usize);
        let mut cf_names: Vec<String> = Vec::with_capacity(cfs.len());

        for column in 0..num_cfs {
            let column_name = format!("col{}", column.to_string());

            let mut cf_opts = Options::default();
            cf_opts.set_max_write_buffer_number(16);

            cfs.push(ColumnFamilyDescriptor::new(&column_name, cf_opts));
            cf_names.push(column_name);
        }

        let mut storage_opts = Options::default();
        storage_opts.increase_parallelism(3);
        storage_opts.create_missing_column_families(true);
        storage_opts.create_if_missing(true);

        let storage = DB::open_cf_descriptors(&storage_opts, path, cfs).map_err(convert_err)?;

        Ok(Self { db: storage, cf_names })
    }

    /// Opens a secondary storage instance from the given path with its given names.
    /// If RocksDB fails to open, returns [StorageError](snarkvm_errors::storage::StorageError).
    pub fn open_secondary_cf<P: AsRef<Path> + Clone>(
        primary_path: P,
        secondary_path: P,
        num_cfs: u32,
    ) -> Result<Self, StorageError> {
        let mut cf_names: Vec<String> = Vec::with_capacity(num_cfs as usize);

        for column in 0..num_cfs {
            let column_name = format!("col{}", column.to_string());

            cf_names.push(column_name);
        }

        let mut storage_opts = Options::default();
        storage_opts.increase_parallelism(2);

        let storage = DB::open_cf_as_secondary(&storage_opts, primary_path, secondary_path, cf_names.clone())
            .map_err(convert_err)?;

        storage.try_catch_up_with_primary().map_err(convert_err)?;

        Ok(Self { db: storage, cf_names })
    }

    /// Returns the column family reference from a given index.
    /// If the given index does not exist, returns [None](std::option::Option).
    pub(crate) fn get_cf_ref(&self, index: u32) -> &ColumnFamily {
        self.db
            .cf_handle(&self.cf_names[index as usize])
            .expect("the column family exists")
    }
}
