#[macro_use]
extern crate diesel;

mod db;
use db::*;

use bitcoin;
use bitcoin::blockdata::script::Instruction;
use bitcoincore_rpc::{Auth, Client, RpcApi};
use dotenv::dotenv;
use log::{error, info};
use log4rs;
use rayon::iter::{ParallelBridge, ParallelIterator};
use std::borrow::BorrowMut;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::env;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::RwLock;

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

fn main() {
    log4rs::init_file("log4rs.yml", Default::default()).unwrap();
    //dotenv().ok();

    let auth = Auth::UserPass(
        env::var("BITCOINRPC_USER").unwrap().to_string(),
        env::var("BITCOINRPC_PASS").unwrap().to_string(),
    );
    let cl = Arc::new(RwLock::new(
        Client::new(env::var("BITCOINRPC_URL").unwrap().to_string(), auth).unwrap(),
    ));
    let wallets: Arc<RwLock<HashSet<Wallet>>> = Arc::new(RwLock::new(HashSet::new()));
    let transactions: Arc<RwLock<HashSet<Transaction>>> = Arc::new(RwLock::new(HashSet::new()));
    let blocks = cl.read().unwrap().get_blockchain_info().unwrap().blocks;

    (0..=blocks).par_bridge().for_each(|blocknum| {
        let local_cl = cl.clone();
        let hash = local_cl.read().unwrap().get_block_hash(blocknum).unwrap();
        let block = local_cl.read().unwrap().get_block(&hash).unwrap();
        on_block(&block, wallets.clone(), transactions.clone());

        info!(
            "Processed block: {} {}/{}",
            &block.block_hash().to_string(),
            blocknum,
            blocks
        );
    });
}

#[derive(Eq, PartialEq)]
struct Wallet {
    id: String,
}
impl Wallet {
    fn new(id: String) -> Self {
        Wallet { id }
    }
}
impl std::hash::Hash for Wallet {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

#[derive(Eq, PartialEq)]
struct Transaction<'a> {
    id: String,
    ins: Vec<(&'a Wallet, u64)>,
    outs: Vec<(&'a Wallet, u64)>,
}
impl<'a> Transaction<'a> {
    fn new(id: String) -> Self {
        Transaction {
            id,
            ins: Vec::new(),
            outs: Vec::new(),
        }
    }
}
impl<'a> std::hash::Hash for Transaction<'a> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

fn on_block<'a>(
    block: &bitcoin::Block,
    wallets: Arc<RwLock<HashSet<Wallet>>>,
    transactions: Arc<RwLock<HashSet<Transaction<'a>>>>,
) {
    //let block_time = block.header.time;

    for tx in &block.txdata {
        on_transaction(&tx, wallets.clone(), transactions.clone());
    }
}

fn on_transaction<'a>(
    tx: &bitcoin::Transaction,
    wallets: Arc<RwLock<HashSet<Wallet>>>,
    transactions: Arc<RwLock<HashSet<Transaction<'a>>>>,
) {
    let txid = tx.txid().to_string();
    let mut transaction = Transaction::<'a>::new(txid.clone());

    if !tx.is_coin_base() {
        for input in &tx.input {
            let outpoint = input.previous_output;
            let last_txid = outpoint.txid.to_string();
            let last_tx = transactions
                .read()
                .unwrap()
                .get(&Transaction::new(last_txid))
                .expect("Expected a last transaction to exist but none was found, chain broken");

            let last_tx_wallet = last_tx.outs[outpoint.vout as usize].0;
            let last_tx_sum = last_tx.outs[outpoint.vout as usize].1;

            // Save it in the transaction as a txout sum
            transaction.ins.push((last_tx_wallet, last_tx_sum));
        }
    }

    for (vout, output) in tx.output.iter().enumerate() {
        match script_to_address(&output.script_pubkey) {
            Ok(address) => {
                let wallet_address = address.to_string();
                let wallet = wallets
                    .read()
                    .unwrap()
                    .get(&Wallet::new(wallet_address))
                    .unwrap();
                transaction.outs.push((wallet, output.value));
            }
            Err(err) => match err {
                AddressError::SignatureDecodingError => {
                    error!(
                        "Signature decoding error at transaction: {} vout: {}",
                        &txid, vout
                    );
                }
            },
        }
    }

    transactions.write().unwrap().insert(transaction);
}

enum AddressError {
    SignatureDecodingError,
}

/***
 * The modern BIP68 variant
 */
fn script_to_address(script: &bitcoin::Script) -> Result<bitcoin::Address, AddressError> {
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
fn script_to_address_v1(script: &bitcoin::Script) -> Result<bitcoin::Address, AddressError> {
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
            error!(
                "p2pk {} p2pkh {} p2sh {}",
                script.is_p2pk(),
                script.is_p2pkh(),
                script.is_p2sh()
            );
            /***
            for instr in script.instructions() {
                match instr.unwrap() {
                    Instruction::Op(op) => {
                        info!("op {}", op);
                    }
                    _ => {}
                }
            }**/
            Err(AddressError::SignatureDecodingError)
        }
    }
}
