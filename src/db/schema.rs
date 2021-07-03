table! {
    transactions (id) {
        id -> Integer,
        txid -> Text,
        minedtime -> Integer,
    }
}

table! {
    txouts (id) {
        id -> Integer,
        transactions_id -> Integer,
        vout -> Integer,
        wallet_id -> Integer,
    }
}

table! {
    wallet (id) {
        id -> Integer,
        waddress -> Text,
        balance -> BigInt,
    }
}

joinable!(txouts -> transactions (transactions_id));
joinable!(txouts -> wallet (wallet_id));

allow_tables_to_appear_in_same_query!(transactions, txouts, wallet,);
