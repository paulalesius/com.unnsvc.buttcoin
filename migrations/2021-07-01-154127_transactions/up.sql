-- Your SQL goes here
CREATE TABLE transactions (
  id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
  txid VARCHAR(64) NOT NULL,
  minedtime INTEGER NOT NULL
);

/***
CREATE TABLE txinout (
  id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
  transaction_id INTEGER NOT NULL,
  inout INTEGER NOT NULL,
  wallet_id INTEGER NOT NULL,
  satoshis BIGINT NOT NULL,
  FOREIGN KEY(transaction_id) REFERENCES transactions(id),
  FOREIGN KEY(wallet_id) REFERENCES wallet(id),
);**/

CREATE TABLE txouts (
  id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
  transactions_id INTEGER NOT NULL,
  vout INTEGER NOT NULL,
  walletin_id INTEGER NOT NULL,
  walletout_id INTEGER NOT NULL,
  balance BIGINT NOT NULL,
  FOREIGN KEY(transactions_id) REFERENCES transactions(id),
  FOREIGN KEY(walletin_id) REFERENCES wallet(id),
  FOREIGN KEY(walletout_id) REFERENCES wallet(id)
);

CREATE TABLE wallet (
  id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
  waddress VARCHAR NOT NULL,
  balance BIGINT NOT NULL
);
