use bincode::{self, deserialize, serialize_into, serialized_size};
use buffett_budget::budget::Budget;
use buffett_budget::instruction::Instruction;
use chrono::prelude::{DateTime, Utc};
use buffett_budget::seal::Seal;
use buffett_interface::account::Account;
use buffett_interface::pubkey::Pubkey;
use std::io;
use crate::transaction::Transaction;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum BudgetError {
    InsufficientFunds(Pubkey),
    ContractAlreadyExists(Pubkey),
    ContractNotPending(Pubkey),
    SourceIsPendingContract(Pubkey),
    UninitializedContract(Pubkey),
    NegativeTokens,
    DestinationMissing(Pubkey),
    FailedWitness,
    UserdataTooSmall,
    UserdataDeserializeFailure,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct BudgetState {
    pub initialized: bool,
    pub pending_budget: Option<Budget>,
}

pub const BUDGET_PROGRAM_ID: [u8; 32] = [
    1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];
impl BudgetState {
    fn is_pending(&self) -> bool {
        self.pending_budget != None
    }
    pub fn id() -> Pubkey {
        Pubkey::new(&BUDGET_PROGRAM_ID)
    }
    pub fn check_id(program_id: &Pubkey) -> bool {
        program_id.as_ref() == BUDGET_PROGRAM_ID
    }

    fn apply_signature(
        &mut self,
        keys: &[Pubkey],
        account: &mut [Account],
    ) -> Result<(), BudgetError> {
        let mut final_payment = None;
        if let Some(ref mut budget) = self.pending_budget {
            budget.apply_seal(&Seal::Signature, &keys[0]);
            final_payment = budget.final_payment();
        }

        if let Some(payment) = final_payment {
            if keys.len() < 2 || payment.to != keys[2] {
                trace!("destination missing");
                return Err(BudgetError::DestinationMissing(payment.to));
            }
            self.pending_budget = None;
            account[1].tokens -= payment.balance;
            account[2].tokens += payment.balance;
        }
        Ok(())
    }

    fn apply_timestamp(
        &mut self,
        keys: &[Pubkey],
        accounts: &mut [Account],
        dt: DateTime<Utc>,
    ) -> Result<(), BudgetError> {
        let mut final_payment = None;

        if let Some(ref mut budget) = self.pending_budget {
            budget.apply_seal(&Seal::Timestamp(dt), &keys[0]);
            final_payment = budget.final_payment();
        }

        if let Some(payment) = final_payment {
            if keys.len() < 2 || payment.to != keys[2] {
                trace!("destination missing");
                return Err(BudgetError::DestinationMissing(payment.to));
            }
            self.pending_budget = None;
            accounts[1].tokens -= payment.balance;
            accounts[2].tokens += payment.balance;
        }
        Ok(())
    }

    fn apply_debits_to_budget_state(
        tx: &Transaction,
        accounts: &mut [Account],
        instruction: &Instruction,
    ) -> Result<(), BudgetError> {
        {
            if !accounts[0].userdata.is_empty() {
                trace!("source is pending");
                return Err(BudgetError::SourceIsPendingContract(tx.keys[0]));
            }
            if let Instruction::NewContract(contract) = &instruction {
                if contract.tokens < 0 {
                    trace!("negative tokens");
                    return Err(BudgetError::NegativeTokens);
                }

                if accounts[0].tokens < contract.tokens {
                    trace!("insufficient funds");
                    return Err(BudgetError::InsufficientFunds(tx.keys[0]));
                } else {
                    accounts[0].tokens -= contract.tokens;
                }
            };
        }
        Ok(())
    }

    
    fn apply_credits_to_budget_state(
        tx: &Transaction,
        accounts: &mut [Account],
        instruction: &Instruction,
    ) -> Result<(), BudgetError> {
        match instruction {
            Instruction::NewContract(contract) => {
                let budget = contract.budget.clone();
                if let Some(payment) = budget.final_payment() {
                    accounts[1].tokens += payment.balance;
                    Ok(())
                } else {
                    let existing = Self::deserialize(&accounts[1].userdata).ok();
                    if Some(true) == existing.map(|x| x.initialized) {
                        trace!("contract already exists");
                        Err(BudgetError::ContractAlreadyExists(tx.keys[1]))
                    } else {
                        let mut state = BudgetState::default();
                        state.pending_budget = Some(budget);
                        accounts[1].tokens += contract.tokens;
                        state.initialized = true;
                        state.serialize(&mut accounts[1].userdata)
                    }
                }
            }
            Instruction::ApplyDatetime(dt) => {
                if let Ok(mut state) = Self::deserialize(&accounts[1].userdata) {
                    if !state.is_pending() {
                        Err(BudgetError::ContractNotPending(tx.keys[1]))
                    } else if !state.initialized {
                        trace!("contract is uninitialized");
                        Err(BudgetError::UninitializedContract(tx.keys[1]))
                    } else {
                        trace!("apply timestamp");
                        state.apply_timestamp(&tx.keys, accounts, *dt)?;
                        trace!("apply timestamp committed");
                        state.serialize(&mut accounts[1].userdata)
                    }
                } else {
                    Err(BudgetError::UninitializedContract(tx.keys[1]))
                }
            }
            Instruction::ApplySignature => {
                if let Ok(mut state) = Self::deserialize(&accounts[1].userdata) {
                    if !state.is_pending() {
                        Err(BudgetError::ContractNotPending(tx.keys[1]))
                    } else if !state.initialized {
                        trace!("contract is uninitialized");
                        Err(BudgetError::UninitializedContract(tx.keys[1]))
                    } else {
                        trace!("apply signature");
                        state.apply_signature(&tx.keys, accounts)?;
                        trace!("apply signature committed");
                        state.serialize(&mut accounts[1].userdata)
                    }
                } else {
                    Err(BudgetError::UninitializedContract(tx.keys[1]))
                }
            }
            Instruction::NewVote(_vote) => {
                
                trace!("GOT VOTE! last_id={}", tx.last_id);
                Ok(())
            }
        }
    }
    fn serialize(&self, output: &mut [u8]) -> Result<(), BudgetError> {
        let len = serialized_size(self).unwrap() as u64;
        if output.len() < len as usize {
            warn!(
                "{} bytes required to serialize, only have {} bytes",
                len,
                output.len()
            );
            return Err(BudgetError::UserdataTooSmall);
        }
        {
            let writer = io::BufWriter::new(&mut output[..8]);
            serialize_into(writer, &len).unwrap();
        }

        {
            let writer = io::BufWriter::new(&mut output[8..8 + len as usize]);
            serialize_into(writer, self).unwrap();
        }
        Ok(())
    }

    pub fn deserialize(input: &[u8]) -> bincode::Result<Self> {
        if input.len() < 8 {
            return Err(Box::new(bincode::ErrorKind::SizeLimit));
        }
        let len: u64 = deserialize(&input[..8]).unwrap();
        if len < 2 {
            return Err(Box::new(bincode::ErrorKind::SizeLimit));
        }
        if input.len() < 8 + len as usize {
            return Err(Box::new(bincode::ErrorKind::SizeLimit));
        }
        deserialize(&input[8..8 + len as usize])
    }

    pub fn process_transaction(
        tx: &Transaction,
        accounts: &mut [Account],
    ) -> Result<(), BudgetError> {
        if let Ok(instruction) = deserialize(&tx.userdata) {
            trace!("process_transaction: {:?}", instruction);
            Self::apply_debits_to_budget_state(tx, accounts, &instruction)
                .and_then(|_| Self::apply_credits_to_budget_state(tx, accounts, &instruction))
        } else {
            info!("transaction instructions invalid on : {:?}", tx.userdata);
            Err(BudgetError::UserdataDeserializeFailure)
        }
    }

    pub fn get_balance(account: &Account) -> i64 {
        if let Ok(state) = deserialize(&account.userdata) {
            let state: BudgetState = state;
            if state.is_pending() {
                0
            } else {
                account.tokens
            }
        } else {
            account.tokens
        }
    }
}
