use soros_runtime::bank::Bank;
use soros_runtime::bank::BankError;
use soros_runtime::loader_utils::load_program;
use soros_sdk::genesis_block::GenesisBlock;
use soros_sdk::native_loader;
use soros_sdk::native_program::ProgramError;
use soros_sdk::transaction::Transaction;

#[test]
fn test_program_native_failure() {
    let (genesis_block, mint_keypair) = GenesisBlock::new(50);
    let bank = Bank::new(&genesis_block);

    let program = "failure".as_bytes().to_vec();
    let program_id = load_program(&bank, &mint_keypair, &native_loader::id(), program);

    // Call user program
    let tx = Transaction::new(
        &mint_keypair,
        &[],
        &program_id,
        &1u8,
        bank.last_blockhash(),
        0,
    );
    assert_eq!(
        bank.process_transaction(&tx),
        Err(BankError::ProgramError(0, ProgramError::GenericError))
    );
}
