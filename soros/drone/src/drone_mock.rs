use soros_sdk::hash::Hash;
use soros_sdk::pubkey::Pubkey;
use soros_sdk::signature::{Keypair, KeypairUtil};
use soros_sdk::system_transaction;
use soros_sdk::transaction::Transaction;
use std::io::{Error, ErrorKind};
use std::net::SocketAddr;

pub fn request_airdrop_transaction(
    _drone_addr: &SocketAddr,
    _id: &Pubkey,
    lamports: u64,
    _blockhash: Hash,
) -> Result<Transaction, Error> {
    if lamports == 0 {
        Err(Error::new(ErrorKind::Other, "Airdrop failed"))?
    }
    let key = Keypair::new();
    let to = Pubkey::new_rand();
    let blockhash = Hash::default();
    let tx = system_transaction::create_user_account(&key, &to, lamports, blockhash, 0);
    Ok(tx)
}
