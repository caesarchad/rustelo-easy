use chrono::prelude::*;
use buffett_interface::pubkey::Pubkey;


#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Witness {
    Timestamp(DateTime<Utc>),
    Signature,
}


#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Payment {
    pub tokens: i64,
    pub to: Pubkey,
}
