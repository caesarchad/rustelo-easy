use bincode::deserialize;
use crate::dynamic_program::DynamicProgram;
use buffett_interface::account::Account;
use buffett_interface::pubkey::Pubkey;
use std::collections::HashMap;
use std::sync::RwLock;
use crate::transaction::Transaction;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum SystemProgram {
    CreateAccount {
        tokens: i64,
        space: u64,
        program_id: Pubkey,
    },
    Assign { program_id: Pubkey },
    Move { tokens: i64 },
    Load { program_id: Pubkey, name: String },
}

pub const SYSTEM_PROGRAM_ID: [u8; 32] = [0u8; 32];

impl SystemProgram {
    pub fn check_id(program_id: &Pubkey) -> bool {
        program_id.as_ref() == SYSTEM_PROGRAM_ID
    }

    pub fn id() -> Pubkey {
        Pubkey::new(&SYSTEM_PROGRAM_ID)
    }
    pub fn get_balance(account: &Account) -> i64 {
        account.tokens
    }
    pub fn process_transaction(
        tx: &Transaction,
        accounts: &mut [Account],
        loaded_programs: &RwLock<HashMap<Pubkey, DynamicProgram>>,
    ) {
        if let Ok(syscall) = deserialize(&tx.userdata) {
            trace!("process_transaction: {:?}", syscall);
            match syscall {
                SystemProgram::CreateAccount {
                    tokens,
                    space,
                    program_id,
                } => {
                    if !Self::check_id(&accounts[0].program_id) {
                        return;
                    }
                    if space > 0
                        && (!accounts[1].userdata.is_empty()
                            || !Self::check_id(&accounts[1].program_id))
                    {
                        return;
                    }
                    accounts[0].tokens -= tokens;
                    accounts[1].tokens += tokens;
                    accounts[1].program_id = program_id;
                    accounts[1].userdata = vec![0; space as usize];
                }
                SystemProgram::Assign { program_id } => {
                    if !Self::check_id(&accounts[0].program_id) {
                        return;
                    }
                    accounts[0].program_id = program_id;
                }
                SystemProgram::Move { tokens } => {
                    accounts[0].tokens -= tokens;
                    accounts[1].tokens += tokens;
                }
                SystemProgram::Load { program_id, name } => {
                    let mut hashmap = loaded_programs.write().unwrap();
                    hashmap.insert(program_id, DynamicProgram::new(name));
                }
            }
        } else {
            info!("Invalid transaction userdata: {:?}", tx.userdata);
        }
    }
}
