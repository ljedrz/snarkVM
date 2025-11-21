// Copyright (c) 2019-2025 Provable Inc.
// This file is part of the snarkVM library.

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at:

// http://www.apache.org/licenses/LICENSE-2.0

// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::{
    io::{Read, Write},
    marker::PhantomData,
};

use rayon::prelude::*;
use serde::{de::DeserializeOwned, Deserializer, Serialize, Serializer};

pub use crate::{FromBytes, ToBytes};
use crate::{SerializationError, serialize::traits::*};

/// Serialize a Vector's elements without serializing the Vector's length
/// If you want to serialize the full Vector, use `CanonicalSerialize for Vec<T>`
pub fn serialize_vec_without_len<'a>(
    src: impl Iterator<Item = &'a (impl CanonicalSerialize + 'a)>,
    mut writer: impl Write,
    compress: Compress,
) -> Result<(), SerializationError> {
    for elem in src {
        CanonicalSerialize::serialize_with_mode(elem, &mut writer, compress)?;
    }
    Ok(())
}

/// Serialize a Vector's element sizes without serializing the Vector's length
/// If you want to serialize the full Vector, use `CanonicalSerialize for Vec<T>`
pub fn serialized_vec_size_without_len(src: &[impl CanonicalSerialize], compress: Compress) -> usize {
    if src.is_empty() { 0 } else { src.len() * CanonicalSerialize::serialized_size(&src[0], compress) }
}

/// Deserialize a Vector's elements without deserializing the Vector's length
/// If you want to deserialize the full Vector, use `CanonicalDeserialize for Vec<T>`
pub fn deserialize_vec_without_len<T: CanonicalDeserialize>(
    mut reader: impl Read,
    compress: Compress,
    validate: Validate,
    len: usize,
) -> Result<Vec<T>, SerializationError> {
    (0..len).map(|_| CanonicalDeserialize::deserialize_with_mode(&mut reader, compress, validate)).collect()
}
 
pub fn serialize_huge_vec<S, T>(data: &[T], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: Serialize + Send + Sync,
{
    use serde::ser::SerializeSeq;

    // Configuration
    const ITEMS_PER_THREAD: usize = 2048;
    const THREADS_PER_BATCH: usize = 128;
    const BATCH_SIZE: usize = ITEMS_PER_THREAD * THREADS_PER_BATCH;

    // Calculate how many "Blobs" we will produce
    let total_chunks = data.len().div_ceil(ITEMS_PER_THREAD);

    // Crucial: We start a Sequence *inside* the field value
    let mut seq = serializer.serialize_seq(Some(total_chunks))?;

    // Stream through the data in large batches to keep RAM bounded
    for batch_slice in data.chunks(BATCH_SIZE) {
        // Parallel CPU work: Serialize T -> Vec<u8>
        let batch_blobs: Vec<Vec<u8>> = batch_slice
            .par_chunks(ITEMS_PER_THREAD)
            .map(|chunk| bincode::serialize(chunk).unwrap())
            .collect();

        // Sequential I/O work: Write Vec<u8> -> Disk
        for blob in batch_blobs {
            // Wrap in serde_bytes to ensure efficient byte-array writing
            seq.serialize_element(&serde_bytes::Bytes::new(&blob))?;
        }
    }

    seq.end()
}

pub fn deserialize_huge_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned + Send,
{
    struct ChunkVisitor<T>(PhantomData<T>);

    impl<'de, T> serde::de::Visitor<'de> for ChunkVisitor<T>
    where
        T: DeserializeOwned + Send,
    {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a sequence of compressed byte chunks")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'de>,
        {
            let mut all_items = Vec::new();

            // Iterate through the Sequence of Byte Arrays
            while let Some(chunk) = seq.next_element::<serde_bytes::ByteBuf>()? {
                // Deserialize the blob back into a Vec<T>
                // Note: You could arguably parallelize this step too if you
                // collected the blobs first, but usually disk read is the limit.
                let chunk_items: Vec<T> = bincode::deserialize(&chunk)
                    .map_err(serde::de::Error::custom)?;
                all_items.extend(chunk_items);
            }

            Ok(all_items)
        }
    }

    deserializer.deserialize_seq(ChunkVisitor(PhantomData))
}
