
use std::vec::Vec;
use std::net::SocketAddr;
use bincode::{serialize_into, deserialize, Bounded, Result as BincodeResult, ErrorKind as BincodeErrorKind};
use tokio_core::net::UdpCodec;

#[derive(Debug, Hash, Eq, PartialEq, Copy, Clone, Serialize, Deserialize)]
pub(crate) struct NodeId(pub SocketAddr);

#[derive(Debug, Eq, PartialEq, Clone)]
pub(crate) struct InboundMessage {
    source: SocketAddr,
    content: MessageContent,
}

fixed_size_list!(RumourList: Rumour; 5; derive(Debug, Eq, PartialEq, Clone, Serialize, Deserialize));
fixed_size_list!(KnownPeersList: NodeId; 5; derive(Debug, Eq, PartialEq, Clone, Serialize, Deserialize));


#[derive(Debug, Eq, PartialEq, Clone, Serialize, Deserialize)]
pub(crate) enum MessageContent {
    ControlPlain(ControlRequest, RumourList)
}

impl InboundMessage {
    
    pub(crate) fn bare_control_plain_message(from: SocketAddr, content: ControlRequest) -> InboundMessage {
        InboundMessage {
            source: from,
            content: MessageContent::ControlPlain(content, RumourList::empty()),
        }
    }

    pub(crate) fn new(from: SocketAddr, request: ControlRequest, rumours: Vec<Rumour>) -> InboundMessage {
        InboundMessage {
            source: from,
            content: MessageContent::ControlPlain(request, rumours.into()),
        }
    }

    pub(crate) fn source(&self) -> SocketAddr {
        self.source
    }

    pub(crate) fn content(&self) -> &MessageContent {
        &self.content
    }
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub(crate) struct OutboundMessage {
    destination: SocketAddr,
    content: MessageContent,
}


impl OutboundMessage {
    pub(crate) fn control_message(destination: SocketAddr, content: ControlRequest, piggyback_rumours: RumourList) -> OutboundMessage {
        OutboundMessage {
            destination: destination,
            content: MessageContent::ControlPlain(content, piggyback_rumours),
        }
    }

    pub(crate) fn destination(&self) -> SocketAddr {
        self.destination
    }

    pub(crate) fn content(&self) -> &MessageContent {
        &self.content
    }

}

#[derive(Debug, Eq, PartialEq, Clone, Serialize, Deserialize)]
pub(crate) enum ControlRequest {
    PlainBytes(Vec<u8>),
    UnknownMessage,
    MemberJoin,
    JoinAck(KnownPeersList),
    Ping,
    PingAck,
    PingRequest(NodeId),
}

#[derive(Debug, Eq, PartialEq, Clone, Serialize, Deserialize, Copy)]
pub(crate) enum Rumour {
    NodeHasJoin(NodeId),
    NodeHasLeft(NodeId),
}

pub(crate) struct RumourCodec;

// https://stackoverflow.com/a/15003663
const _MAXIMUM_UDP_PACKET_CONTENT: u64 = 508;

impl UdpCodec for RumourCodec {
    type In = InboundMessage;
    type Out = OutboundMessage;

    fn decode(&mut self, src: &SocketAddr, buf: &[u8]) -> ::std::io::Result<Self::In> {
        let deserialized: BincodeResult<MessageContent> = deserialize(&buf);
        match deserialized {
            Ok(inbound_data) => {
                debug!("<<< Received: {:?}", inbound_data);
                Ok(InboundMessage { source: src.clone(), content: inbound_data })
            },
            Err(ek) => {
                match *ek {
                    BincodeErrorKind::IoError(io_error) => Err(io_error),
                    BincodeErrorKind::Custom(msg) => {
                        panic!(format!("Bincode error: {}", msg))
                    },
                    _ => {
                        error!("Bincode error: {:?}", ek);
                        panic!("There was a problem deserializing inbound messaged. This is a fatal implementation fault. Processing will not resume.")
                    }
                }
            }
        }
    }

    fn encode(&mut self, msg: Self::Out, buf: &mut Vec<u8>) -> SocketAddr {
        debug!(">>> Sending {:?}", msg);
        let encoding_attempt = serialize_into(buf, msg.content(), Bounded(_MAXIMUM_UDP_PACKET_CONTENT));
        if encoding_attempt.is_err() {
            panic!("Unable to send network messages; message too big. This is a fatal implementation fault. Processing will not resume.");
        }
        msg.destination
    }
}
