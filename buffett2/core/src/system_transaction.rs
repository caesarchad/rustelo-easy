

use bincode::serialize;
use buffett_crypto::hash::Hash;
use buffett_crypto::signature::{Keypair, KeypairUtil};
use buffett_interface::pubkey::Pubkey;
use crate::system_program::SystemProgram;
use crate::transaction::Transaction;

pub trait SystemTransaction {
    fn system_create(
        from_keypair: &Keypair,
        to: Pubkey,
        last_id: Hash,
        tokens: i64,
        space: u64,
        program_id: Pubkey,
        fee: i64,
    ) -> Self;

    fn system_assign(from_keypair: &Keypair, last_id: Hash, program_id: Pubkey, fee: i64) -> Self;

    fn system_new(from_keypair: &Keypair, to: Pubkey, tokens: i64, last_id: Hash) -> Self;

    fn system_move(
        from_keypair: &Keypair,
        to: Pubkey,
        tokens: i64,
        last_id: Hash,
        fee: i64,
    ) -> Self;

    fn system_load(
        from_keypair: &Keypair,
        last_id: Hash,
        fee: i64,
        program_id: Pubkey,
        name: String,
    ) -> Self;
}

impl SystemTransaction for Transaction {
    
    fn system_create(
        from_keypair: &Keypair,
        to: Pubkey,
        last_id: Hash,
        tokens: i64,
        space: u64,
        program_id: Pubkey,
        fee: i64,
    ) -> Self {
        let create = SystemProgram::CreateAccount {
            tokens, 
            space,
            program_id,
        };
        Transaction::new(
            from_keypair,
            &[to],
            SystemProgram::id(),
            serialize(&create).unwrap(),
            last_id,
            fee,
        )
    }
    
    fn system_assign(from_keypair: &Keypair, last_id: Hash, program_id: Pubkey, fee: i64) -> Self {
        let create = SystemProgram::Assign { program_id };
        Transaction::new(
            from_keypair,
            &[],
            SystemProgram::id(),
            serialize(&create).unwrap(),
            last_id,
            fee,
        )
    }
    
    fn system_new(from_keypair: &Keypair, to: Pubkey, tokens: i64, last_id: Hash) -> Self {
        Transaction::system_create(from_keypair, to, last_id, tokens, 0, Pubkey::default(), 0)
    }
    
    fn system_move(
        from_keypair: &Keypair,
        to: Pubkey,
        tokens: i64,
        last_id: Hash,
        fee: i64,
    ) -> Self {
        let create = SystemProgram::Move { tokens };
        Transaction::new(
            from_keypair,
            &[to],
            SystemProgram::id(),
            serialize(&create).unwrap(),
            last_id,
            fee,
        )
    }
    
    fn system_load(
        from_keypair: &Keypair,
        last_id: Hash,
        fee: i64,
        program_id: Pubkey,
        name: String,
    ) -> Self {
        let load = SystemProgram::Load { program_id, name };
        Transaction::new(
            from_keypair,
            &[],
            SystemProgram::id(),
            serialize(&load).unwrap(),
            last_id,
            fee,
        )
    }
}

pub fn test_tx() -> Transaction {
    let keypair1 = Keypair::new();
    let pubkey1 = keypair1.pubkey();
    let zero = Hash::default();
    Transaction::system_new(&keypair1, pubkey1, 42, zero)
}

#[cfg(test)]
pub fn memfind<A: Eq>(a: &[A], b: &[A]) -> Option<usize> {
    assert!(a.len() >= b.len());
    let end = a.len() - b.len() + 1;
    for i in 0..end {
        if a[i..i + b.len()] == b[..] {
            return Some(i);
        }
    }
    None
}

