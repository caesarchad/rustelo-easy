use crate::blocktree::{create_new_tmp_ledger, tmp_copy_blocktree};
use crate::cluster::Cluster;
use crate::cluster_info::{Node, FULLNODE_PORT_RANGE};
use crate::contact_info::ContactInfo;
use crate::fullnode::{Fullnode, FullnodeConfig};
use crate::gossip_service::discover;
use crate::service::Service;
use soros_client::client::create_client;
use soros_client::thin_client::{retry_get_balance, ThinClient};
use soros_sdk::genesis_block::GenesisBlock;
use soros_sdk::pubkey::Pubkey;
use soros_sdk::signature::{Keypair, KeypairUtil};
use soros_sdk::system_transaction::SystemTransaction;
use soros_sdk::timing::DEFAULT_SLOTS_PER_EPOCH;
use soros_sdk::timing::DEFAULT_TICKS_PER_SLOT;
use soros_vote_api::vote_state::VoteState;
use soros_vote_api::vote_transaction::VoteTransaction;
use std::collections::HashMap;
use std::fs::remove_dir_all;
use std::io::{Error, ErrorKind, Result};
use std::sync::Arc;

pub struct FullnodeInfo {
    pub keypair: Arc<Keypair>,
    pub ledger_path: String,
}

impl FullnodeInfo {
    fn new(keypair: Arc<Keypair>, ledger_path: String) -> Self {
        Self {
            keypair,
            ledger_path,
        }
    }
}

pub struct LocalCluster {
    /// Keypair with funding to particpiate in the network
    pub funding_keypair: Keypair,
    pub fullnode_config: FullnodeConfig,
    /// Entry point from which the rest of the network can be discovered
    pub entry_point_info: ContactInfo,
    pub fullnodes: HashMap<Pubkey, Fullnode>,
    pub fullnode_infos: HashMap<Pubkey, FullnodeInfo>,
}

impl LocalCluster {
    pub fn new(num_nodes: usize, cluster_lamports: u64, lamports_per_node: u64) -> Self {
        let stakes: Vec<_> = (0..num_nodes).map(|_| lamports_per_node).collect();
        Self::new_with_config(&stakes, cluster_lamports, &FullnodeConfig::default())
    }

    pub fn new_with_config(
        node_stakes: &[u64],
        cluster_lamports: u64,
        fullnode_config: &FullnodeConfig,
    ) -> Self {
        Self::new_with_tick_config(
            node_stakes,
            cluster_lamports,
            fullnode_config,
            DEFAULT_TICKS_PER_SLOT,
            DEFAULT_SLOTS_PER_EPOCH,
        )
    }

    pub fn new_with_tick_config(
        node_stakes: &[u64],
        cluster_lamports: u64,
        fullnode_config: &FullnodeConfig,
        ticks_per_slot: u64,
        slots_per_epoch: u64,
    ) -> Self {
        let leader_keypair = Arc::new(Keypair::new());
        let leader_pubkey = leader_keypair.pubkey();
        let leader_node = Node::new_localhost_with_pubkey(&leader_keypair.pubkey());
        let (mut genesis_block, mint_keypair) =
            GenesisBlock::new_with_leader(cluster_lamports, &leader_pubkey, node_stakes[0]);
        genesis_block.ticks_per_slot = ticks_per_slot;
        genesis_block.slots_per_epoch = slots_per_epoch;
        let (genesis_ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_block);
        let leader_ledger_path = tmp_copy_blocktree!(&genesis_ledger_path);
        let voting_keypair = Keypair::new();
        let leader_contact_info = leader_node.info.clone();
        let leader_server = Fullnode::new(
            leader_node,
            &leader_keypair,
            &leader_ledger_path,
            &voting_keypair.pubkey(),
            voting_keypair,
            None,
            fullnode_config,
        );
        let mut fullnodes = HashMap::new();
        let mut fullnode_infos = HashMap::new();
        fullnodes.insert(leader_pubkey, leader_server);
        fullnode_infos.insert(
            leader_pubkey,
            FullnodeInfo::new(leader_keypair.clone(), leader_ledger_path),
        );

        let mut client = create_client(
            leader_contact_info.client_facing_addr(),
            FULLNODE_PORT_RANGE,
        );
        for stake in &node_stakes[1..] {
            // Must have enough tokens to fund vote account and set delegate
            assert!(*stake > 2);
            let validator_keypair = Arc::new(Keypair::new());
            let voting_keypair = Keypair::new();
            let validator_pubkey = validator_keypair.pubkey();
            let validator_node = Node::new_localhost_with_pubkey(&validator_keypair.pubkey());
            let ledger_path = tmp_copy_blocktree!(&genesis_ledger_path);

            // Send each validator some lamports to vote
            let validator_balance =
                Self::transfer(&mut client, &mint_keypair, &validator_pubkey, *stake);
            info!(
                "validator {} balance {}",
                validator_pubkey, validator_balance
            );

            Self::create_and_fund_vote_account(
                &mut client,
                &voting_keypair,
                &validator_keypair,
                stake - 1,
            )
            .unwrap();
            let validator_server = Fullnode::new(
                validator_node,
                &validator_keypair,
                &ledger_path,
                &voting_keypair.pubkey(),
                voting_keypair,
                Some(&leader_contact_info),
                fullnode_config,
            );
            fullnodes.insert(validator_keypair.pubkey(), validator_server);
            fullnode_infos.insert(
                validator_keypair.pubkey(),
                FullnodeInfo::new(validator_keypair.clone(), ledger_path),
            );
        }
        discover(&leader_contact_info.gossip, node_stakes.len()).unwrap();
        Self {
            funding_keypair: mint_keypair,
            entry_point_info: leader_contact_info,
            fullnodes,
            fullnode_config: fullnode_config.clone(),
            fullnode_infos,
        }
    }

    pub fn exit(&self) {
        for node in self.fullnodes.values() {
            node.exit();
        }
    }

    pub fn close_preserve_ledgers(&mut self) {
        self.exit();
        for (_, node) in self.fullnodes.drain() {
            node.join().unwrap();
        }
    }

    fn close(&mut self) {
        self.close_preserve_ledgers();
        for info in self.fullnode_infos.values() {
            remove_dir_all(&info.ledger_path)
                .unwrap_or_else(|_| panic!("Unable to remove {}", info.ledger_path));
        }
    }

    fn transfer(
        client: &mut ThinClient,
        source_keypair: &Keypair,
        dest_pubkey: &Pubkey,
        lamports: u64,
    ) -> u64 {
        trace!("getting leader blockhash");
        let blockhash = client.get_recent_blockhash();
        let mut tx =
            SystemTransaction::new_account(&source_keypair, dest_pubkey, lamports, blockhash, 0);
        info!(
            "executing transfer of {} from {} to {}",
            lamports,
            source_keypair.pubkey(),
            *dest_pubkey
        );
        client
            .retry_transfer(&source_keypair, &mut tx, 5)
            .expect("client transfer");
        retry_get_balance(client, dest_pubkey, Some(lamports)).expect("get balance")
    }

    fn create_and_fund_vote_account(
        client: &mut ThinClient,
        vote_account: &Keypair,
        from_account: &Arc<Keypair>,
        amount: u64,
    ) -> Result<()> {
        let vote_account_pubkey = vote_account.pubkey();
        let delegate_id = from_account.pubkey();
        // Create the vote account if necessary
        if client.poll_get_balance(&vote_account_pubkey).unwrap_or(0) == 0 {
            // 1) Create vote account
            let mut transaction = VoteTransaction::new_account(
                from_account,
                &vote_account_pubkey,
                client.get_recent_blockhash(),
                amount,
                1,
            );

            client
                .retry_transfer(&from_account, &mut transaction, 5)
                .expect("client transfer");
            retry_get_balance(client, &vote_account_pubkey, Some(amount)).expect("get balance");

            // 2) Set delegate for new vote account
            let mut transaction = VoteTransaction::delegate_vote_account(
                vote_account,
                client.get_recent_blockhash(),
                &delegate_id,
                0,
            );

            client
                .retry_transfer(&vote_account, &mut transaction, 5)
                .expect("client transfer 2");
        }

        info!("Checking for vote account registration");
        let vote_account_user_data = client.get_account_data(&vote_account_pubkey);
        if let Ok(Some(vote_account_user_data)) = vote_account_user_data {
            if let Ok(vote_state) = VoteState::deserialize(&vote_account_user_data) {
                if vote_state.delegate_id == delegate_id {
                    return Ok(());
                }
            }
        }

        Err(Error::new(
            ErrorKind::Other,
            "expected successful vote account registration",
        ))
    }
}

impl Cluster for LocalCluster {
    fn restart_node(&mut self, pubkey: Pubkey) {
        // Shut down the fullnode
        let node = self.fullnodes.remove(&pubkey).unwrap();
        node.exit();
        node.join().unwrap();

        // Restart the node
        let fullnode_info = &self.fullnode_infos[&pubkey];
        let node = Node::new_localhost_with_pubkey(&fullnode_info.keypair.pubkey());
        if pubkey == self.entry_point_info.id {
            self.entry_point_info = node.info.clone();
        }
        let new_voting_keypair = Keypair::new();
        let restarted_node = Fullnode::new(
            node,
            &fullnode_info.keypair,
            &fullnode_info.ledger_path,
            &new_voting_keypair.pubkey(),
            new_voting_keypair,
            None,
            &self.fullnode_config,
        );

        self.fullnodes.insert(pubkey, restarted_node);
    }

    fn get_node_ids(&self) -> Vec<Pubkey> {
        self.fullnodes.keys().cloned().collect()
    }
}

impl Drop for LocalCluster {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_local_cluster_start_and_exit() {
        soros_logger::setup();
        let _cluster = LocalCluster::new(1, 100, 3);
    }

    #[test]
    fn test_local_cluster_start_and_exit_with_config() {
        soros_logger::setup();
        let mut fullnode_exit = FullnodeConfig::default();
        fullnode_exit.rpc_config.enable_fullnode_exit = true;
        let _cluster = LocalCluster::new_with_tick_config(&[3], 100, &fullnode_exit, 16, 16);
    }
}