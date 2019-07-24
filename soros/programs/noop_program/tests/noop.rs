use soros_runtime::bank::Bank;
use soros_runtime::bank_client::BankClient;
use soros_runtime::loader_utils::{create_invoke_instruction, load_program};
use soros_sdk::client::SyncClient;
use soros_sdk::genesis_block::GenesisBlock;
use soros_sdk::native_loader;
use soros_sdk::signature::KeypairUtil;

#[test]
fn test_program_native_noop() {
    soros_logger::setup();

    let (genesis_block, alice_keypair) = GenesisBlock::new(50);
    let bank = Bank::new(&genesis_block);
    let bank_client = BankClient::new(bank);

    let program = "soros_noop_program".as_bytes().to_vec();
    let program_id = load_program(&bank_client, &alice_keypair, &native_loader::id(), program);

    // Call user program
    let instruction = create_invoke_instruction(alice_keypair.pubkey(), program_id, &1u8);
    bank_client
        .send_instruction(&alice_keypair, instruction);
}
