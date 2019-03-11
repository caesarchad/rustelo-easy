use bitconch_sdk::hash::Hash;
use bitconch_sdk::signature::{Keypair, KeypairUtil};
use bitconch_sdk::system_transaction::SystemTransaction;
use bitconch_sdk::transaction::Transaction;

pub fn test_tx() -> Transaction {
    let keypair1 = Keypair::new();
    let pubkey1 = keypair1.pubkey();
    let zero = Hash::default();
    SystemTransaction::new_account(&keypair1, pubkey1, 42, zero, 0)
}
