use bincode::deserialize;
use bincode::serialize;
use crate::budget_program::BudgetState;
use crate::budget_transaction::BudgetTransaction;
use buffett_metrics::counter::Counter;
use crate::dynamic_program::DynamicProgram;
use crate::entry::Entry;
use buffett_crypto::hash::{hash, Hash};
use itertools::Itertools;
use crate::ledger::Block;
use log::Level;
use crate::coinery::Mint;
use buffett_budget::payment_plan::Payment;
use buffett_crypto::signature::{Keypair, Signature};
use buffett_interface::account::{Account, KeyedAccount};
use buffett_interface::pubkey::Pubkey;
use std;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::result;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::RwLock;
use std::time::Instant;
use crate::storage_program::StorageProgram;
use crate::system_program::SystemProgram;
use crate::system_transaction::SystemTransaction;
use crate::tictactoe_dashboard_program::TicTacToeDashboardProgram;
use crate::tictactoe_program::TicTacToeProgram;
use buffett_timing::timing::{duration_in_microseconds, timestamp};
use crate::transaction::Transaction;
use crate::window::WINDOW_SIZE;
use buffett_metrics::sub_new_counter_info;

pub const MAX_ENTRY_IDS: usize = 1024 * 16;

pub const VERIFY_BLOCK_SIZE: usize = 16;


#[derive(Debug, PartialEq, Eq, Clone)]
pub enum BankError {
    
    AccountNotFound,

    
    InsufficientFundsForFee,


    DuplicateSignature,


    LastIdNotFound,


    SignatureNotFound,

    
    LedgerVerificationFailed,
    
    UnbalancedTransaction,
    
    ResultWithNegativeTokens,

    UnknownContractId,

    ModifiedContractId,

    ExternalAccountTokenSpend,

    ProgramRuntimeError,
}

pub type Result<T> = result::Result<T, BankError>;
type SignatureStatusMap = HashMap<Signature, Result<()>>;

#[derive(Default)]
struct ErrorCounters {
    account_not_found_validator: usize,
    account_not_found_leader: usize,
    account_not_found_vote: usize,
}


pub struct Bank {
    
    accounts: RwLock<HashMap<Pubkey, Account>>,

    
    last_ids: RwLock<VecDeque<Hash>>,

    
    last_ids_sigs: RwLock<HashMap<Hash, (SignatureStatusMap, u64)>>,

    
    transaction_count: AtomicUsize,

    
    pub is_leader: bool,

    
    finality_time: AtomicUsize,

    
    loaded_contracts: RwLock<HashMap<Pubkey, DynamicProgram>>,
}

impl Default for Bank {
    fn default() -> Self {
        Bank {
            accounts: RwLock::new(HashMap::new()),
            last_ids: RwLock::new(VecDeque::new()),
            last_ids_sigs: RwLock::new(HashMap::new()),
            transaction_count: AtomicUsize::new(0),
            is_leader: true,
            finality_time: AtomicUsize::new(std::usize::MAX),
            loaded_contracts: RwLock::new(HashMap::new()),
        }
    }
}

impl Bank {
    
    pub fn new_default(is_leader: bool) -> Self {
        let mut bank = Bank::default();
        bank.is_leader = is_leader;
        bank
    }
    
    pub fn new_from_deposit(deposit: &Payment) -> Self {
        let bank = Self::default();
        {
            let mut accounts = bank.accounts.write().unwrap();
            let account = accounts.entry(deposit.to).or_insert_with(Account::default);
            Self::apply_payment(deposit, account);
        }
        bank
    }

    
    pub fn new(mint: &Mint) -> Self {
        let deposit = Payment {
            to: mint.pubkey(),
            tokens: mint.tokens,
        };
        let bank = Self::new_from_deposit(&deposit);
        bank.register_entry_id(&mint.last_id());
        bank
    }

    
    fn apply_payment(payment: &Payment, account: &mut Account) {
        trace!("apply payments {}", payment.tokens);
        account.tokens += payment.tokens;
    }

    
    pub fn last_id(&self) -> Hash {
        let last_ids = self.last_ids.read().expect("'last_ids' read lock");
        let last_item = last_ids
            .iter()
            .last()
            .expect("get last item from 'last_ids' list");
        *last_item
    }

    
    fn reserve_signature(signatures: &mut SignatureStatusMap, signature: &Signature) -> Result<()> {
        if let Some(_result) = signatures.get(signature) {
            return Err(BankError::DuplicateSignature);
        }
        signatures.insert(*signature, Ok(()));
        Ok(())
    }

    
    pub fn clear_signatures(&self) {
        for (_, sigs) in self.last_ids_sigs.write().unwrap().iter_mut() {
            sigs.0.clear();
        }
    }

    fn reserve_signature_with_last_id(&self, signature: &Signature, last_id: &Hash) -> Result<()> {
        if let Some(entry) = self
            .last_ids_sigs
            .write()
            .expect("'last_ids' read lock in reserve_signature_with_last_id")
            .get_mut(last_id)
        {
            return Self::reserve_signature(&mut entry.0, signature);
        }
        Err(BankError::LastIdNotFound)
    }

    fn update_signature_status(
        signatures: &mut SignatureStatusMap,
        signature: &Signature,
        result: &Result<()>,
    ) {
        let entry = signatures.entry(*signature).or_insert(Ok(()));
        *entry = result.clone();
    }

    fn update_signature_status_with_last_id(
        &self,
        signature: &Signature,
        result: &Result<()>,
        last_id: &Hash,
    ) {
        if let Some(entry) = self.last_ids_sigs.write().unwrap().get_mut(last_id) {
            Self::update_signature_status(&mut entry.0, signature, result);
        }
    }

    fn update_transaction_statuses(&self, txs: &[Transaction], res: &[Result<()>]) {
        for (i, tx) in txs.iter().enumerate() {
            self.update_signature_status_with_last_id(&tx.signature, &res[i], &tx.last_id);
        }
    }

    
    pub fn count_valid_ids(&self, ids: &[Hash]) -> Vec<(usize, u64)> {
        let last_ids = self.last_ids_sigs.read().unwrap();
        let mut ret = Vec::new();
        for (i, id) in ids.iter().enumerate() {
            if let Some(entry) = last_ids.get(id) {
                ret.push((i, entry.1));
            }
        }
        ret
    }

    
    pub fn register_entry_id(&self, last_id: &Hash) {
        let mut last_ids = self
            .last_ids
            .write()
            .expect("'last_ids' write lock in register_entry_id");
        let mut last_ids_sigs = self
            .last_ids_sigs
            .write()
            .expect("last_ids_sigs write lock");
        if last_ids.len() >= MAX_ENTRY_IDS {
            let id = last_ids.pop_front().unwrap();
            last_ids_sigs.remove(&id);
        }
        last_ids_sigs.insert(*last_id, (HashMap::new(), timestamp()));
        last_ids.push_back(*last_id);
    }

    
    pub fn process_transaction(&self, tx: &Transaction) -> Result<()> {
        match self.process_transactions(&[tx.clone()])[0] {
            Err(ref e) => {
                info!("a transaction error happened in tx_vault: {:?}", e);
                Err((*e).clone())
            }
            Ok(_) => Ok(()),
        }
    }

    fn load_account(
        &self,
        tx: &Transaction,
        accounts: &HashMap<Pubkey, Account>,
        error_counters: &mut ErrorCounters,
    ) -> Result<Vec<Account>> {
        
        if accounts.get(&tx.keys[0]).is_none() {
            if !self.is_leader {
                error_counters.account_not_found_validator += 1;
            } else {
                error_counters.account_not_found_leader += 1;
            }
            if BudgetState::check_id(&tx.program_id) {
                use buffett_budget::budget_instruction::Instruction;
                if let Some(Instruction::NewVote(_vote)) = tx.instruction() {
                    error_counters.account_not_found_vote += 1;
                }
            }
            Err(BankError::AccountNotFound)
        } else if accounts.get(&tx.keys[0]).unwrap().tokens < tx.fee {
            Err(BankError::InsufficientFundsForFee)
        } else {
            let mut called_accounts: Vec<Account> = tx
                .keys
                .iter()
                .map(|key| accounts.get(key).cloned().unwrap_or_default())
                .collect();
            
            self.reserve_signature_with_last_id(&tx.signature, &tx.last_id)?;
            called_accounts[0].tokens -= tx.fee;
            Ok(called_accounts)
        }
    }

    fn load_accounts(
        &self,
        txs: &[Transaction],
        accounts: &HashMap<Pubkey, Account>,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<Vec<Account>>> {
        txs.iter()
            .map(|tx| self.load_account(tx, accounts, error_counters))
            .collect()
    }

    pub fn verify_transaction(
        tx: &Transaction,
        pre_program_id: &Pubkey,
        pre_tokens: i64,
        account: &Account,
    ) -> Result<()> {
        
        if !((*pre_program_id == account.program_id)
            || (SystemProgram::check_id(&tx.program_id)
                && SystemProgram::check_id(&pre_program_id)))
        {
            
            return Err(BankError::ModifiedContractId);
        }
        
        if tx.program_id != account.program_id && pre_tokens > account.tokens {
            return Err(BankError::ExternalAccountTokenSpend);
        }
        if account.tokens < 0 {
            return Err(BankError::ResultWithNegativeTokens);
        }
        Ok(())
    }

    fn loaded_contract(&self, tx: &Transaction, accounts: &mut [Account]) -> bool {
        let loaded_contracts = self.loaded_contracts.write().unwrap();
        match loaded_contracts.get(&tx.program_id) {
            Some(dc) => {
                let mut infos: Vec<_> = (&tx.keys)
                    .into_iter()
                    .zip(accounts)
                    .map(|(key, account)| KeyedAccount { key, account })
                    .collect();

                dc.call(&mut infos, &tx.userdata);
                true
            }
            None => false,
        }
    }

    
    fn execute_transaction(&self, tx: &Transaction, accounts: &mut [Account]) -> Result<()> {
        let pre_total: i64 = accounts.iter().map(|a| a.tokens).sum();
        let pre_data: Vec<_> = accounts
            .iter_mut()
            .map(|a| (a.program_id, a.tokens))
            .collect();

        
        if SystemProgram::check_id(&tx.program_id) {
            SystemProgram::process_transaction(&tx, accounts, &self.loaded_contracts)
        } else if BudgetState::check_id(&tx.program_id) {
            
            if BudgetState::process_transaction(&tx, accounts).is_err() {
                return Err(BankError::ProgramRuntimeError);
            }
        } else if StorageProgram::check_id(&tx.program_id) {
            if StorageProgram::process_transaction(&tx, accounts).is_err() {
                return Err(BankError::ProgramRuntimeError);
            }
        } else if TicTacToeProgram::check_id(&tx.program_id) {
            if TicTacToeProgram::process_transaction(&tx, accounts).is_err() {
                return Err(BankError::ProgramRuntimeError);
            }
        } else if TicTacToeDashboardProgram::check_id(&tx.program_id) {
            if TicTacToeDashboardProgram::process_transaction(&tx, accounts).is_err() {
                return Err(BankError::ProgramRuntimeError);
            }
        } else if self.loaded_contract(&tx, accounts) {
        } else {
            return Err(BankError::UnknownContractId);
        }
        
        for ((pre_program_id, pre_tokens), post_account) in pre_data.iter().zip(accounts.iter()) {
            Self::verify_transaction(&tx, pre_program_id, *pre_tokens, post_account)?;
        }
        
        let post_total: i64 = accounts.iter().map(|a| a.tokens).sum();
        if pre_total != post_total {
            Err(BankError::UnbalancedTransaction)
        } else {
            Ok(())
        }
    }

    pub fn store_accounts(
        txs: &[Transaction],
        res: &[Result<()>],
        loaded: &[Result<Vec<Account>>],
        accounts: &mut HashMap<Pubkey, Account>,
    ) {
        for (i, racc) in loaded.iter().enumerate() {
            if res[i].is_err() || racc.is_err() {
                continue;
            }

            let tx = &txs[i];
            let acc = racc.as_ref().unwrap();
            for (key, account) in tx.keys.iter().zip(acc.iter()) {
                //purge if 0
                if account.tokens == 0 {
                    accounts.remove(&key);
                } else {
                    *accounts.entry(*key).or_insert_with(Account::default) = account.clone();
                    assert_eq!(accounts.get(key).unwrap().tokens, account.tokens);
                }
            }
        }
    }

    
    #[must_use]
    pub fn process_transactions(&self, txs: &[Transaction]) -> Vec<Result<()>> {
        debug!("processing transactions: {}", txs.len());
        
        let mut accounts = self.accounts.write().unwrap();
        let txs_len = txs.len();
        let mut error_counters = ErrorCounters::default();
        let now = Instant::now();
        let mut loaded_accounts = self.load_accounts(&txs, &accounts, &mut error_counters);
        let load_elapsed = now.elapsed();
        let now = Instant::now();

        let res: Vec<_> = loaded_accounts
            .iter_mut()
            .zip(txs.iter())
            .map(|(acc, tx)| match acc {
                Err(e) => Err(e.clone()),
                Ok(ref mut accounts) => self.execute_transaction(tx, accounts),
            }).collect();
        let execution_elapsed = now.elapsed();
        let now = Instant::now();
        Self::store_accounts(&txs, &res, &loaded_accounts, &mut accounts);
        self.update_transaction_statuses(&txs, &res);
        let write_elapsed = now.elapsed();
        debug!(
            "load: {}us execution: {}us write: {}us txs_len={}",
            duration_in_microseconds(&load_elapsed),
            duration_in_microseconds(&execution_elapsed),
            duration_in_microseconds(&write_elapsed),
            txs_len
        );
        let mut tx_count = 0;
        let mut err_count = 0;
        for r in &res {
            if r.is_ok() {
                tx_count += 1;
            } else {
                if err_count == 0 {
                    debug!("tx error: {:?}", r);
                }
                err_count += 1;
            }
        }
        if err_count > 0 {
            info!("{} errors of {} txs", err_count, err_count + tx_count);
            if !self.is_leader {
                sub_new_counter_info!("bank-process_transactions_err-validator", err_count);
                sub_new_counter_info!(
                    "bank-appy_debits-account_not_found-validator",
                    error_counters.account_not_found_validator
                );
            } else {
                sub_new_counter_info!("bank-process_transactions_err-leader", err_count);
                sub_new_counter_info!(
                    "bank-appy_debits-account_not_found-leader",
                    error_counters.account_not_found_leader
                );
                sub_new_counter_info!(
                    "bank-appy_debits-vote_account_not_found",
                    error_counters.account_not_found_vote
                );
            }
        }
        let cur_tx_count = self.transaction_count.load(Ordering::Relaxed);
        if ((cur_tx_count + tx_count) & !(262_144 - 1)) > cur_tx_count & !(262_144 - 1) {
            info!("accounts.len: {}", accounts.len());
        }
        self.transaction_count
            .fetch_add(tx_count, Ordering::Relaxed);
        res
    }

    pub fn process_entry(&self, entry: &Entry) -> Result<()> {
        if !entry.transactions.is_empty() {
            for result in self.process_transactions(&entry.transactions) {
                result?;
            }
        }
        self.register_entry_id(&entry.id);
        Ok(())
    }

    
    fn process_entries_tail(
        &self,
        entries: Vec<Entry>,
        tail: &mut Vec<Entry>,
        tail_idx: &mut usize,
    ) -> Result<u64> {
        let mut entry_count = 0;

        for entry in entries {
            if tail.len() > *tail_idx {
                tail[*tail_idx] = entry.clone();
            } else {
                tail.push(entry.clone());
            }
            *tail_idx = (*tail_idx + 1) % WINDOW_SIZE as usize;

            entry_count += 1;
            self.process_entry(&entry)?;
        }

        Ok(entry_count)
    }

    
    pub fn process_entries(&self, entries: &[Entry]) -> Result<()> {
        for entry in entries {
            self.process_entry(&entry)?;
        }
        Ok(())
    }

    
    fn process_blocks<I>(
        &self,
        start_hash: Hash,
        entries: I,
        tail: &mut Vec<Entry>,
        tail_idx: &mut usize,
    ) -> Result<u64>
    where
        I: IntoIterator<Item = Entry>,
    {
        
        let mut entry_count = *tail_idx as u64;
        let mut id = start_hash;
        for block in &entries.into_iter().chunks(VERIFY_BLOCK_SIZE) {
            let block: Vec<_> = block.collect();
            if !block.verify(&id) {
                warn!("Ledger proof of history failed at entry: {}", entry_count);
                return Err(BankError::LedgerVerificationFailed);
            }
            id = block.last().unwrap().id;
            entry_count += self.process_entries_tail(block, tail, tail_idx)?;
        }
        Ok(entry_count)
    }

    
    pub fn process_ledger<I>(&self, entries: I) -> Result<(u64, Vec<Entry>)>
    where
        I: IntoIterator<Item = Entry>,
    {
        let mut entries = entries.into_iter();

        
        let entry0 = entries.next().expect("invalid ledger: empty");

        
        let entry1 = entries
            .next()
            .expect("invalid ledger: need at least 2 entries");
        {
            let tx = &entry1.transactions[0];
            assert!(SystemProgram::check_id(&tx.program_id), "Invalid ledger");
            let instruction: SystemProgram = deserialize(&tx.userdata).unwrap();
            let deposit = if let SystemProgram::Move { tokens } = instruction {
                Some(tokens)
            } else {
                None
            }.expect("invalid ledger, needs to start with a contract");
            {
                let mut accounts = self.accounts.write().unwrap();
                let account = accounts.entry(tx.keys[0]).or_insert_with(Account::default);
                account.tokens += deposit;
                trace!("applied genesis payment {:?} => {:?}", deposit, account);
            }
        }
        self.register_entry_id(&entry0.id);
        self.register_entry_id(&entry1.id);
        let entry1_id = entry1.id;

        let mut tail = Vec::with_capacity(WINDOW_SIZE as usize);
        tail.push(entry0);
        tail.push(entry1);
        let mut tail_idx = 2;
        let entry_count = self.process_blocks(entry1_id, entries, &mut tail, &mut tail_idx)?;

        
        if tail.len() == WINDOW_SIZE as usize {
            tail.rotate_left(tail_idx)
        }

        Ok((entry_count, tail))
    }

    
    pub fn transfer(
        &self,
        n: i64,
        keypair: &Keypair,
        to: Pubkey,
        last_id: Hash,
    ) -> Result<Signature> {
        let tx = Transaction::system_new(keypair, to, n, last_id);
        let signature = tx.signature;
        self.process_transaction(&tx).map(|_| signature)
    }

    pub fn read_balance(account: &Account) -> i64 {
        if SystemProgram::check_id(&account.program_id) {
            SystemProgram::get_balance(account)
        } else if BudgetState::check_id(&account.program_id) {
            BudgetState::get_balance(account)
        } else {
            account.tokens
        }
    }
    
    pub fn get_balance(&self, pubkey: &Pubkey) -> i64 {
        self.get_account(pubkey)
            .map(|x| Self::read_balance(&x))
            .unwrap_or(0)
    }

    pub fn get_account(&self, pubkey: &Pubkey) -> Option<Account> {
        let accounts = self
            .accounts
            .read()
            .expect("'accounts' read lock in get_balance");
        accounts.get(pubkey).cloned()
    }

    pub fn transaction_count(&self) -> usize {
        self.transaction_count.load(Ordering::Relaxed)
    }

    pub fn get_signature_status(&self, signature: &Signature) -> Result<()> {
        let last_ids_sigs = self.last_ids_sigs.read().unwrap();
        for (_hash, (signatures, _)) in last_ids_sigs.iter() {
            if let Some(res) = signatures.get(signature) {
                return res.clone();
            }
        }
        Err(BankError::SignatureNotFound)
    }

    pub fn has_signature(&self, signature: &Signature) -> bool {
        self.get_signature_status(signature) != Err(BankError::SignatureNotFound)
    }

    
    pub fn hash_internal_state(&self) -> Hash {
        let mut ordered_accounts = BTreeMap::new();
        for (pubkey, account) in self.accounts.read().unwrap().iter() {
            ordered_accounts.insert(*pubkey, account.clone());
        }
        hash(&serialize(&ordered_accounts).unwrap())
    }

    pub fn finality(&self) -> usize {
        self.finality_time.load(Ordering::Relaxed)
    }

    pub fn set_finality(&self, finality: usize) {
        self.finality_time.store(finality, Ordering::Relaxed);
    }
}

