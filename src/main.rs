#![feature(hash_set_entry)]
use bitcoin;
use bitcoin::blockdata::script::Instruction;
use bitcoincore_rpc::{Auth, Client, RpcApi};
use dotenv::dotenv;
use hashbrown::HashSet;
use log::{error, info};
use log4rs;
use std::rc::Rc;
use std::sync::{Arc, RwLock};
use std::{env, sync::RwLockWriteGuard};
use serde::{Serialize, Deserialize};

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

struct Vout {
    address: String,
    satoshi: u64,
}
impl Vout {
    fn new(address: String, satoshi: u64) -> Self {
        Vout { address, satoshi }
    }
}

#[derive(Eq, PartialEq, Serialize, Deserialize)]
struct Transaction {
    id: String,
    outs: Vec<(Arc<Wallet>, u64)>,
}
impl Transaction {
    fn new(id: String) -> Self {
        Transaction {
            id,
            outs: Vec::new(),
        }
    }

    fn add_vout(&mut self, wallet: Arc<Wallet>, value: u64) {
        self.outs.push((wallet, value));
    }
}
impl std::hash::Hash for Transaction {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        state.finish();
    }
}

impl From<String> for Wallet {
    fn from(value: String) -> Self {
        Wallet::new(value)
    }
}

#[derive(Serialize, Deserialize)]
struct Context {
    transactions: HashSet<Transaction>,
    wallets: HashSet<Arc<Wallet>>,
}
impl Context {
    fn new() -> Self {
        Context {
            transactions: HashSet::new(),
            wallets: HashSet::new(),
        }
    }

    fn add_transaction_vouts(&mut self, txid: String, vouts: Vec<Vout>) {

        let mut txn = Transaction::new(txid);
        for vout in vouts {

            let wallet: &Arc<Wallet> = self.wallets.get_or_insert(Arc::new(Wallet::new(vout.address.clone())));
            txn.add_vout(wallet.clone(), vout.satoshi);
        }

        self.transactions.insert(txn);
    }
}

fn main() {
    log4rs::init_file("log4rs.yaml", Default::default()).unwrap();
    dotenv().ok();

    let user = env::var("BITCOINRPC_USER").unwrap().to_string();
    let pass = env::var("BITCOINRPC_PASS").unwrap().to_string();
    let url = env::var("BITCOINRPC_URL").unwrap().to_string();

    let auth = Auth::UserPass(user, pass);
    let cl = Client::new(url, auth).unwrap();
    let blocks = cl.get_blockchain_info().unwrap().blocks;
    let ctx = Arc::new(RwLock::new(Context::new()));

    let pool = &rayon::ThreadPoolBuilder::new()
        .num_threads(8)
        .build()
        .unwrap();
    with_scope(pool, blocks, ctx, &cl);
}

fn with_scope(pool: &rayon::ThreadPool, blocks: u64, ctx: Arc<RwLock<Context>>, cl: &Client) {
    pool.scope(|s| {
        (0..blocks).for_each(|blocknum| {
            let ctx = ctx.clone();

            s.spawn(move |s1| {
                let hash = cl.get_block_hash(blocknum).unwrap();
                let block = cl.get_block(&hash).unwrap();

                on_block(s1, block, ctx);

                if (blocknum % 100) == 0 {
                    info!("Processed block {}", blocknum);
                }
            });
        });
    });


    // Now persist the data
    let file = std::fs::File::create("target/ctx.dat").expect("Expected file");
    bincode::serialize_into(file, &ctx).expect("Expected serialization");
}

fn on_block<'a>(scope: &rayon::Scope<'a>, block: bitcoin::Block, ctx: Arc<RwLock<Context>>) {
    let txdata = block.txdata;
    for tx in txdata {
        let ctx = ctx.clone();
        scope.spawn(|s1| {
            on_transaction(s1, tx, ctx);
        })
    }
}

fn on_transaction<'a>(
    scope: &rayon::Scope<'a>,
    tx: bitcoin::Transaction,
    ctx: Arc<RwLock<Context>>,
) {

    let mut vouts: Vec<Vout> = Vec::new();

    for (vout, output) in tx.output.iter().enumerate() {
        
        let ctx = ctx.clone();
        let txid = tx.txid().to_string();

        match script_to_p2sh(&output.script_pubkey) {
            Ok(address) => {
                let vout = Vout::new(address, output.value);
                vouts.push(vout);
                //ctx.write().unwrap().add_transaction_vouts(txid, vout);
            }
            Err(e) => {
                error!(
                    "Script is not a valid address in transaction: {}; {}",
                    txid,
                    e
                );
            }
        }
    }

    ctx.write()
        .unwrap()
        .add_transaction_vouts(tx.txid().to_string(), vouts);
}

fn script_to_p2sh(script: &bitcoin::Script) -> Result<String, String> {
    match bitcoin::Address::from_script(script, bitcoin::Network::Bitcoin) {
        Some(address) => Ok(address.to_string()),
        None => {
            // @TODO Attempt to parse the script manually
            script_to_v0(script)
        }
    }
}

fn script_to_v0(script: &bitcoin::Script) -> Result<String, String> {
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
}

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
