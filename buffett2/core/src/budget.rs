use chrono::prelude::*;
use crate::payment_plan::{Payment, Witness};
use buffett_interface::pubkey::Pubkey;
use std::mem;


#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Condition {
    Timestamp(DateTime<Utc>, Pubkey),
    Signature(Pubkey),
}

impl Condition {
    pub fn is_satisfied(&self, witness: &Witness, from: &Pubkey) -> bool {
        match (self, witness) {
            (Condition::Signature(pubkey), Witness::Signature) => pubkey == from,
            (Condition::Timestamp(dt, pubkey), Witness::Timestamp(last_time)) => {
                pubkey == from && dt <= last_time
            }
            _ => false,
        }
    }
}


#[repr(C)]
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Budget {
    Pay(Payment),
    After(Condition, Payment),
    Or((Condition, Payment), (Condition, Payment)),
    And(Condition, Condition, Payment),
}

impl Budget {
    
    pub fn new_payment(tokens: i64, to: Pubkey) -> Self {
        Budget::Pay(Payment { tokens, to })
    }

    
    pub fn new_authorized_payment(from: Pubkey, tokens: i64, to: Pubkey) -> Self {
        Budget::After(Condition::Signature(from), Payment { tokens, to })
    }

    
    pub fn new_2_2_multisig_payment(from0: Pubkey, from1: Pubkey, tokens: i64, to: Pubkey) -> Self {
        Budget::And(
            Condition::Signature(from0),
            Condition::Signature(from1),
            Payment { tokens, to },
        )
    }

    
    pub fn new_future_payment(dt: DateTime<Utc>, from: Pubkey, tokens: i64, to: Pubkey) -> Self {
        Budget::After(Condition::Timestamp(dt, from), Payment { tokens, to })
    }

    
    pub fn new_cancelable_future_payment(
        dt: DateTime<Utc>,
        from: Pubkey,
        tokens: i64,
        to: Pubkey,
    ) -> Self {
        Budget::Or(
            (Condition::Timestamp(dt, from), Payment { tokens, to }),
            (Condition::Signature(from), Payment { tokens, to: from }),
        )
    }

   
    pub fn final_payment(&self) -> Option<Payment> {
        match self {
            Budget::Pay(payment) => Some(payment.clone()),
            _ => None,
        }
    }

    
    pub fn verify(&self, spendable_tokens: i64) -> bool {
        match self {
            Budget::Pay(payment) | Budget::After(_, payment) | Budget::And(_, _, payment) => {
                payment.tokens == spendable_tokens
            }
            Budget::Or(a, b) => a.1.tokens == spendable_tokens && b.1.tokens == spendable_tokens,
        }
    }

    
    pub fn apply_witness(&mut self, witness: &Witness, from: &Pubkey) {
        let new_budget = match self {
            Budget::After(cond, payment) if cond.is_satisfied(witness, from) => {
                Some(Budget::Pay(payment.clone()))
            }
            Budget::Or((cond, payment), _) if cond.is_satisfied(witness, from) => {
                Some(Budget::Pay(payment.clone()))
            }
            Budget::Or(_, (cond, payment)) if cond.is_satisfied(witness, from) => {
                Some(Budget::Pay(payment.clone()))
            }
            Budget::And(cond0, cond1, payment) => {
                if cond0.is_satisfied(witness, from) {
                    Some(Budget::After(cond1.clone(), payment.clone()))
                } else if cond1.is_satisfied(witness, from) {
                    Some(Budget::After(cond0.clone(), payment.clone()))
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(budget) = new_budget {
            mem::replace(self, budget);
        }
    }
}

