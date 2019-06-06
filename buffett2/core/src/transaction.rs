use bincode::serialize;
use buffett_crypto::hash::{Hash, Hasher};
use buffett_crypto::signature::{Keypair, KeypairUtil,Signature};
use buffett_interface::pubkey::Pubkey;
use std::mem::size_of;

pub const SIGNED_DATA_OFFSET: usize = size_of::<Signature>();
pub const SIG_OFFSET: usize = 0;
pub const PUB_KEY_OFFSET: usize = size_of::<Signature>() + size_of::<u64>();


#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Transaction {
    
    pub signature: Signature,

    pub keys: Vec<Pubkey>,

    pub program_id: Pubkey,

    pub last_id: Hash,

    pub fee: i64,

    pub userdata: Vec<u8>,
}

impl Transaction {
    pub fn new(
        from_keypair: &Keypair,
        transaction_keys: &[Pubkey],
        program_id: Pubkey,
        userdata: Vec<u8>,
        last_id: Hash,
        fee: i64,
    ) -> Self {
        let from = from_keypair.pubkey();
        let mut keys = vec![from];
        keys.extend_from_slice(transaction_keys);
        let mut tx = Transaction {
            signature: Signature::default(),
            keys,
            program_id,
            last_id,
            fee,
            userdata,
        };
        tx.sign(from_keypair);
        tx
    }

    pub fn get_sign_data(&self) -> Vec<u8> {
        let mut data = serialize(&(&self.keys)).expect("serialize keys");

        let program_id = serialize(&(&self.program_id)).expect("serialize program_id");
        data.extend_from_slice(&program_id);

        let last_id_data = serialize(&(&self.last_id)).expect("serialize last_id");
        data.extend_from_slice(&last_id_data);

        let fee_data = serialize(&(&self.fee)).expect("serialize last_id");
        data.extend_from_slice(&fee_data);

        let userdata = serialize(&(&self.userdata)).expect("serialize userdata");
        data.extend_from_slice(&userdata);
        data
    }

    pub fn sign(&mut self, keypair: &Keypair) {
        let sign_data = self.get_sign_data();
        self.signature = Signature::new(keypair.sign(&sign_data).as_ref());
    }

    pub fn verify_signature(&self) -> bool {
        warn!("transaction signature verification called");
        self.signature
            .verify(&self.from().as_ref(), &self.get_sign_data())
    }

    pub fn from(&self) -> &Pubkey {
        &self.keys[0]
    }

    pub fn hash(transactions: &[Transaction]) -> Hash {
        let mut hasher = Hasher::default();
        transactions
            .iter()
            .for_each(|tx| hasher.hash(&tx.signature.as_ref()));
        hasher.result()
    }
}
