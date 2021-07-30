use super::schema::transactions;
use super::schema::txouts;
use super::schema::wallet;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Queryable, Serialize, Associations, Identifiable)]
#[table_name = "transactions"]
pub struct Transactions {
    pub id: i32,
    pub txid: String,
    pub minedtime: i32,
}

#[derive(Insertable)]
#[table_name = "transactions"]
pub struct NewTransaction<'a> {
    pub txid: &'a String,
    pub minedtime: i32,
}

#[derive(PartialEq, Debug, Deserialize, Queryable, Serialize, Associations, Identifiable)]
#[table_name = "txouts"]
pub struct Txouts {
    pub id: i32,
    pub transactions_id: i32,
    pub vout: i32,
    pub walletin_id: i32,
    pub walletout_id: i32,
    pub balance: i64,
}

#[derive(Insertable)]
#[table_name = "txouts"]
pub struct NewTxouts {
    pub transactions_id: i32,
    pub vout: i32,
    pub walletin_id: i32,
    pub walletout_id: i32,
    pub balance: i64,
}

#[derive(PartialEq, Debug, Deserialize, Queryable, Serialize, Associations, Identifiable)]
#[table_name = "wallet"]
pub struct Wallet {
    pub id: i32,
    pub waddress: String,
    pub balance: i64,
}

#[derive(Insertable)]
#[table_name = "wallet"]
pub struct NewWallet<'a> {
    pub waddress: &'a String,
    pub balance: i64,
}
