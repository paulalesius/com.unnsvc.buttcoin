#![feature(hash_set_entry)]
//use bitcoin::blockdata::script::Instruction;
//use bitcoincore_rpc::{Auth, Client, RpcApi};
use bitcoincore_rpc as bitcoin;
use bitcoincore_rpc::RpcApi;
use dotenv::dotenv;
use hashbrown::HashSet;
use log::{error, info};
use log4rs;
use serde::{Deserialize, Serialize};
use std::{
    env,
    fs::File,
    io::{BufWriter, Write},
    sync::atomic::{AtomicU64, AtomicUsize, Ordering},
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

#[derive(Eq, PartialEq, Serialize, Deserialize)]
struct Wallet {
    id: u64,
    address: String,
}
impl Wallet {
    fn new(address: String) -> Self {
        Wallet { id: 0, address }
    }
}
// Don't hash the ID of the wallet, the address is a unique identifier.
impl std::hash::Hash for Wallet {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.address.hash(state);
        state.finish();
    }
}

#[derive(Eq, PartialEq, Serialize, Deserialize)]
enum Vout {
    // Address, satoshis
    VALID(u64, u64),
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

#[derive(Serialize, Deserialize)]
struct Segment {
    id: usize,
    blocks: Vec<Block>,
}

fn main() {
    log4rs::init_file("log4rs.yaml", Default::default()).unwrap();
    dotenv().ok();
    // Create dir to store data in
    std::fs::create_dir_all("target/data").unwrap();

    let user = env::var("BITCOINRPC_USER").unwrap().to_string();
    let pass = env::var("BITCOINRPC_PASS").unwrap().to_string();
    let url = env::var("BITCOINRPC_URL").unwrap().to_string();
    let pool = &rayon::ThreadPoolBuilder::new()
        .num_threads(8)
        .build()
        .unwrap();

    let auth = bitcoin::Auth::UserPass(user, pass);
    let cl = bitcoin::Client::new(url, auth).unwrap();
    let total_blocks = cl.get_blockchain_info().unwrap().blocks;
    let blocknums = (0..total_blocks).collect::<Vec<u64>>();
    let ctx = Arc::new(Context::new(total_blocks, 5000000, 1000));

    pool.scope(|scope| {
        with_scope(scope, &cl, &blocknums, ctx);
    });
}

struct Context {
    // This is global processed blocks for all thread executions.
    // It will be flushed by individual threads once it fills up.
    nr_total_blocks: u64,
    processed_blocks: Arc<RwLock<Vec<Block>>>,
    processed_transactions: Arc<AtomicU64>,
    chunk_nr: Arc<AtomicUsize>,
    // The number of blocks to process in a chunk
    chunk_size_in_blocks: u64,
    // Number of transactions to flush per segment, approximately
    segment_transactions_flush_threshold: u64,
    nr_blocks_processed: Arc<AtomicU64>,
    wallets: Arc<RwLock<HashSet<Wallet>>>,
    nr_total_wallets: Arc<AtomicU64>,
}
impl Context {
    fn new(total_blocks: u64, segment_transactions_flush_threshold: u64, chunk_size: u64) -> Self {
        Context {
            nr_total_blocks: total_blocks,
            processed_blocks: Arc::new(RwLock::new(Vec::new())),
            processed_transactions: Arc::new(AtomicU64::new(9)),
            chunk_nr: Arc::new(AtomicUsize::new(0)),
            chunk_size_in_blocks: chunk_size,
            segment_transactions_flush_threshold,
            nr_blocks_processed: Arc::new(AtomicU64::new(0)),
            wallets: Arc::new(RwLock::new(HashSet::new())),
            nr_total_wallets: Arc::new(AtomicU64::new(0)),
        }
    }

    fn get_total_blocks(&self) -> u64 {
        self.nr_total_blocks
    }

    fn get_nr_blocks_processed(&self) -> u64 {
        self.nr_blocks_processed.load(Ordering::SeqCst)
    }

    fn get_chunk_size(&self) -> u64 {
        self.chunk_size_in_blocks
    }

    fn get_id_for_wallet_address(&self, address: String) -> u64 {
        let mut new_or_existing_wallet = Wallet::new(address);

        let wallet = match self.wallets.read() {
            Ok(wallets) => match wallets.get(&new_or_existing_wallet) {
                Some(wallet) => Some(wallet.id),
                None => None,
            },
            Err(_) => None,
        };

        match wallet {
            // Wallet found, just return its ID
            Some(id) => return id,
            // No wallet found, create new wallet and store it.
            None => match self.wallets.write() {
                Ok(mut wallets) => {
                    let wallet_id = self
                        .nr_total_wallets
                        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |val| Some(val + 1))
                        .unwrap();
                    new_or_existing_wallet.id = wallet_id;
                    wallets.insert(new_or_existing_wallet);
                    wallet_id
                }
                Err(e) => panic!("{}", e),
            },
        }
    }

    fn add_blocks_and_flush(
        &self,
        processed_blocks: Vec<Block>,
        processed_transactions: u64,
    ) -> Option<Segment> {
        let mut flush: Option<Vec<Block>> = None;

        // Hold the lock for as briefly as possible, only to either add blocks to the global or produce a flush vector.
        match self.processed_blocks.write() {
            Ok(ref mut processed_blocks_global) => {
                let nr_processed_block_this_chunk = processed_blocks.len();
                processed_blocks_global.extend(processed_blocks);

                // If there's more than the flush threshold, then produce a Segment and drain global blocks
                let nr_blocks_processed = self
                    .nr_blocks_processed
                    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |val| {
                        Some(val + nr_processed_block_this_chunk as u64)
                    })
                    .unwrap();
                let nr_txns_processed = self
                    .processed_transactions
                    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |val| {
                        Some(val + processed_transactions)
                    })
                    .unwrap();

                // If segment transactions threshold is reached, or if total blocks is reached
                if nr_txns_processed >= self.segment_transactions_flush_threshold
                    || nr_blocks_processed == self.get_total_blocks()
                {
                    flush = Some(processed_blocks_global.drain(0..).collect::<Vec<Block>>());
                    self.processed_transactions.store(0, Ordering::SeqCst);
                }
            }
            Err(_) => {}
        };

        if let Some(flush) = flush {
            let chunknr = self
                .chunk_nr
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |v| Some(v + 1))
                .unwrap();
            return Some(Segment {
                id: chunknr,
                blocks: flush,
            });
        }

        None
    }
}

fn with_scope<'a>(
    scope: &rayon::Scope<'a>,
    cl: &'a bitcoin::Client,
    blocknums: &'a Vec<u64>,
    ctx: Arc<Context>,
) {
    blocknums
        .chunks(ctx.get_chunk_size() as usize)
        .for_each(|chunk| {
            let ctx = ctx.clone();

            scope.spawn(move |_| {
                /***
                 * Fetch chunk
                 */
                let mut bitcoin_blocks: Vec<bitcoincore_rpc::bitcoin::Block> = Vec::new();
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
                    let block = on_block(ctx.clone(), block);
                    processed_transactions += block.transactions.len();
                    processed_blocks_local.push(block);
                });
                let end_process = Instant::now().duration_since(start_process);

                /***
                 * Now store if necessary, but acquire write lock briefly and perform the flush later so
                 * we don't hold the lock.
                 **/
                let start_flush = Instant::now();
                match ctx
                    .add_blocks_and_flush(processed_blocks_local, processed_transactions as u64)
                {
                    Some(segment) => {
                        let file = File::create(format!("target/data/segment-{}.dat", segment.id))
                            .expect("Failed to create file");
                        let writer = BufWriter::new(file);
                        bincode::serialize_into(writer, &segment).expect("Failed to serialize");
                    }
                    None => {}
                }
                let end_flush = Instant::now().duration_since(start_flush);

                info!(
                "Processed blocks {}/{}; Transactions: {}; Fetch: {}ms; Process: {}ms; Flush: {}ms",
                ctx.get_nr_blocks_processed(),
                ctx.get_total_blocks(),
                processed_transactions,
                end_fetch.as_millis(),
                end_process.as_millis(),
                end_flush.as_millis(),
            );
            });
        });
}

fn on_block<'a>(ctx: Arc<Context>, block: &bitcoincore_rpc::bitcoin::Block) -> Block {
    let mut block_result = Block::new(block.block_hash().to_string());
    let txdata = &block.txdata;
    for tx in txdata {
        let transaction = on_transaction(ctx.clone(), tx);
        block_result.add_transaction(transaction);
    }

    block_result
}

fn on_transaction(ctx: Arc<Context>, tx: &bitcoincore_rpc::bitcoin::Transaction) -> Transaction {
    let txid = tx.txid().to_string();
    let mut transaction = Transaction::new(txid);

    for output in tx.output.iter() {
        match script_to_p2sh(&output.script_pubkey) {
            Ok(address) => {
                let id = ctx.get_id_for_wallet_address(address);
                let vout = Vout::VALID(id, output.value);
                transaction.add_vout(vout);
            }
            Err(_) => {
                transaction.add_vout(Vout::INVALID);
            }
        }
    }

    transaction
}

fn script_to_p2sh(script: &bitcoincore_rpc::bitcoin::Script) -> Result<String, String> {

    match bitcoin::bitcoin::util::address::Address::from_script(script, bitcoin::bitcoin::Network::Bitcoin) {
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

fn script_to_p2pk(script: &bitcoincore_rpc::bitcoin::Script) -> Result<String, String> {
    let pubsig: Option<&[u8]> = script
        .instructions()
        .find_map(|instr| match instr.unwrap() {
            bitcoin::bitcoin::blockdata::script::Instruction::PushBytes(bytes) => {
                return Some(bytes);
            }
            _ => {
                return None;
            }
        });

    match pubsig {
        Some(pub_sig) => match bitcoin::bitcoin::PublicKey::from_slice(pub_sig) {
            Ok(pubkey) => {
                let addr = bitcoin::bitcoin::util::address::Address::p2pkh(&pubkey, bitcoin::bitcoin::Network::Bitcoin);
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
