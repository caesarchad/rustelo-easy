#![feature(test)]

extern crate test;
#[macro_use]
extern crate soros;

use rand::{thread_rng, Rng};
use rayon::prelude::*;
use soros::banking_stage::{create_test_recorder, BankingStage};
use soros::blocktree::{get_tmp_ledger_path, Blocktree};
use soros::cluster_info::ClusterInfo;
use soros::cluster_info::Node;
use soros::leader_schedule_cache::LeaderScheduleCache;
use soros::packet::to_packets_chunked;
use soros::poh_recorder::WorkingBankEntries;
use soros::service::Service;
use soros_runtime::bank::Bank;
use soros_sdk::genesis_block::GenesisBlock;
use soros_sdk::hash::hash;
use soros_sdk::pubkey::Pubkey;
use soros_sdk::signature::{KeypairUtil, Signature};
use soros_sdk::system_transaction;
use soros_sdk::timing::{DEFAULT_TICKS_PER_SLOT, MAX_RECENT_BLOCKHASHES};
use std::iter;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use test::Bencher;

fn check_txs(receiver: &Receiver<WorkingBankEntries>, ref_tx_count: usize) {
    let mut total = 0;
    loop {
        let entries = receiver.recv_timeout(Duration::new(1, 0));
        if let Ok((_, entries)) = entries {
            for (entry, _) in &entries {
                total += entry.transactions.len();
            }
        } else {
            break;
        }
        if total >= ref_tx_count {
            break;
        }
    }
    assert_eq!(total, ref_tx_count);
}

#[bench]
#[ignore]
fn bench_banking_stage_multi_accounts(bencher: &mut Bencher) {
    let num_threads = BankingStage::num_threads() as usize;
    //   a multiple of packet chunk  2X duplicates to avoid races
    let txes = 192 * 50 * num_threads * 2;
    let mint_total = 1_000_000_000_000;
    let (genesis_block, mint_keypair) = GenesisBlock::new(mint_total);

    let (verified_sender, verified_receiver) = channel();
    let (vote_sender, vote_receiver) = channel();
    let bank = Arc::new(Bank::new(&genesis_block));
    let leader_schedule_cache = Arc::new(LeaderScheduleCache::new_from_bank(&bank));
    let dummy = system_transaction::transfer(
        &mint_keypair,
        &mint_keypair.pubkey(),
        1,
        genesis_block.hash(),
        0,
    );
    let transactions: Vec<_> = (0..txes)
        .into_par_iter()
        .map(|_| {
            let mut new = dummy.clone();
            let from: Vec<u8> = (0..64).map(|_| thread_rng().gen()).collect();
            let to: Vec<u8> = (0..64).map(|_| thread_rng().gen()).collect();
            let sig: Vec<u8> = (0..64).map(|_| thread_rng().gen()).collect();
            new.message.account_keys[0] = Pubkey::new(&from[0..32]);
            new.message.account_keys[1] = Pubkey::new(&to[0..32]);
            new.signatures = vec![Signature::new(&sig[0..64])];
            new
        })
        .collect();
    // fund all the accounts
    transactions.iter().for_each(|tx| {
        let fund = system_transaction::transfer(
            &mint_keypair,
            &tx.message.account_keys[0],
            mint_total / txes as u64,
            genesis_block.hash(),
            0,
        );
        let x = bank.process_transaction(&fund);
        x.unwrap();
    });
    //sanity check, make sure all the transactions can execute sequentially
    transactions.iter().for_each(|tx| {
        let res = bank.process_transaction(&tx);
        assert!(res.is_ok(), "sanity test transactions");
    });
    bank.clear_signatures();
    //sanity check, make sure all the transactions can execute in parallel
    let res = bank.process_transactions(&transactions);
    for r in res {
        assert!(r.is_ok(), "sanity parallel execution");
    }
    bank.clear_signatures();
    let verified: Vec<_> = to_packets_chunked(&transactions.clone(), 192)
        .into_iter()
        .map(|x| {
            let len = x.packets.len();
            (x, iter::repeat(1).take(len).collect())
        })
        .collect();
    let ledger_path = get_tmp_ledger_path!();
    {
        let blocktree = Arc::new(
            Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger"),
        );
        let (exit, poh_recorder, poh_service, signal_receiver) =
            create_test_recorder(&bank, &blocktree);
        let cluster_info = ClusterInfo::new_with_invalid_keypair(Node::new_localhost().info);
        let cluster_info = Arc::new(RwLock::new(cluster_info));
        let _banking_stage = BankingStage::new(
            &cluster_info,
            &poh_recorder,
            verified_receiver,
            vote_receiver,
            &leader_schedule_cache,
        );
        poh_recorder.lock().unwrap().set_bank(&bank);

        let mut id = genesis_block.hash();
        for _ in 0..(MAX_RECENT_BLOCKHASHES * DEFAULT_TICKS_PER_SLOT as usize) {
            id = hash(&id.as_ref());
            bank.register_tick(&id);
        }

        let half_len = verified.len() / 2;
        let mut start = 0;
        bencher.iter(move || {
            // make sure the transactions are still valid
            bank.register_tick(&genesis_block.hash());
            for v in verified[start..start + half_len].chunks(verified.len() / num_threads) {
                verified_sender.send(v.to_vec()).unwrap();
            }
            check_txs(&signal_receiver, txes / 2);
            bank.clear_signatures();
            start += half_len;
            start %= verified.len();
        });
        drop(vote_sender);
        exit.store(true, Ordering::Relaxed);
        poh_service.join().unwrap();
    }
    Blocktree::destroy(&ledger_path).unwrap();
}

#[bench]
#[ignore]
fn bench_banking_stage_multi_programs(bencher: &mut Bencher) {
    let progs = 4;
    let num_threads = BankingStage::num_threads() as usize;
    //   a multiple of packet chunk  2X duplicates to avoid races
    let txes = 96 * 100 * num_threads * 2;
    let mint_total = 1_000_000_000_000;
    let (genesis_block, mint_keypair) = GenesisBlock::new(mint_total);

    let (verified_sender, verified_receiver) = channel();
    let (vote_sender, vote_receiver) = channel();
    let bank = Arc::new(Bank::new(&genesis_block));
    let leader_schedule_cache = Arc::new(LeaderScheduleCache::new_from_bank(&bank));
    let dummy = system_transaction::transfer(
        &mint_keypair,
        &mint_keypair.pubkey(),
        1,
        genesis_block.hash(),
        0,
    );
    let transactions: Vec<_> = (0..txes)
        .into_par_iter()
        .map(|_| {
            let mut new = dummy.clone();
            let from: Vec<u8> = (0..32).map(|_| thread_rng().gen()).collect();
            let sig: Vec<u8> = (0..64).map(|_| thread_rng().gen()).collect();
            let to: Vec<u8> = (0..32).map(|_| thread_rng().gen()).collect();
            new.message.account_keys[0] = Pubkey::new(&from[0..32]);
            new.message.account_keys[1] = Pubkey::new(&to[0..32]);
            let prog = new.message.instructions[0].clone();
            for i in 1..progs {
                //generate programs that spend to random keys
                let to: Vec<u8> = (0..32).map(|_| thread_rng().gen()).collect();
                let to_key = Pubkey::new(&to[0..32]);
                new.message.account_keys.push(to_key);
                assert_eq!(new.message.account_keys.len(), i + 2);
                new.message.instructions.push(prog.clone());
                assert_eq!(new.message.instructions.len(), i + 1);
                new.message.instructions[i].accounts[1] = 1 + i as u8;
                assert_eq!(new.key(i, 1), Some(&to_key));
                assert_eq!(
                    new.message.account_keys[new.message.instructions[i].accounts[1] as usize],
                    to_key
                );
            }
            assert_eq!(new.message.instructions.len(), progs);
            new.signatures = vec![Signature::new(&sig[0..64])];
            new
        })
        .collect();
    transactions.iter().for_each(|tx| {
        let fund = system_transaction::transfer(
            &mint_keypair,
            &tx.message.account_keys[0],
            mint_total / txes as u64,
            genesis_block.hash(),
            0,
        );
        bank.process_transaction(&fund).unwrap();
    });
    //sanity check, make sure all the transactions can execute sequentially
    transactions.iter().for_each(|tx| {
        let res = bank.process_transaction(&tx);
        assert!(res.is_ok(), "sanity test transactions");
    });
    bank.clear_signatures();
    //sanity check, make sure all the transactions can execute in parallel
    let res = bank.process_transactions(&transactions);
    for r in res {
        assert!(r.is_ok(), "sanity parallel execution");
    }
    bank.clear_signatures();
    let verified: Vec<_> = to_packets_chunked(&transactions.clone(), 96)
        .into_iter()
        .map(|x| {
            let len = x.packets.len();
            (x, iter::repeat(1).take(len).collect())
        })
        .collect();

    let ledger_path = get_tmp_ledger_path!();
    {
        let blocktree = Arc::new(
            Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger"),
        );
        let (exit, poh_recorder, poh_service, signal_receiver) =
            create_test_recorder(&bank, &blocktree);
        let cluster_info = ClusterInfo::new_with_invalid_keypair(Node::new_localhost().info);
        let cluster_info = Arc::new(RwLock::new(cluster_info));
        let _banking_stage = BankingStage::new(
            &cluster_info,
            &poh_recorder,
            verified_receiver,
            vote_receiver,
            &leader_schedule_cache,
        );
        poh_recorder.lock().unwrap().set_bank(&bank);

        let mut id = genesis_block.hash();
        for _ in 0..(MAX_RECENT_BLOCKHASHES * DEFAULT_TICKS_PER_SLOT as usize) {
            id = hash(&id.as_ref());
            bank.register_tick(&id);
        }

        let half_len = verified.len() / 2;
        let mut start = 0;
        bencher.iter(move || {
            // make sure the transactions are still valid
            bank.register_tick(&genesis_block.hash());
            for v in verified[start..start + half_len].chunks(verified.len() / num_threads) {
                verified_sender.send(v.to_vec()).unwrap();
            }
            check_txs(&signal_receiver, txes / 2);
            bank.clear_signatures();
            start += half_len;
            start %= verified.len();
        });
        drop(vote_sender);
        exit.store(true, Ordering::Relaxed);
        poh_service.join().unwrap();
    }
    Blocktree::destroy(&ledger_path).unwrap();
}
