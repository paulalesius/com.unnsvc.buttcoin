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
        walletin_id -> Integer,
        walletout_id -> Integer,
        balance -> BigInt,
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

allow_tables_to_appear_in_same_query!(
    transactions,
    txouts,
    wallet,
);
