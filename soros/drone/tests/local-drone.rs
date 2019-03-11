use bitconch_drone::drone::{request_airdrop_transaction, run_local_drone};
use bitconch_sdk::hash::Hash;
use bitconch_sdk::signature::{Keypair, KeypairUtil};
use bitconch_sdk::system_instruction::SystemInstruction;
use bitconch_sdk::system_program;
use bitconch_sdk::transaction::Transaction;
use std::sync::mpsc::channel;

#[test]
fn test_local_drone() {
    let keypair = Keypair::new();
    let to = Keypair::new().pubkey();
    let tokens = 50;
    let last_id = Hash::new(&to.as_ref());
    let expected_instruction = SystemInstruction::CreateAccount {
        tokens,
        space: 0,
        program_id: system_program::id(),
    };
    let mut expected_tx = Transaction::new(
        &keypair,
        &[to],
        system_program::id(),
        &expected_instruction,
        last_id,
        0,
    );
    expected_tx.sign(&[&keypair], last_id);

    let (sender, receiver) = channel();
    run_local_drone(keypair, sender);
    let drone_addr = receiver.recv().unwrap();

    let result = request_airdrop_transaction(&drone_addr, &to, tokens, last_id);
    assert_eq!(expected_tx, result.unwrap());
}
