// zarrs 0.6-0.10
fn write_array(path: &Path, metadata: serde_json::Value, shape: &[u64], data: Data) -> Result<(), String> {
    let store = Arc::new(
        zarrs_version::storage::store::FilesystemStore::new(path)
            .map_err(|err| format!("create store: {err}"))?,
    );
    let metadata = serde_json::from_value::<zarrs_version::array::ArrayMetadata>(metadata)
        .map_err(|err| format!("parse metadata: {err}"))?;
    let array = zarrs_version::array::Array::new_with_metadata(store, ARRAY_PATH, metadata)
        .map_err(|err| format!("create array: {err}"))?;
    array.store_metadata().map_err(|err| format!("store metadata: {err}"))?;
    let subset = zarrs_version::array_subset::ArraySubset::new_with_shape(shape.to_vec());
    array
        .store_array_subset(&subset, fixed_bytes(data)?)
        .map_err(|err| format!("store array subset: {err}"))
}

fn read_array(path: &Path, shape: &[u64]) -> Result<Data, String> {
    let store = Arc::new(
        zarrs_version::storage::store::FilesystemStore::new(path)
            .map_err(|err| format!("create store: {err}"))?,
    );
    let metadata = serde_json::from_value::<zarrs_version::array::ArrayMetadata>(read_metadata(path)?)
        .map_err(|err| format!("parse metadata: {err}"))?;
    let array = zarrs_version::array::Array::new_with_metadata(store, ARRAY_PATH, metadata)
        .map_err(|err| format!("open array: {err}"))?;
    let subset = zarrs_version::array_subset::ArraySubset::new_with_shape(shape.to_vec());
    let bytes = array
        .retrieve_array_subset(&subset)
        .map_err(|err| format!("retrieve array subset: {err}"))?;
    Ok(fixed_data(bytes.into_vec()))
}
