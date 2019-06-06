use bincode::{deserialize, serialize};
use buffett_budget::budget::{Budget};
use buffett_budget::condition::{Condition};
use buffett_budget::instruction::{Contract, Instruction, Vote};
use buffett_budget::payment::Payment;
use crate::budget_program::BudgetState;
use chrono::prelude::*;
use buffett_crypto::hash::Hash;
use buffett_crypto::signature::Keypair;
use buffett_interface::pubkey::Pubkey;
use crate::transaction::Transaction;

pub trait BudgetTransaction {
    fn budget_new_taxed(
        from_keypair: &Keypair,
        to: Pubkey,
        tokens: i64,
        fee: i64,
        last_id: Hash,
    ) -> Self;

    fn budget_new(from_keypair: &Keypair, to: Pubkey, tokens: i64, last_id: Hash) -> Self;

    fn budget_new_timestamp(
        from_keypair: &Keypair,
        contract: Pubkey,
        to: Pubkey,
        dt: DateTime<Utc>,
        last_id: Hash,
    ) -> Self;

    fn budget_new_signature(
        from_keypair: &Keypair,
        contract: Pubkey,
        to: Pubkey,
        last_id: Hash,
    ) -> Self;

    fn budget_new_vote(from_keypair: &Keypair, vote: Vote, last_id: Hash, fee: i64) -> Self;

    fn budget_new_on_date(
        from_keypair: &Keypair,
        to: Pubkey,
        contract: Pubkey,
        dt: DateTime<Utc>,
        dt_pubkey: Pubkey,
        cancelable: Option<Pubkey>,
        tokens: i64,
        last_id: Hash,
    ) -> Self;

    fn budget_new_when_signed(
        from_keypair: &Keypair,
        to: Pubkey,
        contract: Pubkey,
        witness: Pubkey,
        cancelable: Option<Pubkey>,
        tokens: i64,
        last_id: Hash,
    ) -> Self;

    fn vote(&self) -> Option<(Pubkey, Vote, Hash)>;

    fn instruction(&self) -> Option<Instruction>;

    fn verify_plan(&self) -> bool;
}

impl BudgetTransaction for Transaction {
    
    fn budget_new_taxed(
        from_keypair: &Keypair,
        to: Pubkey,
        tokens: i64,
        fee: i64,
        last_id: Hash,
    ) -> Self {
        let payment = Payment {
            balance: tokens - fee,
            to,
        };
        let budget = Budget::Pay(payment);
        let instruction = Instruction::NewContract(Contract { budget, tokens });
        let userdata = serialize(&instruction).unwrap();
        Self::new(
            from_keypair,
            &[to],
            BudgetState::id(),
            userdata,
            last_id,
            fee,
        )
    }

    
    fn budget_new(from_keypair: &Keypair, to: Pubkey, tokens: i64, last_id: Hash) -> Self {
        Self::budget_new_taxed(from_keypair, to, tokens, 0, last_id)
    }

    
    fn budget_new_timestamp(
        from_keypair: &Keypair,
        contract: Pubkey,
        to: Pubkey,
        dt: DateTime<Utc>,
        last_id: Hash,
    ) -> Self {
        let instruction = Instruction::ApplyDatetime(dt);
        let userdata = serialize(&instruction).unwrap();
        Self::new(
            from_keypair,
            &[contract, to],
            BudgetState::id(),
            userdata,
            last_id,
            0,
        )
    }

    
    fn budget_new_signature(
        from_keypair: &Keypair,
        contract: Pubkey,
        to: Pubkey,
        last_id: Hash,
    ) -> Self {
        let instruction = Instruction::ApplySignature;
        let userdata = serialize(&instruction).unwrap();
        Self::new(
            from_keypair,
            &[contract, to],
            BudgetState::id(),
            userdata,
            last_id,
            0,
        )
    }

    fn budget_new_vote(from_keypair: &Keypair, vote: Vote, last_id: Hash, fee: i64) -> Self {
        let instruction = Instruction::NewVote(vote);
        let userdata = serialize(&instruction).expect("serialize instruction");
        Self::new(from_keypair, &[], BudgetState::id(), userdata, last_id, fee)
    }

    
    fn budget_new_on_date(
        from_keypair: &Keypair,
        to: Pubkey,
        contract: Pubkey,
        dt: DateTime<Utc>,
        dt_pubkey: Pubkey,
        cancelable: Option<Pubkey>,
        tokens: i64,
        last_id: Hash,
    ) -> Self {
        let budget = if let Some(from) = cancelable {
            Budget::Or(
                (Condition::Timestamp(dt, dt_pubkey), Payment { balance:tokens, to }),
                (Condition::Signature(from), Payment { balance:tokens, to: from }),
            )
        } else {
            Budget::After(Condition::Timestamp(dt, dt_pubkey), Payment { balance:tokens, to })
        };
        let instruction = Instruction::NewContract(Contract { budget, tokens });
        let userdata = serialize(&instruction).expect("serialize instruction");
        Self::new(
            from_keypair,
            &[contract],
            BudgetState::id(),
            userdata,
            last_id,
            0,
        )
    }
    
    fn budget_new_when_signed(
        from_keypair: &Keypair,
        to: Pubkey,
        contract: Pubkey,
        witness: Pubkey,
        cancelable: Option<Pubkey>,
        tokens: i64,
        last_id: Hash,
    ) -> Self {
        let budget = if let Some(from) = cancelable {
            Budget::Or(
                (Condition::Signature(witness), Payment { balance:tokens, to }),
                (Condition::Signature(from), Payment { balance:tokens, to: from }),
            )
        } else {
            Budget::After(Condition::Signature(witness), Payment { balance:tokens, to })
        };
        let instruction = Instruction::NewContract(Contract { budget, tokens });
        let userdata = serialize(&instruction).expect("serialize instruction");
        Self::new(
            from_keypair,
            &[contract],
            BudgetState::id(),
            userdata,
            last_id,
            0,
        )
    }

    fn vote(&self) -> Option<(Pubkey, Vote, Hash)> {
        if let Some(Instruction::NewVote(vote)) = self.instruction() {
            Some((*self.from(), vote, self.last_id))
        } else {
            None
        }
    }

    fn instruction(&self) -> Option<Instruction> {
        deserialize(&self.userdata).ok()
    }

    
    fn verify_plan(&self) -> bool {
        if let Some(Instruction::NewContract(contract)) = self.instruction() {
            self.fee >= 0
                && self.fee <= contract.tokens
                && contract.budget.verify(contract.tokens - self.fee)
        } else {
            true
        }
    }
}

