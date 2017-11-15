

use std::collections::HashSet;
use std::cmp::Eq;
use std::hash::Hash;
use std::vec::{Vec, Drain};

use rand;

use super::codec::{NodeId};

pub(crate) fn new() -> NodeLivenessState {
    NodeLivenessState {
        live_nodes: HashSet::new(),
        live_node_round_robin: Vec::new(),
        live_node_round_robin_idx: 0,
        pending_pings: Vec::new(),
        pending_ping_reqs: Vec::new(),
        suspect_nodes: Vec::new(),
    }
}

#[derive(Debug)]
pub(crate) struct NodeLivenessState {

    live_nodes: HashSet<NodeId>,
    live_node_round_robin_idx : usize,
    live_node_round_robin: Vec<NodeId>,
    pending_pings: Vec<NodeId>,
    pending_ping_reqs: Vec<NodeId>,

    suspect_nodes: Vec<NodeId>,
}

fn add_to_vec<T>(thing: T, destination: &mut Vec<T>) -> bool
    where T: Eq {
    for t in destination.iter() {
        if t == &thing {
            return false;
        }
    }

    destination.push(thing);
    true
}

impl NodeLivenessState {
    pub(crate) fn get_live_nodes(&self) -> Vec<NodeId> {
        to_vec(&self.live_nodes)
    }

    pub(crate) fn get_suspect_nodes(&self) -> Vec<NodeId> {
        self.suspect_nodes.clone()
    }

    pub(crate) fn add_pending_ping(&mut self, pinged_node: NodeId) {
        add_to_vec(pinged_node, &mut self.pending_pings);
    }

    pub(crate) fn remove_pending_ping(&mut self, acker: NodeId) {
        let mut got_to = 0;
        while got_to < self.pending_pings.len() {
            if self.pending_pings[got_to] == acker {
                self.pending_pings.swap_remove(got_to);
            } else {
                got_to += 1;
            }
        }
    }

    pub(crate) fn live_nodes(&self) -> Vec<NodeId> {
        to_vec(&self.live_nodes)
    }

    pub(crate) fn next_live_peer_for_message(&mut self) -> Option<NodeId> {
        if self.live_node_round_robin.is_empty() {
            return None;
        }

        if self.live_node_round_robin_idx >= self.live_node_round_robin.len() {
            self.re_index_round_robin_list();
        }

        let idx_to_pick = self.live_node_round_robin_idx;
        self.live_node_round_robin_idx += 1;            

        Some(self.live_node_round_robin[idx_to_pick])
    }

    pub(crate) fn re_index_round_robin_list(&mut self) {
        self.live_node_round_robin_idx = 0;
        let node_count = self.live_node_round_robin.len();
        for i in 0..node_count {
            self.live_node_round_robin.swap(i, rand::random::<usize>() % node_count);
        }
    }

    pub(crate) fn get_live_peers(&mut self, number: usize) -> Vec<NodeId> {
        let live_peer_count = self.live_node_round_robin.len();
        let n = ::std::cmp::min(number, live_peer_count);
        let mut out = Vec::with_capacity(n);

        let from_this_round = ::std::cmp::min(live_peer_count - self.live_node_round_robin_idx, n);
        let from_next_round = n - from_this_round;

        for node in self.live_node_round_robin[self.live_node_round_robin_idx..(self.live_node_round_robin_idx+from_this_round)].iter() {
            out.push(*node);
        }
        if from_next_round > 0 {
            for node in self.live_node_round_robin[0..from_next_round].iter() {
                out.push(*node);
            }
        }

        out
    }

    pub(crate) fn add_live_node(&mut self, id: NodeId) {
        if self.live_nodes.insert(id) {
            self.live_node_round_robin.push(id);
        }
    }

    pub(crate) fn remove_live_node(&mut self, node_to_remove: NodeId) {
        let mut found = false;
        for i in 0..self.live_node_round_robin.len() {
            let node_inspecting = self.live_node_round_robin[i];
            if node_inspecting == node_to_remove {
                self.live_node_round_robin.swap_remove(i);
                self.re_index_round_robin_list();
                found = true;
                break;
            }
        }

        if !found {
            println!("Failed to find node to remove {:?} from {:?}", node_to_remove, self.live_node_round_robin);
        }

        self.live_nodes.remove(&node_to_remove);

        println!("After removing {:?}, remaining live nodes: {:?}", node_to_remove, self.live_node_round_robin);
    }

    pub(crate) fn add_pending_indirect_ping(&mut self, node_being_pinged: NodeId) {
        add_to_vec(node_being_pinged, &mut self.pending_ping_reqs);
    }

    pub(crate) fn drain_pending_pings(&mut self) -> Drain<NodeId>{
        self.pending_pings.drain(..)
    }

    pub(crate) fn drain_pending_indirect_pings(&mut self) -> Drain<NodeId> {
        self.pending_ping_reqs.drain(..)
    }

    pub(crate) fn add_suspect(&mut self, suspected_node: NodeId) -> bool {
        add_to_vec(suspected_node, &mut self.suspect_nodes)
    }

    pub(crate) fn accuse_suspect(&mut self, suspected_node: NodeId) -> bool {
        let mut suspect_index = None;

        for (i, &s) in self.suspect_nodes.iter().enumerate() {
            if s == suspected_node {
                suspect_index = Some(i);
                break;
            }
        }

        match suspect_index {
            Some(i) => {
                self.suspect_nodes.swap_remove(i);
                self.remove_live_node(suspected_node);
                true
            },
            None => false
        }
    }
}

fn to_vec<T>(s: &HashSet<T>) -> Vec<T> 
    where T: Eq + Hash + Clone {

    let mut out : Vec<T> = Vec::with_capacity(s.len());
    for node in s.iter() {
        out.push(node.clone());
    }
    out
}