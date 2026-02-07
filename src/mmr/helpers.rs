use crate::error::MmrError;

pub fn find_peaks(elements_count: u64) -> Vec<u64> {
    let mut remaining = elements_count as u128;
    let mut shift = 0u128;
    let mut peaks = Vec::new();

    let mut mountain_elements_count = if elements_count == 0 {
        0u128
    } else {
        (1u128 << bit_length(elements_count)) - 1
    };

    while mountain_elements_count > 0 {
        if mountain_elements_count <= remaining {
            shift += mountain_elements_count;
            peaks.push(shift as u64);
            remaining -= mountain_elements_count;
        }
        mountain_elements_count >>= 1;
    }

    if remaining > 0 { Vec::new() } else { peaks }
}

pub fn map_leaf_index_to_element_index(leaf_index: u64) -> u64 {
    2 * leaf_index + 1 - u64::from(leaf_index.count_ones())
}

pub fn leaf_count_to_peaks_count(leaf_count: u64) -> u32 {
    leaf_count.count_ones()
}

pub fn leaf_count_to_mmr_size(leaf_count: u64) -> u64 {
    2 * leaf_count - u64::from(leaf_count_to_peaks_count(leaf_count))
}

pub fn leaf_count_to_append_no_merges(leaf_count: u64) -> u64 {
    u64::from(leaf_count.trailing_ones())
}

pub fn find_siblings(element_index: u64, elements_count: u64) -> Result<Vec<u64>, MmrError> {
    let mut leaf_index = element_index_to_leaf_index(element_index)?;
    let mut height = 0u32;
    let mut siblings = Vec::new();
    let mut current_index = element_index;

    while current_index <= elements_count {
        let siblings_offset_u128 = (2u128 << height) - 1;
        let siblings_offset =
            u64::try_from(siblings_offset_u128).map_err(|_| MmrError::Overflow)?;

        if leaf_index % 2 == 1 {
            if current_index < siblings_offset {
                return Err(MmrError::Overflow);
            }
            siblings.push(current_index - siblings_offset);
            current_index = current_index.checked_add(1).ok_or(MmrError::Overflow)?;
        } else {
            siblings.push(
                current_index
                    .checked_add(siblings_offset)
                    .ok_or(MmrError::Overflow)?,
            );
            current_index = current_index
                .checked_add(siblings_offset)
                .and_then(|v| v.checked_add(1))
                .ok_or(MmrError::Overflow)?;
        }

        leaf_index /= 2;
        height += 1;
    }

    siblings.pop();
    Ok(siblings)
}

pub fn element_index_to_leaf_index(element_index: u64) -> Result<u64, MmrError> {
    if element_index == 0 {
        return Err(MmrError::InvalidElementIndex);
    }
    elements_count_to_leaf_count(element_index - 1)
}

pub fn elements_count_to_leaf_count(elements_count: u64) -> Result<u64, MmrError> {
    let mut leaf_count = 0u128;
    let mut current = elements_count as u128;

    let mut mountain_leaf_count = if elements_count == 0 {
        1u128
    } else {
        1u128 << bit_length(elements_count)
    };

    while mountain_leaf_count > 0 {
        let mountain_elements_count = 2 * mountain_leaf_count - 1;
        if mountain_elements_count <= current {
            leaf_count += mountain_leaf_count;
            current -= mountain_elements_count;
        }
        mountain_leaf_count >>= 1;
    }

    if current > 0 {
        Err(MmrError::InvalidElementCount)
    } else {
        u64::try_from(leaf_count).map_err(|_| MmrError::Overflow)
    }
}

pub fn get_peak_info(mut elements_count: u64, mut element_index: u64) -> (usize, usize) {
    let mut mountain_height = bit_length(elements_count);
    let mut mountain_elements_count = if mountain_height == 0 {
        0u128
    } else {
        (1u128 << mountain_height) - 1
    };
    let mut mountain_index = 0usize;

    loop {
        if mountain_elements_count <= elements_count as u128 {
            if element_index as u128 <= mountain_elements_count {
                return (mountain_index, mountain_height.saturating_sub(1) as usize);
            }
            elements_count -= mountain_elements_count as u64;
            element_index -= mountain_elements_count as u64;
            mountain_index += 1;
        }

        if mountain_height == 0 {
            return (mountain_index, 0);
        }

        mountain_elements_count >>= 1;
        mountain_height -= 1;
    }
}

pub fn mmr_size_to_leaf_count(mmr_size: u64) -> u64 {
    let mut remaining = mmr_size as u128;
    let bits = bit_length_u128(remaining + 1);
    let mut mountain_tips = 1u128 << bits.saturating_sub(1);
    let mut leaf_count = 0u128;

    while mountain_tips != 0 {
        let mountain_size = 2 * mountain_tips - 1;
        if mountain_size <= remaining {
            remaining -= mountain_size;
            leaf_count += mountain_tips;
        }
        mountain_tips >>= 1;
    }

    leaf_count as u64
}

fn bit_length(num: u64) -> u32 {
    64 - num.leading_zeros()
}

fn bit_length_u128(num: u128) -> u32 {
    128 - num.leading_zeros()
}
