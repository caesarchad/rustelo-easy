use buffett_crypto::hash::Hash;
use buffett_crypto::signature::Signature;
use buffett_interface::account::Account;
use buffett_interface::pubkey::Pubkey;

#[cfg_attr(feature = "cargo-clippy", allow(large_enum_variant))]
#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum Request {
    GetAccount { key: Pubkey },
    GetLastId,
    GetTransactionCount,
    GetSignature { signature: Signature },
    GetFinality,
}

impl Request {
    
    pub fn verify(&self) -> bool {
        true
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Response {
    Account {
        key: Pubkey,
        account: Option<Account>,
    },
    LastId {
        id: Hash,
    },
    TransactionCount {
        transaction_count: u64,
    },
    SignatureStatus {
        signature_status: bool,
    },
    Finality {
        time: usize,
    },
}
