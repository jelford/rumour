use super::codec::{Rumour, NodeId};
use std::cmp::{min, Ord, PartialOrd, Ordering, PartialEq};

#[derive(Debug, Eq)]
struct GossipState {
    number_of_times_to_spread: u16, 
    rumour: Rumour,
}

impl PartialEq for GossipState {
    fn eq(&self, other: &Self) -> bool {
        self.number_of_times_to_spread == other.number_of_times_to_spread && self.rumour == other.rumour
    }
}

impl PartialOrd for GossipState {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.number_of_times_to_spread.partial_cmp(&other.number_of_times_to_spread)
    }
}

impl Ord for GossipState {
    fn cmp(&self, other: &Self) -> Ordering {
        self.number_of_times_to_spread.cmp(&other.number_of_times_to_spread)
    }
}

#[derive(Debug)]
pub(crate) struct Gossip {
    gossip: Vec<GossipState>,
}

impl Gossip {
    pub(crate) fn pull_rumours(&mut self, how_many: usize) -> Vec<Rumour> {
        let how_many = min(self.gossip.len(), how_many);
        let mut to_send = Vec::with_capacity(how_many);
        let mut to_reinsert = Vec::with_capacity(how_many);
        
        for gs in self.gossip.drain(..how_many) {
            let mut gss = gs;
            to_send.push(gss.rumour);
            if gss.number_of_times_to_spread > 1 {
                gss.number_of_times_to_spread -= 1;
                to_reinsert.push(gss);
            }
        }

        for gs in to_reinsert {
            self.insert(gs);
        }

        to_send
    }

    fn insert(&mut self, gs: GossipState) {
        let search = self.gossip.binary_search_by_key(&gs.number_of_times_to_spread, |s| s.number_of_times_to_spread);
        let pos = match search {
            Ok(x) => x,
            Err(x) => x,
        };
        self.gossip.insert(pos, gs);
    }

    pub(crate) fn remember(&mut self, rumour: Rumour) {

        if self.gossip.iter().any(|gs| gs.rumour == rumour) {
            return;
        }
        self.insert(GossipState{number_of_times_to_spread: 10, rumour: rumour});
    }

    pub(crate) fn forget(&mut self, node: NodeId) {
        self.gossip.retain(|g| {
            match g.rumour {
                Rumour::NodeHasJoin(nid) | Rumour::NodeHasLeft(nid) => nid != node
            }
        });
    }
}

pub(crate) fn new() -> Gossip {
    Gossip {
        gossip: Vec::with_capacity(10),
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use super::super::codec::NodeId;

    fn a_node() -> NodeId {
        NodeId("127.0.0.2:2073".parse().unwrap())
    }

    #[test]
    fn initially_no_rumours() {
        let mut gossip = new();
        let result = gossip.pull_rumours(5);
        assert!(result.is_empty());
    }

    #[test]
    fn if_i_tell_it_a_rumour_then_i_can_retreive_that() {
        let mut gossip = new();
        let rumour = Rumour::NodeHasJoin(a_node());

        gossip.remember(rumour);
        let result = gossip.pull_rumours(1);

        assert!(result.contains(&rumour))
    }

    #[test]
    fn eventually_stops_spreaing_a_rumour() {
        let mut gossip = new();
        let rumour = Rumour::NodeHasJoin(a_node());

        gossip.remember(rumour);

        for _ in 0..100 {
            if !gossip.pull_rumours(1).contains(&rumour) {
                return;
            }
        }

        panic!("Still spreading {:?} after 100 queuries", rumour);
    }

    #[test]
    fn hearing_something_weve_heard_before_is_not_remembered() {
        let mut gossip = new();
        let rumour = Rumour::NodeHasJoin(a_node());

        gossip.remember(rumour);

        let mut number_of_times_to_spread = 0;
        for i in 1..100 {
            if gossip.pull_rumours(1).contains(&rumour) {
                number_of_times_to_spread = i;
            } else {
                break;
            }
        }

        gossip.remember(rumour);
        for _ in 0..number_of_times_to_spread-1 {
            let _ = gossip.pull_rumours(1);
        }

        gossip.remember(rumour);
        let _ = gossip.pull_rumours(1);
        assert!(!gossip.pull_rumours(1).contains(&rumour));

    }
}