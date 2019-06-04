use crate::tx_vault::Bank;
use bincode::deserialize;
use crate::budget_transaction::BudgetTransaction;
use crate::counter::Counter;
use crate::entry::Entry;
use log::Level;
use crate::packet::Packets;
use poh_recorder::PohRecorder;
use rayon::prelude::*;
use crate::result::{Error, Result};
use crate::service::Service;
use crate::sigverify_stage::VerifiedPackets;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;
use std::time::Instant;
use buffett_timing::timing;
use crate::transaction::Transaction;


pub const NUM_THREADS: usize = 1;


pub struct BankingStage {
    
    thread_hdls: Vec<JoinHandle<()>>,
}

pub enum Config {
    
    Tick(usize),
    
    Sleep(Duration),
}

impl Default for Config {
    fn default() -> Config {
        
        Config::Sleep(Duration::from_millis(500))
    }
}
impl BankingStage {
    
    pub fn new(
        bank: &Arc<Bank>,
        verified_receiver: Receiver<VerifiedPackets>,
        config: Config,
    ) -> (Self, Receiver<Vec<Entry>>) {
        let (entry_sender, entry_receiver) = channel();
        let shared_verified_receiver = Arc::new(Mutex::new(verified_receiver));
        let poh = PohRecorder::new(bank.clone(), entry_sender);
        let tick_poh = poh.clone();
        
        let poh_exit = Arc::new(AtomicBool::new(false));
        let banking_exit = poh_exit.clone();
        
        let tick_producer = Builder::new()
            .name("bitconch-banking-stage-tick_producer".to_string())
            .spawn(move || {
                if let Err(e) = Self::tick_producer(&tick_poh, &config, &poh_exit) {
                    match e {
                        Error::SendError => (),
                        _ => error!(
                            "bitconch-banking-stage-tick_producer unexpected error {:?}",
                            e
                        ),
                    }
                }
                debug!("tick producer exiting");
                poh_exit.store(true, Ordering::Relaxed);
            }).unwrap();

        
        let mut thread_hdls: Vec<JoinHandle<()>> = (0..NUM_THREADS)
            .into_iter()
            .map(|_| {
                let thread_bank = bank.clone();
                let thread_verified_receiver = shared_verified_receiver.clone();
                let thread_poh = poh.clone();
                let thread_banking_exit = banking_exit.clone();
                Builder::new()
                    .name("bitconch-banking-stage-tx".to_string())
                    .spawn(move || {
                        loop {
                            if let Err(e) = Self::process_packets(
                                &thread_bank,
                                &thread_verified_receiver,
                                &thread_poh,
                            ) {
                                debug!("got error {:?}", e);
                                match e {
                                    Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                                    Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => {
                                        break
                                    }
                                    Error::RecvError(_) => break,
                                    Error::SendError => break,
                                    _ => error!("bitconch-banking-stage-tx {:?}", e),
                                }
                            }
                            if thread_banking_exit.load(Ordering::Relaxed) {
                                debug!("tick service exited");
                                break;
                            }
                        }
                        thread_banking_exit.store(true, Ordering::Relaxed);
                    }).unwrap()
            }).collect();
        thread_hdls.push(tick_producer);
        (BankingStage { thread_hdls }, entry_receiver)
    }

    
    fn deserialize_transactions(p: &Packets) -> Vec<Option<(Transaction, SocketAddr)>> {
        p.packets
            .par_iter()
            .map(|x| {
                deserialize(&x.data[0..x.meta.size])
                    .map(|req| (req, x.meta.addr()))
                    .ok()
            }).collect()
    }

    fn tick_producer(poh: &PohRecorder, config: &Config, poh_exit: &AtomicBool) -> Result<()> {
        loop {
            match *config {
                Config::Tick(num) => {
                    for _ in 0..num {
                        poh.hash();
                    }
                }
                Config::Sleep(duration) => {
                    sleep(duration);
                }
            }
            poh.tick()?;
            if poh_exit.load(Ordering::Relaxed) {
                debug!("tick service exited");
                return Ok(());
            }
        }
    }

    fn process_transactions(
        bank: &Arc<Bank>,
        transactions: &[Transaction],
        poh: &PohRecorder,
    ) -> Result<()> {
        debug!("transactions: {}", transactions.len());
        let mut chunk_start = 0;
        while chunk_start != transactions.len() {
            let chunk_end = chunk_start + Entry::num_will_fit(&transactions[chunk_start..]);

            let results = bank.process_transactions(&transactions[chunk_start..chunk_end]);

            let processed_transactions: Vec<_> = transactions[chunk_start..chunk_end]
                .into_iter()
                .enumerate()
                .filter_map(|(i, x)| match results[i] {
                    Ok(_) => Some(x.clone()),
                    Err(ref e) => {
                        debug!("process transaction failed {:?}", e);
                        None
                    }
                }).collect();

            if !processed_transactions.is_empty() {
                let hash = Transaction::hash(&processed_transactions);
                debug!("processed ok: {} {}", processed_transactions.len(), hash);
                poh.record(hash, processed_transactions)?;
            }
            chunk_start = chunk_end;
        }
        debug!("done process_transactions");
        Ok(())
    }

    
    pub fn process_packets(
        bank: &Arc<Bank>,
        verified_receiver: &Arc<Mutex<Receiver<VerifiedPackets>>>,
        poh: &PohRecorder,
    ) -> Result<()> {
        let recv_start = Instant::now();
        let mms = verified_receiver
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_millis(100))?;
        let mut reqs_len = 0;
        let mms_len = mms.len();
        info!(
            "@{:?} process start stalled for: {:?}ms batches: {}",
            timing::timestamp(),
            timing::duration_in_milliseconds(&recv_start.elapsed()),
            mms.len(),
        );
        inc_new_counter_info!("banking_stage-entries_received", mms_len);
        let bank_starting_tx_count = bank.transaction_count();
        let count = mms.iter().map(|x| x.1.len()).sum();
        let proc_start = Instant::now();
        for (msgs, vers) in mms {
            let transactions = Self::deserialize_transactions(&msgs.read().unwrap());
            reqs_len += transactions.len();

            debug!("transactions received {}", transactions.len());

            let transactions: Vec<_> = transactions
                .into_iter()
                .zip(vers)
                .filter_map(|(tx, ver)| match tx {
                    None => None,
                    Some((tx, _addr)) => if tx.verify_plan() && ver != 0 {
                        Some(tx)
                    } else {
                        None
                    },
                }).collect();
            debug!("verified transactions {}", transactions.len());
            Self::process_transactions(bank, &transactions, poh)?;
        }

        inc_new_counter_info!(
            "banking_stage-time_ms",
            timing::duration_in_milliseconds(&proc_start.elapsed()) as usize
        );
        let total_time_s = timing::duration_in_seconds(&proc_start.elapsed());
        let total_time_ms = timing::duration_in_milliseconds(&proc_start.elapsed());
        info!(
            "Current Timing @{:?} done processing transaction bundle: {} time: {:?}milli-seconds requested: {} requsts per second: {}",
            timing::timestamp(),
            mms_len,
            total_time_ms,
            reqs_len,
            (reqs_len as f32) / (total_time_s)
        );
        inc_new_counter_info!("banking_stage-process_packets", count);
        inc_new_counter_info!(
            "banking_stage-process_transactions",
            bank.transaction_count() - bank_starting_tx_count
        );
        Ok(())
    }
}

impl Service for BankingStage {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }
}

