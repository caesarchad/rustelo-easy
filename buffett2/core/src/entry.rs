use bincode::{serialize_into, serialized_size};
use crate::budget_transaction::BudgetTransaction;
use buffett_crypto::hash::Hash;
use crate::packet::{SharedBlob, BLOB_DATA_SIZE};
use crate::poh::Poh;
use rayon::prelude::*;
use buffett_interface::pubkey::Pubkey;
use std::io::Cursor;
use std::net::SocketAddr;
use std::sync::mpsc::{Receiver, Sender};
use crate::transaction::Transaction;

pub type EntrySender = Sender<Vec<Entry>>;
pub type EntryReceiver = Receiver<Vec<Entry>>;



#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Entry {
    
    pub num_hashes: u64,

    
    pub id: Hash,

    
    pub transactions: Vec<Transaction>,
}

impl Entry {
    
    pub fn new(start_hash: &Hash, num_hashes: u64, transactions: Vec<Transaction>) -> Self {
        let num_hashes = num_hashes + if transactions.is_empty() { 0 } else { 1 };
        let id = next_hash(start_hash, 0, &transactions);
        let entry = Entry {
            num_hashes,
            id,
            transactions,
        };

        let size = serialized_size(&entry).unwrap();
        if size > BLOB_DATA_SIZE as u64 {
            panic!(
                "Serialized entry size too large: {} ({} transactions):",
                size,
                entry.transactions.len()
            );
        }
        entry
    }

    pub fn to_blob(
        &self,
        idx: Option<u64>,
        id: Option<Pubkey>,
        addr: Option<&SocketAddr>,
    ) -> SharedBlob {
        let blob = SharedBlob::default();
        {
            let mut blob_w = blob.write().unwrap();
            let pos = {
                let mut out = Cursor::new(blob_w.data_mut());
                serialize_into(&mut out, &self).expect("failed to serialize output");
                out.position() as usize
            };
            blob_w.set_size(pos);

            if let Some(idx) = idx {
                blob_w.set_index(idx).expect("set_index()");
            }
            if let Some(id) = id {
                blob_w.set_id(id).expect("set_id()");
            }
            if let Some(addr) = addr {
                blob_w.meta.set_addr(addr);
            }
            blob_w.set_flags(0).unwrap();
        }
        blob
    }

    pub fn will_fit(transactions: Vec<Transaction>) -> bool {
        serialized_size(&Entry {
            num_hashes: 0,
            id: Hash::default(),
            transactions,
        }).unwrap()
            <= BLOB_DATA_SIZE as u64
    }

    pub fn num_will_fit(transactions: &[Transaction]) -> usize {
        if transactions.is_empty() {
            return 0;
        }
        let mut num = transactions.len();
        let mut upper = transactions.len();
        let mut lower = 1; 
        let mut next = transactions.len(); 
        loop {
            debug!(
                "num {}, upper {} lower {} next {} transactions.len() {}",
                num,
                upper,
                lower,
                next,
                transactions.len()
            );
            if Entry::will_fit(transactions[..num].to_vec()) {
                next = (upper + num) / 2;
                lower = num;
                debug!("num {} fits, maybe too well? trying {}", num, next);
            } else {
                next = (lower + num) / 2;
                upper = num;
                debug!("num {} doesn't fit! trying {}", num, next);
            }
            
            if next == num {
                debug!("converged on num {}", num);
                break;
            }
            num = next;
        }
        num
    }

    
    pub fn new_mut(
        start_hash: &mut Hash,
        num_hashes: &mut u64,
        transactions: Vec<Transaction>,
    ) -> Self {
        let entry = Self::new(start_hash, *num_hashes, transactions);
        *start_hash = entry.id;
        *num_hashes = 0;
        assert!(serialized_size(&entry).unwrap() <= BLOB_DATA_SIZE as u64);
        entry
    }

    
    pub fn new_tick(num_hashes: u64, id: &Hash) -> Self {
        Entry {
            num_hashes,
            id: *id,
            transactions: vec![],
        }
    }

    
    pub fn verify(&self, start_hash: &Hash) -> bool {
        let tx_plans_verified = self.transactions.par_iter().all(|tx| {
            let r = tx.verify_plan();
            if !r {
                warn!("tx plan invalid: {:?}", tx);
            }
            r
        });
        if !tx_plans_verified {
            return false;
        }
        let ref_hash = next_hash(start_hash, self.num_hashes, &self.transactions);
        if self.id != ref_hash {
            warn!(
                "next_hash is invalid expected: {:?} actual: {:?}",
                self.id, ref_hash
            );
            return false;
        }
        true
    }
}


fn next_hash(start_hash: &Hash, num_hashes: u64, transactions: &[Transaction]) -> Hash {
    if num_hashes == 0 && transactions.is_empty() {
        return *start_hash;
    }

    let mut poh = Poh::new(*start_hash);

    for _ in 1..num_hashes {
        poh.hash();
    }

    if transactions.is_empty() {
        poh.tick().id
    } else {
        poh.record(Transaction::hash(transactions)).id
    }
}


pub fn next_entry(start_hash: &Hash, num_hashes: u64, transactions: Vec<Transaction>) -> Entry {
    assert!(num_hashes > 0 || transactions.is_empty());
    Entry {
        num_hashes,
        id: next_hash(start_hash, num_hashes, &transactions),
        transactions,
    }
}

