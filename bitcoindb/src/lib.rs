use leveldb::options::{Options, WriteOptions, ReadOptions};
use leveldb::db::Database;
use leveldb::error::Error;
use leveldb::util::FromU8;
use leveldb::iterator::Iterable;
use std::path::Path;

/***
struct BitcoindDB {

}

impl BitcoinDB {

}
**/

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let path = Path::new("/home/noname/.bitcoin");
        let mut options = Options::new();
        options.create_if_missing = false;
        let database = Database::open(&path, &options);
        

        //BitcoinDB::new();
    }
}
