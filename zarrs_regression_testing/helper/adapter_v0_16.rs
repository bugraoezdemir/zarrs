// zarrs 0.16
use zarrs_version::array::ArrayBytes;
use zarrs_version::storage::store::FilesystemStore;

fn open_store(path: &Path) -> Result<Arc<FilesystemStore>, String> {
    Ok(Arc::new(
        FilesystemStore::new(path).map_err(|err| format!("create store: {err}"))?,
    ))
}

fn open_array(path: &Path) -> Result<zarrs_version::array::Array<FilesystemStore>, String> {
    let metadata = serde_json::from_value::<zarrs_version::array::ArrayMetadata>(read_metadata(path)?)
        .map_err(|err| format!("parse metadata: {err}"))?;
    zarrs_version::array::Array::new_with_metadata(open_store(path)?, ARRAY_PATH, metadata)
        .map_err(|err| format!("open array: {err}"))
}

fn to_array_bytes(data: Data) -> Result<ArrayBytes<'static>, String> {
    if !data.masks.is_empty() {
        return Err("optional data is not supported by this zarrs version".to_string());
    }
    Ok(match data.offsets {
        None => ArrayBytes::new_flen(data.bytes),
        Some(offsets) => ArrayBytes::new_vlen(data.bytes, offsets),
    })
}

#[allow(unreachable_patterns)]
fn from_array_bytes(bytes: ArrayBytes<'_>) -> Result<Data, String> {
    match bytes {
        ArrayBytes::Fixed(bytes) => Ok(fixed_data(bytes.into_owned())),
        ArrayBytes::Variable(bytes, offsets) => Ok(Data {
            bytes: bytes.into_owned(),
            offsets: Some(offsets.to_vec()),
            masks: vec![],
        }),
        _ => Err("unsupported array bytes".to_string()),
    }
}

fn write_array(path: &Path, metadata: serde_json::Value, shape: &[u64], data: Data) -> Result<(), String> {
    let metadata = serde_json::from_value::<zarrs_version::array::ArrayMetadata>(metadata)
        .map_err(|err| format!("parse metadata: {err}"))?;
    let array = zarrs_version::array::Array::new_with_metadata(open_store(path)?, ARRAY_PATH, metadata)
        .map_err(|err| format!("create array: {err}"))?;
    array.store_metadata().map_err(|err| format!("store metadata: {err}"))?;
    let subset = zarrs_version::array_subset::ArraySubset::new_with_shape(shape.to_vec());
    array
        .store_array_subset(&subset, to_array_bytes(data)?)
        .map_err(|err| format!("store array subset: {err}"))
}

fn read_array(path: &Path, shape: &[u64]) -> Result<Data, String> {
    let array = open_array(path)?;
    let subset = zarrs_version::array_subset::ArraySubset::new_with_shape(shape.to_vec());
    let bytes = array
        .retrieve_array_subset(&subset)
        .map_err(|err| format!("retrieve array subset: {err}"))?;
    from_array_bytes(bytes)
}
