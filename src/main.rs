#[macro_use]
extern crate diesel;

mod db;
use db::*;

use bitcoin;
use bitcoin::blockdata::script::Instruction;
use bitcoincore_rpc::{Auth, Client, RpcApi};
use dotenv::dotenv;
use log::{error, info, warn};
use log4rs;
use std::env;

struct LocalBlock {
    hash: String,
    version: i32,
    height: u64,
    transactions: usize,
    pub satoshis: i64,
}

impl LocalBlock {
    fn new(hash: String, version: i32, height: u64, transactions: usize, satoshis: i64) -> Self {
        LocalBlock {
            hash,
            version,
            height,
            transactions,
            satoshis,
        }
    }
}

impl std::fmt::Display for LocalBlock {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        let out = format!(
            "Block({} ver={} hei={} txns={} satoshis={})",
            self.hash.as_str(),
            self.version,
            self.height,
            self.transactions,
            self.satoshis
        );
        fmt.write_str(&out)?;
        Ok(())
    }
}

enum LocalErr {
    BlockDecodingFailed(LocalBlock, bitcoin::Transaction),
}

fn main() {
    log4rs::init_file("log4rs.yaml", Default::default()).unwrap();

    dotenv().ok();

    let auth = Auth::UserPass(
        env::var("BITCOINRPC_USER").unwrap().to_string(),
        env::var("BITCOINRPC_PASS").unwrap().to_string(),
    );
    let cl = Client::new(env::var("BITCOINRPC_URL").unwrap().to_string(), auth).unwrap();
    let blocks = cl.get_blockchain_info().unwrap().blocks;
    let db = Database::new();

    for i in 0..=blocks {
        match on_block(&db, &cl, i) {
            Ok(local_block) => {
                info!("Processed: {}", local_block);
            }
            Err(local_err) => match local_err {
                LocalErr::BlockDecodingFailed(block, tx) => {
                    info!("Failed: {}", block);
                    info!("           Tx({:?})", tx);
                }
            },
        }
    }
}

fn on_block(
    db: &Database,
    cl: &bitcoincore_rpc::Client,
    block_height: u64,
) -> Result<LocalBlock, LocalErr> {
    let block_hash = cl.get_block_hash(block_height).unwrap();
    let block = cl.get_block(&block_hash).unwrap();
    let block_version = block.header.version;
    let block_time = block.header.time;

    let mut lb: LocalBlock = LocalBlock::new(
        block_hash.to_string(),
        block_version,
        block_height,
        block.txdata.len(),
        0,
    );
    for tx in block.txdata {
        match on_transaction(&db, &tx, block_time) {
            Ok(transferred) => lb.satoshis += transferred,
            Err(_) => {
                // Failed to decode
                return Err(LocalErr::BlockDecodingFailed(lb, tx));
            }
        }
    }

    return Ok(lb);
}

fn on_transaction(db: &Database, tx: &bitcoin::Transaction, block_time: u32) -> Result<i64, ()> {
    let txid = tx.txid().to_string();

    if !tx.is_coin_base() {
        for input in &tx.input {
            let outpoint = input.previous_output;
            let prev_out = db
                .get_txout_by_txn(&outpoint.txid.to_string(), outpoint.vout as i32)
                .unwrap();
            let wal = db.find_wallet(prev_out.wallet_id).unwrap();
            db.update_wallet(&wal, 0);
        }
    }

    match db.get_transaction(&txid) {
        Some(_) => {}
        None => {
            db.insert_transaction(&txid, block_time as i32);
        }
    }

    let mut transferred: i64 = 0;
    for (vout, output) in tx.output.iter().enumerate() {
        let address = script_to_address(&output.script_pubkey)?;
        // Now persist to database
        let txnout = match db.get_txout_by_txn(&txid, vout as i32) {
            Some(txnout) => txnout,
            None => {
                // Doesn't exist, insert
                db.insert_vout(&txid, vout as i32, &address.to_string())
                    .unwrap()
            }
        };
        let bal = output.value;
        let wal = db.find_wallet(txnout.wallet_id).unwrap();
        db.update_wallet(&wal, bal as i64);
        transferred += bal as i64;
    }

    Ok(transferred)
}

/***
 * The modern BIP68 variant
 */
fn script_to_address(script: &bitcoin::Script) -> Result<bitcoin::Address, ()> {
    match bitcoin::Address::from_script(script, bitcoin::Network::Bitcoin) {
        Some(address) => {
            //println!("BIP68 WORKED");
            Ok(address)
        }
        None => {
            return script_to_address_v1(script);
        }
    }
}

/***
 * The old variant that hashes the public key in the operations
 */
fn script_to_address_v1(script: &bitcoin::Script) -> Result<bitcoin::Address, ()> {
    let pubsig: Option<&[u8]> = script
        .instructions()
        .find_map(|instr| match instr.unwrap() {
            Instruction::PushBytes(bytes) => {
                if bytes.len() == 65 {
                    return Some(bytes);
                }
                return None;
            }
            _ => {
                return None;
            }
        });

    match pubsig {
        Some(address) => {
            let pk = bitcoin::PublicKey::from_slice(address).unwrap();
            let addr = bitcoin::Address::p2pkh(&pk, bitcoin::Network::Bitcoin);
            return Ok(addr);
        }
        None => {
            for instr in script.instructions() {
                match instr.unwrap() {
                    Instruction::Op(op) => {
                        info!("{}", op);
                    }
                    _ => {}
                }
            }
            return Err(());
        }
    }
}
