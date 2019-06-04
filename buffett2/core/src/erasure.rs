use crate::packet::{SharedBlob, BLOB_DATA_SIZE, BLOB_HEADER_SIZE};
use buffett_interface::pubkey::Pubkey;
use std::cmp;
use std::mem;
use std::result;
use crate::window::WindowSlot;

pub const NUM_DATA: usize = 16; 
pub const NUM_CODING: usize = 4; 
pub const ERASURE_SET_SIZE: usize = NUM_DATA + NUM_CODING; 

pub const JERASURE_ALIGN: usize = 4; 

macro_rules! align {
    ($x:expr, $align:expr) => {
        $x + ($align - 1) & !($align - 1)
    };
}

#[derive(Debug, PartialEq, Eq)]
pub enum ErasureError {
    NotEnoughBlocksToDecode,
    DecodeError,
    EncodeError,
    InvalidBlockSize,
}

pub type Result<T> = result::Result<T, ErasureError>;



extern "C" {
    fn jerasure_matrix_encode(
        k: i32,
        m: i32,
        w: i32,
        matrix: *const i32,
        data_ptrs: *const *const u8,
        coding_ptrs: *const *mut u8,
        size: i32,
    );
    fn jerasure_matrix_decode(
        k: i32,
        m: i32,
        w: i32,
        matrix: *const i32,
        row_k_ones: i32,
        erasures: *const i32,
        data_ptrs: *const *mut u8,
        coding_ptrs: *const *mut u8,
        size: i32,
    ) -> i32;
    fn galois_single_divide(a: i32, b: i32, w: i32) -> i32;
}

fn get_matrix(m: i32, k: i32, w: i32) -> Vec<i32> {
    let mut matrix = vec![0; (m * k) as usize];
    for i in 0..m {
        for j in 0..k {
            unsafe {
                matrix[(i * k + j) as usize] = galois_single_divide(1, i ^ (m + j), w);
            }
        }
    }
    matrix
}

pub const ERASURE_W: i32 = 32;


pub fn generate_coding_blocks(coding: &mut [&mut [u8]], data: &[&[u8]]) -> Result<()> {
    if data.is_empty() {
        return Ok(());
    }
    let k = data.len() as i32;
    let m = coding.len() as i32;
    let block_len = data[0].len() as i32;
    let matrix: Vec<i32> = get_matrix(m, k, ERASURE_W);
    let mut data_arg = Vec::with_capacity(data.len());
    for block in data {
        if block_len != block.len() as i32 {
            error!(
                "data block size incorrect {} expected {}",
                block.len(),
                block_len
            );
            return Err(ErasureError::InvalidBlockSize);
        }
        data_arg.push(block.as_ptr());
    }
    let mut coding_arg = Vec::with_capacity(coding.len());
    for mut block in coding {
        if block_len != block.len() as i32 {
            error!(
                "coding block size incorrect {} expected {}",
                block.len(),
                block_len
            );
            return Err(ErasureError::InvalidBlockSize);
        }
        coding_arg.push(block.as_mut_ptr());
    }

    unsafe {
        jerasure_matrix_encode(
            k,
            m,
            ERASURE_W,
            matrix.as_ptr(),
            data_arg.as_ptr(),
            coding_arg.as_ptr(),
            block_len,
        );
    }
    Ok(())
}


pub fn decode_blocks(
    data: &mut [&mut [u8]],
    coding: &mut [&mut [u8]],
    erasures: &[i32],
) -> Result<()> {
    if data.is_empty() {
        return Ok(());
    }
    let block_len = data[0].len();
    let matrix: Vec<i32> = get_matrix(coding.len() as i32, data.len() as i32, ERASURE_W);

    
    let mut coding_arg: Vec<*mut u8> = Vec::new();
    for x in coding.iter_mut() {
        if x.len() != block_len {
            return Err(ErasureError::InvalidBlockSize);
        }
        coding_arg.push(x.as_mut_ptr());
    }

    
    let mut data_arg: Vec<*mut u8> = Vec::new();
    for x in data.iter_mut() {
        if x.len() != block_len {
            return Err(ErasureError::InvalidBlockSize);
        }
        data_arg.push(x.as_mut_ptr());
    }
    let ret = unsafe {
        jerasure_matrix_decode(
            data.len() as i32,
            coding.len() as i32,
            ERASURE_W,
            matrix.as_ptr(),
            0,
            erasures.as_ptr(),
            data_arg.as_ptr(),
            coding_arg.as_ptr(),
            data[0].len() as i32,
        )
    };
    trace!("jerasure_matrix_decode ret: {}", ret);
    for x in data[erasures[0] as usize][0..8].iter() {
        trace!("{} ", x)
    }
    trace!("");
    if ret < 0 {
        return Err(ErasureError::DecodeError);
    }
    Ok(())
}


pub fn generate_coding(
    id: &Pubkey,
    window: &mut [WindowSlot],
    receive_index: u64,
    num_blobs: usize,
    transmit_index_coding: &mut u64,
) -> Result<()> {
    
    let coding_index_start =
        receive_index - (receive_index % NUM_DATA as u64) + (NUM_DATA - NUM_CODING) as u64;

    let start_idx = receive_index as usize % window.len();
    let mut block_start = start_idx - (start_idx % NUM_DATA);

    loop {
        let block_end = block_start + NUM_DATA;
        if block_end > (start_idx + num_blobs) {
            break;
        }
        info!(
            "generate_coding {} start: {} end: {} start_idx: {} num_blobs: {}",
            id, block_start, block_end, start_idx, num_blobs
        );

        let mut max_data_size = 0;

        
        for i in block_start..block_end {
            let n = i % window.len();
            trace!("{} window[{}] = {:?}", id, n, window[n].data);

            if let Some(b) = &window[n].data {
                max_data_size = cmp::max(b.read().unwrap().meta.size, max_data_size);
            } else {
                trace!("{} data block is null @ {}", id, n);
                return Ok(());
            }
        }

       
        max_data_size = align!(max_data_size, JERASURE_ALIGN);

        trace!("{} max_data_size: {}", id, max_data_size);

        let mut data_blobs = Vec::with_capacity(NUM_DATA);
        for i in block_start..block_end {
            let n = i % window.len();

            if let Some(b) = &window[n].data {
                
                let mut b_wl = b.write().unwrap();
                for i in b_wl.meta.size..max_data_size {
                    b_wl.data[i] = 0;
                }
                data_blobs.push(b);
            }
        }

        
        *transmit_index_coding = cmp::min(*transmit_index_coding, coding_index_start);

        let mut coding_blobs = Vec::with_capacity(NUM_CODING);
        let coding_start = block_end - NUM_CODING;
        for i in coding_start..block_end {
            let n = i % window.len();
            assert!(window[n].coding.is_none());

            window[n].coding = Some(SharedBlob::default());

            let coding = window[n].coding.clone().unwrap();
            let mut coding_wl = coding.write().unwrap();
            for i in 0..max_data_size {
                coding_wl.data[i] = 0;
            }
            
            if let Some(data) = &window[n].data {
                let data_rl = data.read().unwrap();

                let index = data_rl.get_index().unwrap();
                let id = data_rl.get_id().unwrap();

                trace!(
                    "{} copying index {} id {:?} from data to coding",
                    id,
                    index,
                    id
                );
                coding_wl.set_index(index).unwrap();
                coding_wl.set_id(id).unwrap();
            }
            coding_wl.set_size(max_data_size);
            if coding_wl.set_coding().is_err() {
                return Err(ErasureError::EncodeError);
            }

            coding_blobs.push(coding.clone());
        }

        let data_locks: Vec<_> = data_blobs.iter().map(|b| b.read().unwrap()).collect();

        let data_ptrs: Vec<_> = data_locks
            .iter()
            .enumerate()
            .map(|(i, l)| {
                trace!("{} i: {} data: {}", id, i, l.data[0]);
                &l.data[..max_data_size]
            }).collect();

        let mut coding_locks: Vec<_> = coding_blobs.iter().map(|b| b.write().unwrap()).collect();

        let mut coding_ptrs: Vec<_> = coding_locks
            .iter_mut()
            .enumerate()
            .map(|(i, l)| {
                trace!("{} i: {} coding: {}", id, i, l.data[0],);
                &mut l.data_mut()[..max_data_size]
            }).collect();

        generate_coding_blocks(coding_ptrs.as_mut_slice(), &data_ptrs)?;
        debug!(
            "{} start_idx: {} data: {}:{} coding: {}:{}",
            id, start_idx, block_start, block_end, coding_start, block_end
        );
        block_start = block_end;
    }
    Ok(())
}


fn is_missing(id: &Pubkey, idx: u64, window_slot: &mut Option<SharedBlob>, c_or_d: &str) -> bool {
    if let Some(blob) = window_slot.take() {
        let blob_idx = blob.read().unwrap().get_index().unwrap();
        if blob_idx == idx {
            trace!("recover {}: idx: {} good {}", id, idx, c_or_d);
            
            mem::replace(window_slot, Some(blob));
            false
        } else {
            trace!(
                "recover {}: idx: {} old {} {}, recycling",
                id,
                idx,
                c_or_d,
                blob_idx,
            );
            true
        }
    } else {
        trace!("recover {}: idx: {} None {}", id, idx, c_or_d);
        
        true
    }
}


fn find_missing(
    id: &Pubkey,
    block_start_idx: u64,
    block_start: usize,
    window: &mut [WindowSlot],
) -> (usize, usize) {
    let mut data_missing = 0;
    let mut coding_missing = 0;
    let block_end = block_start + NUM_DATA;
    let coding_start = block_start + NUM_DATA - NUM_CODING;

    
    for i in block_start..block_end {
        let idx = (i - block_start) as u64 + block_start_idx;
        let n = i % window.len();

        if is_missing(id, idx, &mut window[n].data, "data") {
            data_missing += 1;
        }

        if i >= coding_start && is_missing(id, idx, &mut window[n].coding, "coding") {
            coding_missing += 1;
        }
    }
    (data_missing, coding_missing)
}


pub fn recover(id: &Pubkey, window: &mut [WindowSlot], start_idx: u64, start: usize) -> Result<()> {
    let block_start = start - (start % NUM_DATA);
    let block_start_idx = start_idx - (start_idx % NUM_DATA as u64);

    debug!("start: {} block_start: {}", start, block_start);

    let coding_start = block_start + NUM_DATA - NUM_CODING;
    let block_end = block_start + NUM_DATA;
    trace!(
        "recover {}: block_start_idx: {} block_start: {} coding_start: {} block_end: {}",
        id,
        block_start_idx,
        block_start,
        coding_start,
        block_end
    );

    let (data_missing, coding_missing) = find_missing(id, block_start_idx, block_start, window);

    
    if data_missing == 0 {
        
        return Ok(());
    }

    if (data_missing + coding_missing) > NUM_CODING {
        trace!(
            "recover {}: start: {} skipping recovery data: {} coding: {}",
            id,
            block_start,
            data_missing,
            coding_missing
        );
        
        return Err(ErasureError::NotEnoughBlocksToDecode);
    }

    trace!(
        "recover {}: recovering: data: {} coding: {}",
        id,
        data_missing,
        coding_missing
    );
    let mut blobs: Vec<SharedBlob> = Vec::with_capacity(NUM_DATA + NUM_CODING);
    let mut locks = Vec::with_capacity(NUM_DATA + NUM_CODING);
    let mut erasures: Vec<i32> = Vec::with_capacity(NUM_CODING);
    let mut meta = None;
    let mut size = None;

    
    for i in block_start..block_end {
        let j = i % window.len();

        if let Some(b) = window[j].data.clone() {
            if meta.is_none() {
                meta = Some(b.read().unwrap().meta.clone());
                trace!("recover {} meta at {} {:?}", id, j, meta);
            }
            blobs.push(b);
        } else {
            let n = SharedBlob::default();
            window[j].data = Some(n.clone());
            
            blobs.push(n);
            erasures.push((i - block_start) as i32);
        }
    }
    for i in coding_start..block_end {
        let j = i % window.len();
        if let Some(b) = window[j].coding.clone() {
            if size.is_none() {
                size = Some(b.read().unwrap().meta.size - BLOB_HEADER_SIZE);
                trace!(
                    "{} recover size {} from {}",
                    id,
                    size.unwrap(),
                    i as u64 + block_start_idx
                );
            }
            blobs.push(b);
        } else {
            let n = SharedBlob::default();
            window[j].coding = Some(n.clone());
            
            blobs.push(n);
            erasures.push(((i - coding_start) + NUM_DATA) as i32);
        }
    }

    
    let size = size.unwrap();
    for i in block_start..block_end {
        let j = i % window.len();

        if let Some(b) = &window[j].data {
            let mut b_wl = b.write().unwrap();
            for i in b_wl.meta.size..size {
                b_wl.data[i] = 0;
            }
        }
    }

    
    erasures.push(-1);
    trace!("erasures[]: {} {:?} data_size: {}", id, erasures, size,);
    
    for b in &blobs {
        locks.push(b.write().unwrap());
    }

    {
        let mut coding_ptrs: Vec<&mut [u8]> = Vec::with_capacity(NUM_CODING);
        let mut data_ptrs: Vec<&mut [u8]> = Vec::with_capacity(NUM_DATA);
        for (i, l) in locks.iter_mut().enumerate() {
            if i < NUM_DATA {
                trace!("{} pushing data: {}", id, i);
                data_ptrs.push(&mut l.data[..size]);
            } else {
                trace!("{} pushing coding: {}", id, i);
                coding_ptrs.push(&mut l.data_mut()[..size]);
            }
        }
        trace!(
            "{} coding_ptrs.len: {} data_ptrs.len {}",
            id,
            coding_ptrs.len(),
            data_ptrs.len()
        );
        decode_blocks(
            data_ptrs.as_mut_slice(),
            coding_ptrs.as_mut_slice(),
            &erasures,
        )?;
    }

    let mut corrupt = false;
    
    for i in &erasures[..erasures.len() - 1] {
        let n = *i as usize;
        let mut idx = n as u64 + block_start_idx;

        let mut data_size;
        if n < NUM_DATA {
            data_size = locks[n].get_data_size().unwrap() as usize;
            data_size -= BLOB_HEADER_SIZE;
            if data_size > BLOB_DATA_SIZE {
                error!("{} corrupt data blob[{}] data_size: {}", id, idx, data_size);
                corrupt = true;
            }
        } else {
            data_size = size;
            idx -= NUM_CODING as u64;
            locks[n].set_index(idx).unwrap();

            if data_size - BLOB_HEADER_SIZE > BLOB_DATA_SIZE {
                error!(
                    "{} corrupt coding blob[{}] data_size: {}",
                    id, idx, data_size
                );
                corrupt = true;
            }
        }

        locks[n].meta = meta.clone().unwrap();
        locks[n].set_size(data_size);
        trace!(
            "{} erasures[{}] ({}) size: {} data[0]: {}",
            id,
            *i,
            idx,
            data_size,
            locks[n].data()[0]
        );
    }
    assert!(!corrupt, " {} ", id);

    Ok(())
}

