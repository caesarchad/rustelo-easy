pub mod budget_expr;
pub mod budget_instruction;
pub mod budget_processor;
pub mod budget_state;

use soros_sdk::pubkey::Pubkey;

const BUDGET_PROGRAM_ID: [u8; 32] = [
    2, 203, 81, 223, 225, 24, 34, 35, 203, 214, 138, 130, 144, 208, 35, 77, 63, 16, 87, 51, 47,
    198, 115, 123, 98, 188, 19, 160, 0, 0, 0, 0,
];

pub fn id() -> Pubkey {
    Pubkey::new(&BUDGET_PROGRAM_ID)
}

pub fn check_id(program_id: &Pubkey) -> bool {
    program_id.as_ref() == BUDGET_PROGRAM_ID
}
