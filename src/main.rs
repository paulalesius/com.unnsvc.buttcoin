#![feature(hash_set_entry, unused_imports)]
use bitcoin;
use bitcoin::blockdata::script::Instruction;
use bitcoincore_rpc::{Auth, Client, RpcApi};
use dotenv::dotenv;
use hashbrown::HashSet;
use log::{error, info};
use log4rs;
use rayon::prelude::*;
use std::sync::{Arc, RwLock};
use std::{
    env,
    sync::{RwLockReadGuard, RwLockWriteGuard, LockResult},
};

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
        state.finish();
    }
}

#[derive(Eq, PartialEq)]
struct Transaction<'a> {
    id: String,
    outs: Vec<(&'a Wallet, u64)>,
}
impl<'a> Transaction<'a> {
    fn new(id: String) -> Self {
        Transaction {
            id,
            outs: Vec::new(),
        }
    }

    fn add_vout(&mut self, wallet: &'a Wallet, value: u64) {
        self.outs.push((wallet, value));
    }
}
impl<'a> std::hash::Hash for Transaction<'a> {
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

struct Context<'a> {
    cl: Client,
    transactions: HashSet<Transaction<'a>>,
    wallets: HashSet<Wallet>,
}
impl<'a> Context<'a> {
    fn new(cl: Client) -> Self {
        Context {
            cl,
            transactions: HashSet::new(),
            wallets: HashSet::new(),
        }
    }

    fn get_block_by_height(&self, height: u64) -> bitcoin::Block {
        let hash = self.cl.get_block_hash(height).unwrap();
        let block = self.cl.get_block(&hash).unwrap();
        return block;
    }

    fn get_wallet(&'a mut self, address: String) -> &'a Wallet {
        let wallet = Wallet::from(address);
        return self.wallets.get_or_insert(wallet);
    }

    fn add_wallet(&'a mut self, address: String) -> &'a Wallet {
        self.wallets.insert(Wallet::new(address.clone()));
        let existing= self.wallets.get(&Wallet::new(address)).unwrap();
        return existing;
    }

    fn add_transaction(&mut self, tx: Transaction<'a>) {
        self.transactions.insert(tx);
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
    let ctx = Arc::new(RwLock::new(Context::new(cl)));

    let pool = rayon::ThreadPoolBuilder::new().num_threads(8).build().unwrap();
    pool.scope(|s: &rayon::Scope<'static>| {

        //let ctx = ctx.clone();
        (0..blocks).for_each(|blocknum| {

            let ctx = ctx.clone();
            let block = ctx.read().unwrap().get_block_by_height(blocknum);

            s.spawn( |s| {

                on_block(s, &block, ctx.clone());
            });
        });
    });

    /***(0..blocks).par_bridge().for_each(|blocknum| {
        let ctx = ctx.clone();
        let block = ctx.read().unwrap().get_block_by_height(blocknum);

        on_block(&block, ctx);

        info!(
            "Processed block: {} {}/{}",
            &block.block_hash().to_string(),
            blocknum,
            blocks
        );
    });**/
}

fn on_block<'a>(scope: &'a rayon::Scope<'a>, block: &'a bitcoin::Block, ctx: Arc<RwLock<Context<'a>>>) {
    /***
    block.txdata.par_iter().for_each(|tx| {
        let ctx = ctx.clone();
        on_transaction(tx, ctx);
    });**/

    for tx in &block.txdata {
        scope.spawn( |s1| {
            on_transaction(s1, tx, ctx.clone());
        })
    }
}

fn on_transaction<'a>(scope: &rayon::Scope, tx: &bitcoin::Transaction, ctx: Arc<RwLock<Context<'a>>>) {
    let mut transaction: Transaction = Transaction::new(tx.txid().to_string());

    for (vout, output) in tx.output.iter().enumerate() {

        match script_to_wallet(&output.script_pubkey, ctx.clone()) {
            Ok(wallet) => {

            }
            Err(e) => {
                error!(
                    "Script is not a valid address in transaction: {}; {}",
                    tx.txid().to_string(),
                    e
                );
            }
        }
    }

    ctx.write().unwrap().add_transaction(transaction);
}

fn script_to_wallet<'a>(script: &bitcoin::Script, ctx: Arc<RwLock<Context<'a>>>) -> Result<&'a Wallet, String> {
    let address = script_to_p2sh(script)?;
    let ctxread: RwLockWriteGuard<Context<'a>> = ctx.write().unwrap();
    let wallet = ctxread.get_wallet(address.clone());
    return Ok(wallet);
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
