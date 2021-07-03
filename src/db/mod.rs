pub mod models;
pub mod schema;

use diesel;
use diesel::prelude::*;
use diesel::result::Error;
use models::*;
use schema::transactions::dsl::*;
use schema::txouts;
use schema::txouts::dsl::*;
use schema::wallet;
use schema::wallet::dsl::*;

pub struct Database {
    conn: SqliteConnection,
}
impl Database {
    pub fn new() -> Self {
        let conn = SqliteConnection::establish(dotenv!("DATABASE_URL"))
            .unwrap_or_else(|_| panic!("Error connecting to target/database.db"));
        Database { conn }
    }

    pub fn get_transaction(&self, transid: &String) -> Option<Transactions> {
        let result = transactions
            .filter(txid.eq(transid))
            .limit(5)
            .get_result::<Transactions>(&self.conn);

        match result {
            Ok(trans) => Some(trans),
            Err(error) => match error {
                Error::NotFound => None,
                _ => panic!("Unknown error {}", error),
            },
        }
    }

    pub fn get_txout_by_txn(&self, transid: &String, txout_vout: i32) -> Option<Txouts> {
        let tx = self
            .get_transaction(&transid)
            .expect("Expected transaction to exist");

        let result = txouts
            .filter(
                txouts::transactions_id
                    .eq(tx.id)
                    .and(txouts::vout.eq(txout_vout)),
            )
            .get_result::<Txouts>(&self.conn);

        match result {
            Ok(tx_out) => Some(tx_out),
            Err(err) => match err {
                Error::NotFound => None,
                _ => panic!("Unknown error: {}", err),
            },
        }
    }

    pub fn get_wallet(&self, waladdr: &String) -> Option<Wallet> {
        let result = wallet
            .filter(wallet::waddress.eq(waladdr))
            .get_result::<Wallet>(&self.conn);

        match result {
            Ok(wal) => Some(wal),
            Err(err) => match err {
                Error::NotFound => None,
                _ => panic!("Unknown error: {}", err),
            },
        }
    }

    pub fn find_wallet(&self, walid: i32) -> Option<Wallet> {
        match wallet.find(walid).get_result::<Wallet>(&self.conn) {
            Ok(wal) => Some(wal),
            Err(err) => match err {
                Error::NotFound => None,
                _ => {
                    panic!("Unknown error trying to get wallet: {}", err)
                }
            },
        }
    }

    pub fn insert_wallet(&self, waladdr: &String) -> Wallet {
        let wal = NewWallet {
            waddress: waladdr,
            balance: 0,
        };
        diesel::insert_into(wallet)
            .values(&wal)
            .execute(&self.conn)
            .expect("Expected wallet insert");

        self.get_wallet(&waladdr)
            .expect("Expected wallet to exist not")
    }

    pub fn update_wallet(&self, wallid: &Wallet, wallbal: i64) {
        diesel::update(wallid)
            .set(wallet::balance.eq(wallbal))
            .execute(&self.conn)
            .expect("Expected update");
    }

    pub fn insert_vout(
        &self,
        transid: &String,
        txout_vout: i32,
        waladdr: &String,
    ) -> Option<Txouts> {
        let tx = self
            .get_transaction(transid)
            .expect("Expected transaction to exist");

        let wal: Wallet = match self.get_wallet(&waladdr) {
            Some(wal) => wal,
            None => {
                // Create and return wallet
                self.insert_wallet(&waladdr)
            }
        };

        let txo = NewTxouts {
            transactions_id: tx.id,
            vout: txout_vout,
            wallet_id: wal.id,
        };
        diesel::insert_into(txouts)
            .values(&txo)
            .execute(&self.conn)
            .expect("Expected insert vouts");

        self.get_txout_by_txn(transid, txout_vout)
    }

    pub fn insert_transaction(&self, transactiondid: &String, time: i32) -> Transactions {
        let tx = NewTransaction {
            txid: transactiondid,
            minedtime: time,
        };
        diesel::insert_into(transactions)
            .values(tx)
            .execute(&self.conn)
            .expect("Expected successful insert");
        return self
            .get_transaction(transactiondid)
            .expect("Expected successful query after insert");
    }
}
