//! Array data exchanged between the current `zarrs` and helpers.

use serde::{Deserialize, Serialize};
use zarrs::array::{ArrayBytes, ArrayBytesOffsets};

/// Array data: element bytes, element offsets if variable length, and validity masks (outermost first) if optional.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Data {
    pub(crate) bytes: Vec<u8>,
    pub(crate) offsets: Option<Vec<usize>>,
    pub(crate) masks: Vec<Vec<u8>>,
}

impl Data {
    pub(crate) fn from_elements(elements: &[Vec<u8>], variable: bool, masks: Vec<Vec<u8>>) -> Self {
        let offsets = variable.then(|| {
            std::iter::once(0)
                .chain(elements.iter().scan(0, |offset, element| {
                    *offset += element.len();
                    Some(*offset)
                }))
                .collect()
        });
        Self {
            bytes: elements.concat(),
            offsets,
            masks,
        }
    }

    /// Zero the elements (and inner masks) of null elements, as their content is unspecified.
    pub(crate) fn canonical(&self, element_size: Option<usize>) -> Self {
        let Some(num_elements) = self.masks.first().map(Vec::len) else {
            return self.clone();
        };
        let mut masks = self.masks.clone();
        let elements = (0..num_elements).map(|index| {
            let element = match (&self.offsets, element_size) {
                (Some(offsets), _) => self.bytes.get(offsets[index]..offsets[index + 1]),
                (None, Some(size)) => self.bytes.get(index * size..(index + 1) * size),
                (None, None) => None,
            };
            let element = element.unwrap_or_default().to_vec();
            if let Some(level) = masks.iter().position(|mask| mask[index] == 0) {
                for mask in &mut masks[level..] {
                    mask[index] = 0;
                }
                vec![0; element_size.unwrap_or(0)]
            } else {
                element
            }
        });
        let elements: Vec<_> = elements.collect();
        Self::from_elements(&elements, self.offsets.is_some(), masks)
    }

    /// Convert to current `zarrs` array bytes.
    pub(crate) fn to_array_bytes(&self) -> Result<ArrayBytes<'static>, String> {
        let mut bytes = match &self.offsets {
            None => ArrayBytes::new_flen(self.bytes.clone()),
            Some(offsets) => {
                let offsets = offsets
                    .iter()
                    .map(|&offset| offset as u64)
                    .collect::<Vec<_>>();
                let offsets =
                    ArrayBytesOffsets::new(offsets).map_err(|err| format!("offsets: {err}"))?;
                ArrayBytes::new_vlen(self.bytes.clone(), offsets)
                    .map_err(|err| format!("offsets: {err}"))?
            }
        };
        for mask in self.masks.iter().rev() {
            bytes = bytes.with_optional_mask(mask.clone());
        }
        Ok(bytes)
    }

    /// Convert from current `zarrs` array bytes.
    pub(crate) fn from_array_bytes(bytes: ArrayBytes<'_>) -> Self {
        match bytes {
            ArrayBytes::Fixed(bytes) => Data {
                bytes: bytes.into_vec(),
                ..Data::default()
            },
            ArrayBytes::Variable(bytes) => {
                let (bytes, offsets) = bytes.into_parts();
                Data {
                    bytes: bytes.into_vec(),
                    offsets: Some(offsets.iter().collect()),
                    masks: vec![],
                }
            }
            ArrayBytes::Optional(bytes) => {
                let (data, mask) = bytes.into_parts();
                let mut data = Self::from_array_bytes(*data);
                data.masks.insert(0, mask.into_vec());
                data
            }
        }
    }
}
