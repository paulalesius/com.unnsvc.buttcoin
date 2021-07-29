#![feature(hash_set_entry)]
use bitcoin;
use bitcoin::blockdata::script::Instruction;
use bitcoincore_rpc::{Auth, Client, RpcApi};
use dotenv::dotenv;
use log::info;
use log4rs;
use serde::{Deserialize, Serialize};
use std::{
    env,
    sync::atomic::{AtomicUsize, Ordering},
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Eq, PartialEq, Serialize, Deserialize)]
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
        state.finish();
    }
}

#[derive(Eq, PartialEq, Serialize, Deserialize)]
enum Vout {
    // Address, satoshis
    VALID(String, u64),
    INVALID,
}

#[derive(Eq, PartialEq, Serialize, Deserialize)]
struct Transaction {
    id: String,
    vouts: Vec<Vout>,
}
impl Transaction {
    fn new(id: String) -> Self {
        Transaction {
            id,
            vouts: Vec::new(),
        }
    }

    fn add_vout(&mut self, vout: Vout) {
        self.vouts.push(vout);
    }
}
impl std::hash::Hash for Transaction {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        state.finish();
    }
}

struct Block {
    id: String,
    transactions: Vec<Transaction>,
}

impl Block {
    fn new(id: String) -> Self {
        Block {
            id,
            transactions: Vec::new(),
        }
    }

    fn add_transaction(&mut self, transaction: Transaction) {
        self.transactions.push(transaction);
    }
}

impl From<String> for Wallet {
    fn from(value: String) -> Self {
        Wallet::new(value)
    }
}

fn main() {
    log4rs::init_file("log4rs.yaml", Default::default()).unwrap();
    dotenv().ok();

    let user = env::var("BITCOINRPC_USER").unwrap().to_string();
    let pass = env::var("BITCOINRPC_PASS").unwrap().to_string();
    let url = env::var("BITCOINRPC_URL").unwrap().to_string();
    let pool = &rayon::ThreadPoolBuilder::new()
        .num_threads(8)
        .build()
        .unwrap();

    let auth = Auth::UserPass(user, pass);
    let cl = Client::new(url, auth).unwrap();
    let total_blocks = cl.get_blockchain_info().unwrap().blocks;
    let blocknums = (0..total_blocks).collect::<Vec<u64>>();
    let processed = Arc::new(AtomicUsize::new(0));

    pool.scope(|scope| {
        with_scope(scope, total_blocks, &cl, &blocknums, &processed);
    });
}

fn with_scope<'a>(
    scope: &rayon::Scope<'a>,
    blocks: u64,
    cl: &'a Client,
    blocknums: &'a Vec<u64>,
    processed: &'a Arc<AtomicUsize>,
) {
    blocknums.chunks(10000).for_each(|chunk| {
        scope.spawn(move |_| {
            // For each chunk
            let mut bitcoin_blocks: Vec<bitcoin::Block> = Vec::new();
            let mut processed_transactions = 0;

            let start_fetch = Instant::now();
            chunk.iter().for_each(|blocknum| {
                let hash = cl.get_block_hash(blocknum.to_owned()).unwrap();
                let block = cl.get_block(&hash).unwrap();
                bitcoin_blocks.push(block);
            });
            let end_fetch = Instant::now().duration_since(start_fetch);

            // Process
            let start_process = Instant::now();
            bitcoin_blocks.iter().for_each(|block| {
                let block = on_block(block);
                processed_transactions += block.transactions.len();
                // @TODO do something with block
            });
            let end_process = Instant::now().duration_since(start_process);

            processed.fetch_add(chunk.len(), Ordering::SeqCst);
            info!(
                "Processed {}/{} blocks {} transactions. Fetch: {}ms Process: {}ms",
                processed.load(Ordering::SeqCst),
                blocks,
                processed_transactions,
                end_fetch.as_millis(),
                end_process.as_millis()
            );
        });
    });
}

fn on_block<'a>(block: &bitcoin::Block) -> Block {
    let mut block_result = Block::new(block.block_hash().to_string());
    let txdata = &block.txdata;
    for tx in txdata {
        let transaction = on_transaction(tx);
        block_result.add_transaction(transaction);
    }

    block_result
}

fn on_transaction(tx: &bitcoin::Transaction) -> Transaction {
    let txid = tx.txid().to_string();
    let mut transaction = Transaction::new(txid);

    for output in tx.output.iter() {
        match script_to_p2sh(&output.script_pubkey) {
            Ok(address) => {
                //info!("Processed wallet addr: {}", address);
                let vout = Vout::VALID(address, output.value);
                transaction.add_vout(vout);
            }
            Err(_) => {
                transaction.add_vout(Vout::INVALID);
            }
        }
    }

    transaction
}

fn script_to_p2sh(script: &bitcoin::Script) -> Result<String, String> {
    match bitcoin::Address::from_script(script, bitcoin::Network::Bitcoin) {
        Some(address) => Ok(address.to_string()),
        None => {
            // @TODO Attempt to parse the script manually
            //script_to_v0(script)
            if script.is_p2pk() {
                return script_to_p2pk(script);
            }
            return Err("Not a p2pk script".to_string());
        }
    }
}

/***fn script_to_v0(script: &bitcoin::Script) -> Result<String, String> {
    if script.is_p2pk() {
        return script_to_p2pk(script);
    } else {
        let is_p2pk = script.is_p2pk();
        let is_p2pkh = script.is_p2pkh();
        let is_p2sh = script.is_p2sh();
        let is_v0_p2wpkh = script.is_v0_p2wpkh();
        let is_v0_p2wsh = script.is_v0_p2wsh();
        return Err(format!(
            "Failed to process script: {} {} {} {} {}",
            is_p2pk, is_p2pkh, is_p2sh, is_v0_p2wpkh, is_v0_p2wsh
        ));
    }
}**/

fn script_to_p2pk(script: &bitcoin::Script) -> Result<String, String> {
    let pubsig: Option<&[u8]> = script
        .instructions()
        .find_map(|instr| match instr.unwrap() {
            Instruction::PushBytes(bytes) => {
                return Some(bytes);
            }
            _ => {
                return None;
            }
        });

    match pubsig {
        Some(pub_sig) => match bitcoin::PublicKey::from_slice(pub_sig) {
            Ok(pubkey) => {
                let addr = bitcoin::Address::p2pkh(&pubkey, bitcoin::Network::Bitcoin);
                return Ok(addr.to_string());
            }
            Err(e) => {
                return Err(format!("Failed to parse pubkey: {}", e));
            }
        },
        None => {
            return Err(format!("Failed to process script, none known processing."));
        }
    }
}
