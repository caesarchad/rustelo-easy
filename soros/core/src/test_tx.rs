use soros_sdk::hash::Hash;
use soros_sdk::instruction::CompiledInstruction;
use soros_sdk::signature::{Keypair, KeypairUtil};
use soros_sdk::system_instruction::SystemInstruction;
use soros_sdk::system_program;
use soros_sdk::system_transaction;
use soros_sdk::transaction::Transaction;

pub fn test_tx() -> Transaction {
    let keypair1 = Keypair::new();
    let pubkey1 = keypair1.pubkey();
    let zero = Hash::default();
    system_transaction::create_user_account(&keypair1, &pubkey1, 42, zero, 0)
}

pub fn test_multisig_tx() -> Transaction {
    let keypair0 = Keypair::new();
    let keypair1 = Keypair::new();
    let keypairs = vec![&keypair0, &keypair1];
    // let lamports = 5;
    let dif = 5;
    let blockhash = Hash::default();

    // let transfer_instruction = SystemInstruction::Transfer { lamports };
    let transfer_instruction = SystemInstruction::Transfer { dif };

    let program_ids = vec![system_program::id(), soros_budget_api::id()];

    let instructions = vec![CompiledInstruction::new(
        0,
        &transfer_instruction,
        vec![0, 1],
    )];

    Transaction::new_with_compiled_instructions(
        &keypairs,
        &[],
        blockhash,
        program_ids,
        instructions,
    )
}
