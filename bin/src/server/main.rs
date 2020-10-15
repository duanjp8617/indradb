#[macro_use]
extern crate failure;

mod errors;

use std::env;
use std::net::ToSocketAddrs;

use async_std::net::TcpListener;
use futures::executor::LocalPool;

const DEFAULT_PORT: u16 = 27615;

use std::net::IpAddr;
use dummy_cli_parser::{CliParser, PatternType};

struct ParseObj {
    addr: IpAddr,
    port: u16,
}

fn parse_cli() -> Result<ParseObj, String> {
    let mut parser = CliParser::new(ParseObj{
        addr: "127.0.0.1".parse::<IpAddr>().unwrap(),
        port: DEFAULT_PORT,
    });

    parser.register_pattern("-ip", PatternType::OptionalWithArg, "ip address", 
        |arg_str, parse_obj| {
            arg_str.parse::<IpAddr>().map(|addr|{
                parse_obj.addr = addr;
            }).map_err(|_|{
                String::from(format!("fail to parse argument \"{}\"", &arg_str))
            })
        }
    )?;

    parser.register_pattern("-p", PatternType::OptionalWithArg, "port", 
        |arg_str, parse_obj| {
            let parse_res = arg_str.parse::<u16>();
            if parse_res.is_ok() {                               
                parse_obj.port = parse_res.unwrap();
                Ok(())
            }
            else {
                Err(String::from(format!("fail to parse port number {}", &arg_str)))
            }
        }
    )?;

    parser.parse_env_args()
}

fn main() -> Result<(), errors::Error> {
    let mut exec = LocalPool::new();

    let po = parse_cli().unwrap();
    let addr = (po.addr.to_string() + ":" +  &po.port.to_string())
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| -> errors::Error { errors::Error::CouldNotParseBinding })?;
    let listener = exec.run_until(async { TcpListener::bind(&addr).await })?;
    println!("{}", listener.local_addr()?);

    let connection_string = env::var("DATABASE_URL").unwrap_or_else(|_| "memory://".to_string());

    if connection_string.starts_with("rocksdb://") {
        let path = &connection_string[10..connection_string.len()];

        let max_open_files_str = env::var("ROCKSDB_MAX_OPEN_FILES").unwrap_or_else(|_| "512".to_string());
        let max_open_files = max_open_files_str.parse::<i32>().expect(
            "Could not parse environment variable `ROCKSDB_MAX_OPEN_FILES`: must be an \
             i32",
        );

        let bulk_load_optimized = env::var("ROCKSDB_BULK_LOAD_OPTIMIZED").unwrap_or_else(|_| "".to_string()) == "true";

        let datastore = indradb::RocksdbDatastore::new(path, Some(max_open_files), bulk_load_optimized)
            .expect("Expected to be able to create the RocksDB datastore");

        exec.run_until(common::server::run(listener, datastore, exec.spawner()))?;
        Ok(())
    } else if connection_string.starts_with("sled://") {
        let path = &connection_string[7..connection_string.len()];
        let datastore = indradb::SledDatastore::new(path).expect("Expected to be able to create the Sled datastore");
        exec.run_until(common::server::run(listener, datastore, exec.spawner()))?;
        Ok(())
    } else if connection_string == "memory://" {
        println!("starting an in-memory db");
        let datastore = indradb::MemoryDatastore::default();
        exec.run_until(common::server::run(listener, datastore, exec.spawner()))?;
        Ok(())
    } else {
        Err(errors::Error::CouldNotParseDatabaseURL)
    }
}
