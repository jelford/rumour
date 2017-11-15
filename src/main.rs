
#[macro_use]
extern crate error_chain;
#[macro_use]
extern crate serde_derive;
extern crate bincode;
#[macro_use]
extern crate clap;

extern crate futures;
extern crate rand;
extern crate tokio_core;
extern crate tokio_timer;

use std::net::{SocketAddr, IpAddr};
use std::vec::Vec;

#[macro_use] mod fixed_list;
mod errors;
use errors::*;
mod node;

quick_main!(run);

fn get_port(arg_name: &str, args: &clap::ArgMatches) -> Result<u16> {
    args.value_of(arg_name).unwrap().parse().map_err(|_| {
        ErrorKind::BadPortSpecified.into()
    })
}

fn get_addresses(arg_name: &str, args: &clap::ArgMatches) -> Result<Vec<SocketAddr>> {
    let addr_list = args.value_of(arg_name);
    if addr_list.is_none() {
        println!("No addresses specified");
        return Ok(Vec::new());
    }
    let addr_list = addr_list.unwrap();

    let mut addresses = Vec::new();
    for addr in addr_list.split(",") {
        if let Ok(host) = addr.parse::<IpAddr>() {
            addresses.push(SocketAddr::new(host, 8123));
        } else {
            let addr: Result<SocketAddr> = addr.parse().map_err(|_| {
                ErrorKind::BadClusterJoinAddressSpecified.into()
            });
            addresses.push(addr?);
        }
    }

    Ok(addresses)

}

fn run() -> Result<()> {
    let cmd = clap_app!(rumour =>
        (version: "0.1")
        (author: "James Elford <james.p.elford@gmail.com>")
        (@arg PORT:
            -p --port +required +takes_value "Port to listen on for incoming cluster connections")
        (@arg PEER_ADDR: --peeraddress +takes_value "Remote port to attempt to join the cluster")
    ).get_matches();


    let listen_port = get_port("PORT", &cmd)?;
    let peer_addresses = get_addresses("PEER_ADDR", &cmd)?;

    println!("Specified port: {}", listen_port);
    println!("Peer addresses: {:?}", peer_addresses);
    
    node::server(node::Config::listen_local(listen_port, peer_addresses))?
        .serve()
}
