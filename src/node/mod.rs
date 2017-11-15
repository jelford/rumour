
use std::vec::Vec;
use std::net::SocketAddr;
use std::time::Duration;
use std::sync::{Arc, Mutex};

use futures::{Stream, Sink, future, Future};
use futures::sync::mpsc::{self, Sender, Receiver};
use tokio_core::net::{UdpSocket};
use tokio_core::reactor::{Core, Handle};
use tokio_timer::Timer;

use errors::*;

mod mill;
use self::mill::*;
mod node_state;
mod gossip;

mod codec;
use self::codec::*;

pub struct Config {
    listen_port: SocketAddr,
    peer_addresses: Vec<SocketAddr>,
}

impl Config {
    pub fn listen_local(port: u16, peer_addresses: Vec<SocketAddr>) -> Config {
        Config {
            listen_port: SocketAddr::new(
                "127.0.0.1".parse().expect(
                    "Failed to parse loopback ip addr",
                ),
                port,
            ),
            peer_addresses: peer_addresses,
        }
    }
}

pub struct Server {
    config: Config,
}

struct TokioMessageSender {
    handle: Handle,
    sender: Sender<OutboundMessage>,
}

impl MessageSender for TokioMessageSender {
    fn send(&mut self, msg: OutboundMessage) {
        let sender = self.sender.clone();
        self.handle.spawn(sender.send(msg).then(|_| future::ok(())))
    }
}

struct TokioTimeoutRequester {
    handle: Handle,
    timer: Timer,
    timeout_handler: Sender<TimeoutRequest>,
}

macro_rules! ignore_error {
    () => {
        |_| ()
    };
}

macro_rules! discard_result {
    () => {
        |_| future::ok(())
    };
}

impl TimeoutRequester for TokioTimeoutRequester {
    fn request_timeout(&mut self, request: TimeoutRequest) -> () {
        let timeout_handler = self.timeout_handler.clone();
        let duration = request.duration();
        let fut = self.timer.sleep(duration)
            .map_err(ignore_error!())
            .then(move |_| {
                timeout_handler.send(request)
                    .and_then(discard_result!())
                    .map_err(ignore_error!())
            });

        self.handle.spawn(fut);
    }
}




impl Server {
    pub fn serve(&self) -> Result<()> {
        let mut core = Core::new()?;
        let handle = core.handle();
        let timer = Timer::default();

        let sock = UdpSocket::bind(&self.config.listen_port, &handle.clone())?;

        let (outbound_socket, incoming_socket) = sock.framed(RumourCodec).split();

        let (outbound_tx, outbound_rx): (Sender<OutboundMessage>,
                                         Receiver<OutboundMessage>) = mpsc::channel(1);

        let out_future = outbound_rx
            .forward(outbound_socket.sink_map_err(ignore_error!()))
            .and_then(discard_result!());

        let sender = TokioMessageSender {
            handle: handle.clone(),
            sender: outbound_tx.clone(),
        };

        let (timeout_tx, timeout_rx) = mpsc::channel(1);

        let timeouter = TokioTimeoutRequester {
            handle: handle.clone(),
            timer: timer.clone(),
            timeout_handler: timeout_tx.clone(),
        };

        let mill = Arc::new(Mutex::new(new_mill(NodeId(self.config.listen_port), sender, timeouter)));
        
        let this_mill = mill.clone();

        let timeout_stream = timeout_rx.for_each(move |request| {
            let mut mill = this_mill.lock().unwrap();
            mill.on_timeout_expired(request);
            future::ok(())
        }).and_then(discard_result!());

        let this_mill = mill.clone();
        let in_stream = incoming_socket
            .and_then(move |msg| {
                this_mill.lock().unwrap().on_message_received(msg);
                future::ok(())
            })
            .for_each(discard_result!())
            .map_err(ignore_error!());

        let this_mill = mill.clone();
        let initial_event = timer.sleep(Duration::from_secs(1))
            .map_err(ignore_error!())
            .and_then(move |_| {
            this_mill.lock().unwrap().do_join(&self.config.peer_addresses);
            future::ok(())
        });

        let joined_stream = out_future.join(in_stream).join(timeout_stream).join(initial_event).then(discard_result!());

        core.run(joined_stream)
    }
}

pub fn server(config: Config) -> Result<Server> {
    Ok(Server {
        config: config,
    })
}
