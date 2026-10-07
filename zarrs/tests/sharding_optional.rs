#![allow(missing_docs)]

//! Tests for the `sharding_indexed` codec with optional data types.

use std::fmt::Debug;
use std::num::NonZeroU64;
use std::sync::Arc;

use zarrs::array::codec::ShardingCodecBuilder;
use zarrs::array::{ArrayBuilder, ArraySubset, DataType, ElementOwned, FillValue, data_type};
use zarrs::storage::store::MemoryStore;
use zarrs_codec::{CodecOptions, CodecSpecificOptions, UnboundArrayToBytesCodecTraits};

const SHAPE: [u64; 2] = [8, 6];
const SHARD_SHAPE: [u64; 2] = [4, 6];
const SUBCHUNK_SHAPE: [u64; 2] = [2, 2];

/// Elements at `indices` of an array with shape [`SHAPE`].
fn select<T: Clone>(elements: &[T], indices: &[[u64; 2]]) -> Vec<T> {
    indices
        .iter()
        .map(|[i, j]| elements[usize::try_from(i * SHAPE[1] + j).unwrap()].clone())
        .collect()
}

fn sharding_optional_round_trip<T: ElementOwned + Clone + PartialEq + Debug>(
    data_type: DataType,
    elements: &[T],
) -> Result<(), Box<dyn std::error::Error>> {
    let store = Arc::new(MemoryStore::default());
    let mut builder = ArrayBuilder::new(
        SHAPE.to_vec(),
        SHARD_SHAPE.to_vec(),
        data_type.clone(),
        FillValue::new_optional_null(),
    );
    builder.subchunk_shape(SUBCHUNK_SHAPE.to_vec());
    let array = builder.build(store, "/")?;
    array.store_array_subset(
        &array.subset_all(),
        T::to_array_bytes(&data_type, elements)?,
    )?;

    // Retrieve the entire array (spanning multiple shards)
    assert_eq!(
        array.retrieve_array_subset::<Vec<T>>(&array.subset_all())?,
        elements
    );

    // Retrieve a subset spanning multiple shards and partial subchunks
    let subset = ArraySubset::new_with_ranges(&[3..7, 1..4]);
    let indices: Vec<[u64; 2]> = (3..7).flat_map(|i| (1..4).map(move |j| [i, j])).collect();
    assert_eq!(
        array.retrieve_array_subset::<Vec<T>>(&subset)?,
        select(elements, &indices)
    );
    Ok(())
}

/// Encode a shard, then decode it entirely and partially with a generic indexer spanning multiple subchunks.
async fn sharding_optional_codec<T: ElementOwned + Clone + PartialEq + Debug>(
    data_type: DataType,
    elements: &[T],
) -> Result<(), Box<dyn std::error::Error>> {
    let shard_shape = SHARD_SHAPE.map(|size| NonZeroU64::new(size).unwrap());
    let shard_elements = &elements[..usize::try_from(SHARD_SHAPE.iter().product::<u64>())?];
    let codec = ShardingCodecBuilder::new(
        SUBCHUNK_SHAPE
            .map(|size| NonZeroU64::new(size).unwrap())
            .to_vec(),
        &data_type,
    )
    .build_arc()
    .with_context(
        data_type.clone(),
        FillValue::new_optional_null(),
        &CodecSpecificOptions::default(),
    )?;
    let options = CodecOptions::default();
    let encoded = codec
        .encode(
            T::to_array_bytes(&data_type, shard_elements)?,
            &shard_shape,
            &options,
        )?
        .into_vec();

    // Decode
    let decoded = codec.decode(encoded.clone().into(), &shard_shape, &options)?;
    assert_eq!(T::from_array_bytes(&data_type, decoded)?, shard_elements);

    // Partial decode with a generic indexer
    let indexer = vec![vec![0, 0], vec![3, 5], vec![2, 3], vec![0, 1], vec![3, 0]];
    let expected = select(
        shard_elements,
        &indexer.iter().map(|i| [i[0], i[1]]).collect::<Vec<_>>(),
    );
    let partial_decoder =
        codec
            .clone()
            .partial_decoder(Arc::new(encoded.clone()), &shard_shape, &options)?;
    let decoded = partial_decoder.partial_decode(&indexer, &options)?;
    assert_eq!(T::from_array_bytes(&data_type, decoded)?, expected);

    #[cfg(feature = "async")]
    {
        let partial_decoder = codec
            .clone()
            .async_partial_decoder(Arc::new(encoded), &shard_shape, &options)
            .await?;
        let decoded = partial_decoder.partial_decode(&indexer, &options).await?;
        assert_eq!(T::from_array_bytes(&data_type, decoded)?, expected);
    }
    Ok(())
}

async fn sharding_optional<T: ElementOwned + Clone + PartialEq + Debug>(
    data_type: DataType,
    element: impl Fn(u64) -> T,
) -> Result<(), Box<dyn std::error::Error>> {
    let elements: Vec<T> = (0..SHAPE.iter().product()).map(element).collect();
    sharding_optional_round_trip(data_type.clone(), &elements)?;
    sharding_optional_codec(data_type, &elements).await
}

#[tokio::test]
async fn sharding_optional_uint8() -> Result<(), Box<dyn std::error::Error>> {
    sharding_optional(data_type::uint8().to_optional(), |i| {
        (i % 3 != 0).then(|| u8::try_from(i).unwrap())
    })
    .await
}

#[tokio::test]
async fn sharding_optional_optional_uint16() -> Result<(), Box<dyn std::error::Error>> {
    sharding_optional(
        data_type::uint16().to_optional().to_optional(),
        |i| match i % 3 {
            0 => None,
            1 => Some(None),
            _ => Some(Some(u16::try_from(i).unwrap())),
        },
    )
    .await
}

#[tokio::test]
async fn sharding_optional_string() -> Result<(), Box<dyn std::error::Error>> {
    sharding_optional(data_type::string().to_optional(), |i| {
        (i % 4 != 0).then(|| format!("element {i}"))
    })
    .await
}
