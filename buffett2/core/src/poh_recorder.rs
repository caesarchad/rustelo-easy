use crate::tx_vault::Bank;
use crate::entry::Entry;
use buffett_crypto::hash::Hash;
use poh::Poh;
use crate::result::Result;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use crate::transaction::Transaction;

#[derive(Clone)]
pub struct PohRecorder {
    poh: Arc<Mutex<Poh>>,
    bank: Arc<Bank>,
    sender: Sender<Vec<Entry>>,
}

impl PohRecorder {
    pub fn new(bank: Arc<Bank>, sender: Sender<Vec<Entry>>) -> Self {
        let poh = Arc::new(Mutex::new(Poh::new(bank.last_id())));
        PohRecorder { poh, bank, sender }
    }

    pub fn hash(&self) {
        let mut poh = self.poh.lock().unwrap();
        poh.hash()
    }

    pub fn tick(&self) -> Result<()> {
        let mut poh = self.poh.lock().unwrap();
        let tick = poh.tick();
        self.bank.register_entry_id(&tick.id);
        let entry = Entry {
            num_hashes: tick.num_hashes,
            id: tick.id,
            transactions: vec![],
        };
        self.sender.send(vec![entry])?;
        Ok(())
    }

    pub fn record(&self, mixin: Hash, txs: Vec<Transaction>) -> Result<()> {
        let mut poh = self.poh.lock().unwrap();
        let tick = poh.record(mixin);
        self.bank.register_entry_id(&tick.id);
        let entry = Entry {
            num_hashes: tick.num_hashes,
            id: tick.id,
            transactions: txs,
        };
        self.sender.send(vec![entry])?;
        Ok(())
    }
}

