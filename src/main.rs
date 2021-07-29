#![feature(hash_set_entry)]
use bitcoin;
use bitcoin::blockdata::script::Instruction;
use bitcoincore_rpc::{Auth, Client, RpcApi};
use dotenv::dotenv;
use log::{info, error};
use log4rs;
use serde::{Deserialize, Serialize};
use std::{
    env,
    sync::atomic::{AtomicUsize, Ordering},
    sync::{Arc, RwLock},
    time::{Duration, Instant},
    fs::File,
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

#[derive(Serialize, Deserialize)]
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
    let nr_processed_blocks = Arc::new(AtomicUsize::new(0));
    let ctx = Arc::new(Context::new(total_blocks, 10000, 10));

    pool.scope(|scope| {
        with_scope(scope, &cl, &blocknums, &nr_processed_blocks, ctx);
    });
}

struct Context {
    // This is global processed blocks for all thread executions.
    // It will be flushed by individual threads once it fills up.
    total_blocks: u64,
    processed_blocks: Arc<RwLock<Vec<Block>>>,
    chunk_nr: Arc<AtomicUsize>,
    flush_threshold: u64,
    chunk_size: u64,
}
impl Context {
    fn new(total_blocks: u64, flush_threshold: u64, chunk_size: u64) -> Self {
        Context {
            total_blocks,
            processed_blocks: Arc::new(RwLock::new(Vec::new())),
            chunk_nr: Arc::new(AtomicUsize::new(0)),
            flush_threshold,
            chunk_size,
        }
    }

    fn get_total_blocks(&self) -> u64 {
        self.total_blocks
    }

    fn get_chunk_size(&self) -> u64 {
        self.chunk_size
    }

    fn add_blocks_and_flush(&self, processed_blocks: Vec<Block>) -> Option<Segment> {

        let mut flush : Option<Vec<Block>> = None;

        // Hold the lock for as briefly as possible, only to either add blocks to the global or produce a flush vector.
        match self.processed_blocks.write() {
            Ok(ref mut processed_blocks_global) => {
                processed_blocks_global.extend(processed_blocks);

                // If there's more than the flush threshold, then produce a Segment and drain global blocks
                let nr_processed = processed_blocks_global.len() as u64;
                if nr_processed > self.flush_threshold || nr_processed == self.total_blocks {
                    flush = Some(processed_blocks_global.drain(0..).collect::<Vec<Block>>());
                }
            }
            Err(_) => {}
        };

        if let Some(flush) = flush {
            let chunknr = self.chunk_nr.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |v| Some(v + 1)).unwrap();
            return Some(Segment{id: chunknr, blocks: flush});
        }

        None
    }
}

#[derive(Serialize, Deserialize)]
struct Segment {
    id: usize,
    blocks: Vec<Block>
}

fn with_scope<'a>(
    scope: &rayon::Scope<'a>,
    cl: &'a Client,
    blocknums: &'a Vec<u64>,
    nr_processed_blocks: &'a Arc<AtomicUsize>,
    ctx: Arc<Context>
) {

    blocknums.chunks(ctx.get_chunk_size() as usize).for_each(|chunk| {
        
        let ctx = ctx.clone();

        scope.spawn(move |_| {
            /***
             * Fetch chunk
             */
            let mut bitcoin_blocks: Vec<bitcoin::Block> = Vec::new();
            let mut processed_transactions = 0;
            // This is local to every thread execution
            let mut processed_blocks_local: Vec<Block> = Vec::new();

            let start_fetch = Instant::now();
            chunk.iter().for_each(|blocknum| {
                let hash = cl.get_block_hash(blocknum.to_owned()).unwrap();
                let block = cl.get_block(&hash).unwrap();
                bitcoin_blocks.push(block);
            });
            let end_fetch = Instant::now().duration_since(start_fetch);

            /***
             * Process chunk
             **/
            let start_process = Instant::now();
            bitcoin_blocks.iter().for_each(|block| {
                let block = on_block(block);
                processed_transactions += block.transactions.len();
                processed_blocks_local.push(block);
            });
            let end_process = Instant::now().duration_since(start_process);

            /***
             * Now store if necessary, but acquire write lock briefly and perform the flush later so
             * we don't hold the lock.
             **/
            let start_flush = Instant::now();
            match ctx.add_blocks_and_flush(processed_blocks_local) {
                Some(segment) => {
                    let file = File::create(format!("target/segment-{}.dat", segment.id)).expect("Failed to create file");
                    bincode::serialize_into(file, &segment).expect("Failed to serialize");
                }
                None => {}
            }

            let end_flush = Instant::now().duration_since(start_flush);

            nr_processed_blocks.fetch_add(chunk.len(), Ordering::SeqCst);
            info!(
                "Processed blocks {}/{}; Transactions: {}; Fetch: {}ms; Process: {}ms; Flush: {}ms",
                nr_processed_blocks.load(Ordering::SeqCst),
                ctx.get_total_blocks(),
                processed_transactions,
                end_fetch.as_millis(),
                end_process.as_millis(),
                end_flush.as_millis(),
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
