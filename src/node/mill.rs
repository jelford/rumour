
use std::time::Duration;
use std::net::SocketAddr;
use super::codec::{OutboundMessage, InboundMessage, NodeId, ControlRequest, MessageContent, RumourList, Rumour, KnownPeersList};
use super::node_state::{self, NodeLivenessState};
use super::gossip::{self, Gossip};
use super::RumourObserver;

pub(crate) trait MessageSender {
    fn send(&mut self, msg: OutboundMessage) -> ();
}

pub(crate) trait TimeoutRequester {
    fn request_timeout(&mut self, request: TimeoutRequest) -> ();
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct TimeoutRequest {
    duration: Duration,
    action: OnTimeoutAction,
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum OnTimeoutAction {
    DoHealthCheck,
    CheckPendingPings,
    CheckPendingPingReqs,
    CheckFailureSuspect(NodeId, Duration, u16),
}

impl TimeoutRequest {
    pub(crate) fn duration(&self) -> Duration {
        self.duration.clone()
    }

    fn new(duration: Duration, action: OnTimeoutAction) -> TimeoutRequest {
        TimeoutRequest {
            duration: duration,
            action: action,
        }
    }
}


pub(crate) fn new_mill<'a, RS, TR>(
    id: NodeId,
    message_sender: RS,
    timeout_requester: TR,
    observer: Box<RumourObserver + 'a>,
) -> RumourMill<'a, RS, TR>
where
    RS: self::MessageSender,
    TR: self::TimeoutRequester,
{

    RumourMill {
        id: id,
        message_sender: message_sender,
        timeout_requester: timeout_requester,
        node_state: node_state::new(observer),
        gossip: gossip::new(),
    }
}

#[derive(Debug, PartialEq)]
struct NodeState {
    id: NodeId,
}


/// The RumourMill is responsible for handling domain logic.
///
pub(crate) struct RumourMill<'a, RSender, TRequester>
where
    RSender: MessageSender,
    TRequester: TimeoutRequester,
{
    id: NodeId,
    message_sender: RSender,
    timeout_requester: TRequester,
    node_state: NodeLivenessState<'a>,
    gossip: Gossip,
}

impl <'a, RSender, TRequester> RumourMill<'a, RSender, TRequester>
    where RSender : MessageSender, TRequester : TimeoutRequester {

        pub fn get_live_nodes(&self) -> Vec<NodeId> {
            let mut peers = self.node_state.get_live_nodes();
            peers.push(self.id);
            peers
        }

        pub fn get_suspect_nodes(&self) -> Vec<NodeId> {
            self.node_state.get_suspect_nodes()
        }

        pub fn do_join(&mut self, remote_nodes: &[SocketAddr]) {
            for &node in remote_nodes {
                self.send_control_message(NodeId(node), ControlRequest::MemberJoin);
            }

            self.timeout_requester.request_timeout(TimeoutRequest::new(Duration::from_secs(1), OnTimeoutAction::DoHealthCheck));
        }

        pub fn on_message_received(&mut self, msg: InboundMessage) {
            match msg.content() {
                &MessageContent::ControlPlain(ref content, ref piggy_backed_rumours) => {
                    self.dispatch_control_plain_request(NodeId(msg.source()), &content);
                    self.dispatch_rumours(piggy_backed_rumours.clone().into());
                }
            };
        }

        fn dispatch_control_plain_request(&mut self, source: NodeId, content: &ControlRequest) {
            match content {
                &ControlRequest::Ping => {
                    self.on_ping(source);
                },
                &ControlRequest::PingAck => {
                    self.on_ack(source);
                },
                &ControlRequest::PingRequest(node_id) => {
                    self.on_ping_request(source, node_id);
                },
                &ControlRequest::UnknownMessage => {
                    self.on_unknown_message(source);
                },
                &ControlRequest::PlainBytes(ref bs) => {
                    info!("{}", ::std::string::String::from_utf8_lossy(&bs));
                }
                &ControlRequest::MemberJoin => {
                    self.on_member_join_request(source);
                }
                &ControlRequest::JoinAck(ref live_node_list) => {
                    self.on_join_ack(source, live_node_list.clone().into());
                }
            }
        }

        fn dispatch_rumours(&mut self, rumours: Vec<Rumour>) {
            for rumour in rumours {
                match rumour {
                    Rumour::NodeHasJoin(node) if node != self.id => self.on_member_join(node),
                    _ => ()
                }
            }
        }

        fn send_control_message(&mut self, destination: NodeId, content: ControlRequest) {
            let rumours = self.gossip.pull_rumours(RumourList::capacity());

            self.message_sender.send(OutboundMessage::control_message(destination.0, content, rumours.into()));
        }

        pub fn trigger_begin_failure_detection(&mut self) {
            self.timeout_requester.request_timeout(TimeoutRequest::new(Duration::from_secs(1), OnTimeoutAction::DoHealthCheck));

            let next_node_to_send_message_to = self.next_live_peer_for_message();
            if let Some(node_id) = next_node_to_send_message_to {
                self.send_control_message(node_id, ControlRequest::Ping);
                self.node_state.add_pending_ping(node_id);
                self.timeout_requester.request_timeout(TimeoutRequest::new(Duration::from_millis(200), OnTimeoutAction::CheckPendingPings));
            }

        }

        pub fn on_timeout_expired(&mut self, request: TimeoutRequest) {
            debug!(">>> Timeout received {:?}", request.action);
            match request.action {
                OnTimeoutAction::DoHealthCheck => self.trigger_begin_failure_detection(),
                OnTimeoutAction::CheckPendingPings => self.check_pending_pings(),
                OnTimeoutAction::CheckPendingPingReqs => self.check_pending_ping_reqs(),
                OnTimeoutAction::CheckFailureSuspect(node_id, _, _) => self.check_failure_suspects(node_id),
            }
        }

        fn on_unknown_message(&mut self, source: NodeId) {
            self.send_control_message(source, ControlRequest::PlainBytes(b"hai from service".to_vec()));
        }

        fn on_member_join_request(&mut self, id: NodeId) {
            let live_peers_to_share = self.node_state.get_live_peers(KnownPeersList::capacity());
            self.send_control_message(id, ControlRequest::JoinAck(live_peers_to_share.into()));
            self.on_member_join(id);
            self.gossip.remember(Rumour::NodeHasJoin(id));
        }

        fn on_member_join(&mut self, id: NodeId) {
            self.add_live_node(id);
        }

        fn on_join_ack(&mut self, ack_from: NodeId, live_nodes: Vec<NodeId>) {
            self.add_live_node(ack_from);
            for node in live_nodes.iter() {
                if node != &self.id {
                    self.add_live_node(*node);
                }
            }
        }

        fn on_ping(&mut self, pinger: NodeId) {
            self.send_control_message(pinger, ControlRequest::PingAck);
        }

        fn on_ack(&mut self, acker: NodeId) {
            self.node_state.remove_pending_ping(acker);
        }

        fn on_ping_request(&mut self, _pinger: NodeId, _to_ping: NodeId) {
            // TODO
        }

        fn next_live_peer_for_message(&mut self) -> Option<NodeId> {
            self.node_state.next_live_peer_for_message()

        }

        fn add_live_node(&mut self, id: NodeId) {
            assert!(id != self.id);
            self.node_state.add_live_node(id);
        }

        fn check_pending_pings(&mut self) {

            let nodes_requiring_followup = self.node_state.drain_pending_pings().collect::<Vec<_>>();
            let any_followup = !nodes_requiring_followup.is_empty();
            for p in nodes_requiring_followup {
                self.send_indirect_ping_to(p);
            }

            if any_followup {
                self.timeout_requester.request_timeout(TimeoutRequest::new(Duration::from_millis(200), OnTimeoutAction::CheckPendingPingReqs));
            }
        }

        fn send_indirect_ping_to(&mut self, to_ping: NodeId) {
            let mut number_of_peers_sent_to = 0;
            let ping_helpers = self.node_state.get_live_peers(3);
            for helper in ping_helpers {
                if helper != to_ping {
                    self.send_control_message(helper, ControlRequest::PingRequest(to_ping));
                    number_of_peers_sent_to += 1;
                }
            }

            if number_of_peers_sent_to == 0 {
                warn!("Nobody to help retrying ping {:?} - will retry locally", to_ping);
                let my_id = self.id;
                self.on_ping_request(my_id, to_ping);
            } else {
                info!("Asked {} peers to check on node", number_of_peers_sent_to);
            }

            self.node_state.add_pending_indirect_ping(to_ping);
        }

        fn check_pending_ping_reqs(&mut self) {
            let unreplied_reqs = self.node_state.drain_pending_indirect_pings().collect::<Vec<_>>();

            for p in unreplied_reqs {
                self.suspect(p);
            }
        }

        fn suspicion_timeout(&self) -> Duration {
            Duration::from_secs(2)
        }

        fn suspect(&mut self, suspect_node: NodeId) {
            let timeout = self.suspicion_timeout();
            if self.node_state.add_suspect(suspect_node) {
            info!("Suspect node: {:?}", suspect_node);
                self.timeout_requester.request_timeout(TimeoutRequest::new(timeout, OnTimeoutAction::CheckFailureSuspect(suspect_node, timeout, 0)));
            }
        }

        fn check_failure_suspects(&mut self, suspect: NodeId) {
            self.node_state.accuse_suspect(suspect);
            self.gossip.forget(suspect);
            self.gossip.remember(Rumour::NodeHasLeft(suspect));
        }
}


#[cfg(test)]
mod tests {
    use super::*;
    use super::super::codec::Rumour;
    use std::net::SocketAddr;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct MockMessageSender {
        sent_messages: Arc<Mutex<Vec<OutboundMessage>>>
    }

    impl MockMessageSender {
        fn new() -> MockMessageSender {
            MockMessageSender {
                sent_messages: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn sent_messages(&mut self) -> Vec<OutboundMessage> {
            let mut sent = self.sent_messages.lock().unwrap();
            let mut sent_so_far = Vec::with_capacity(sent.len());
            sent_so_far.append(&mut sent);
            sent_so_far
        }


        fn control_plain_messages(&mut self) -> Vec<(SocketAddr, ControlRequest)> {
            self.sent_messages().iter().filter(|msg| {
                match msg.content() {
                    &MessageContent::ControlPlain(_, _) => true,
                }
            }).map(|m| {
                match m.content() {
                    &MessageContent::ControlPlain(ref r, _) => (m.destination(), r.clone()),
                }
            }).collect()
        }

        fn sent_messages_of_type(&mut self, kind: ControlRequest) -> Vec<OutboundMessage> {
            let mut all = self.sent_messages();
            
            all.retain(|m| {
                match 
                    m.content() {
                        &MessageContent::ControlPlain(ref control_request, _) => {
                            match (control_request, &kind) {
                                (&ControlRequest::Ping, &ControlRequest::Ping) => true,
                                _ => false
                            }
                        },
                    }
                }
            );
            all
        }
    }
    

    impl MessageSender for MockMessageSender {
        fn send(&mut self, msg: OutboundMessage) {
            println!("<<< sending message: {:?}", msg);
            self.sent_messages.lock().unwrap().push(msg);
        }
    }

    #[derive(Clone)]
    struct MockTimeoutRequester {
        requested_timeouts: Arc<Mutex<Vec<TimeoutRequest>>>
    }

    impl MockTimeoutRequester {
        fn new() -> MockTimeoutRequester {
            MockTimeoutRequester {
                requested_timeouts: Arc::new(Mutex::new(Vec::new()))
            }
        }

        fn requested_timeouts(&mut self) -> Vec<TimeoutRequest> {
            self.requested_timeouts.lock().unwrap().clone()
        }
    }

    impl TimeoutRequester for MockTimeoutRequester {
        fn request_timeout(&mut self, request: TimeoutRequest) -> () {
            println!("<<< set timeout: {:?}", request);
            let mut outstanding_requests = self.requested_timeouts.lock().unwrap();
            match outstanding_requests.binary_search_by_key(&request.duration(), |r| r.duration()) {
                Ok(idx) => outstanding_requests.insert(idx, request),
                Err(idx) => outstanding_requests.insert(idx, request),
            }
        }
    }

    fn progress_timeout<RS, TR>(mill: &mut RumourMill<RS, TR>, requester: &mut MockTimeoutRequester, how_far: Duration)
        where RS: MessageSender, TR: TimeoutRequester {
        let mut fired_so_far = Duration::from_millis(0);
        println!("Progressing timeout by {:?}", how_far);
        while fired_so_far < how_far {
            let this_time = process_next_timeout(mill, requester);
            match this_time {
                Some(elapsed) => fired_so_far += elapsed,
                _ => break,
            }
            
        }
    }

    /// Sends the next timeout back to the RumourMill
    /// returns: true if there was anything to send, otherwise false
    fn process_next_timeout<RS, TR>(mill: &mut RumourMill<RS, TR>, requester: &mut MockTimeoutRequester) -> Option<Duration>
        where RS: MessageSender, TR: TimeoutRequester {
        
        let (to_fire, how_far) = {
            let mut timeout_requests = requester.requested_timeouts.lock().unwrap();
            if timeout_requests.is_empty() { return None; }

            let mut timeout_to_fire = timeout_requests.remove(0);
            let how_far = timeout_to_fire.duration;
            timeout_to_fire.duration = Duration::from_millis(0);
            for t in timeout_requests.iter_mut() {
                t.duration -= how_far;
            }
            (timeout_to_fire, how_far)
        };

        mill.on_timeout_expired(to_fire);
        Some(how_far)
    }

    fn localnode() -> NodeId {
        NodeId(SocketAddr::new("127.0.0.1".parse().expect("localhost ip address parsing failed"), 2073))
    }

    fn remote_node(ip: &str) -> NodeId {
        NodeId(SocketAddr::new(ip.parse().expect("Couldn't parse ip addr"), 2073))
    }

    #[derive(Debug, Clone)]
    struct NoopRumourObserver {
        dead_notifications_count: Arc<Mutex<u64>>,
        joined_notifications_count: Arc<Mutex<u64>>,
    }
    impl RumourObserver for NoopRumourObserver {
        fn on_node_dead(&mut self) -> () { *self.dead_notifications_count.lock().unwrap() += 1;}
        fn on_node_joined(&mut self) -> () { *self.joined_notifications_count.lock().unwrap() += 1; }
    }

    fn noop_observer() -> Box<NoopRumourObserver> {
        Box::new(NoopRumourObserver{dead_notifications_count: Arc::new(Mutex::new(0)), joined_notifications_count: Arc::new(Mutex::new(0))})
    }

    macro_rules! val {
        ($e: expr) => {
            *$e.lock().unwrap()
        };
    }
        

    #[test]
    fn shows_self_as_active_from_the_start() {
        let mill = new_mill(localnode(), MockMessageSender::new(), MockTimeoutRequester::new(), noop_observer());
        assert_eq!(mill.get_live_nodes(), &[localnode()])
    }

    #[test]
    fn adds_joined_node_to_active_list_right_away() {
        let mut mill = new_mill(localnode(), MockMessageSender::new(), MockTimeoutRequester::new(), noop_observer());
        let remote_node = remote_node("127.0.0.2");

        mill.on_message_received(InboundMessage::bare_control_plain_message(remote_node.0, ControlRequest::MemberJoin));

        let live_nodes = mill.get_live_nodes();
        assert!(live_nodes.contains(&localnode()));
        assert!(live_nodes.contains(&remote_node));
    }

    #[test]
    fn responds_to_ping() {
        let mut message_sender = MockMessageSender::new();
        let mut mill = new_mill(localnode(), message_sender.clone(), MockTimeoutRequester::new(), noop_observer());
        let remote_node = remote_node("127.0.0.2");

        mill.on_message_received(InboundMessage::bare_control_plain_message(remote_node.0, ControlRequest::Ping));

        assert!(message_sender.control_plain_messages().contains(&(remote_node.0, ControlRequest::PingAck)));
    }

    #[test]
    fn do_join_sends_join_messages_to_known_nodes() {
        let mut message_sender = MockMessageSender::new();
        let mut mill = new_mill(localnode(), message_sender.clone(), MockTimeoutRequester::new(), noop_observer());
        let remote_node = remote_node("127.0.0.2");

        mill.do_join(&[remote_node.0]);

        let sent = message_sender.control_plain_messages();
        assert!(sent.contains(&(remote_node.0, ControlRequest::MemberJoin)));
    }

    #[test]
    fn do_join_schedules_a_health_check_timeout() {
        let mut timeout_requester = MockTimeoutRequester::new();
        let mut mill = new_mill(localnode(), MockMessageSender::new(), timeout_requester.clone(), noop_observer());
        let remote_node = remote_node("127.0.0.2");

        mill.do_join(&[remote_node.0]);

        let requested_timeouts = timeout_requester.requested_timeouts();
        assert!(requested_timeouts.contains(&TimeoutRequest::new(Duration::from_secs(1), OnTimeoutAction::DoHealthCheck)));
    }

    #[test]
    fn remote_nodes_marked_live_when_join_ack_received() {
        let mut mill = new_mill(localnode(), MockMessageSender::new(), MockTimeoutRequester::new(), noop_observer());
        let remote_node1 = remote_node("127.0.0.2");
        let remote_node2 = remote_node("127.0.0.3");
        let remote_node3 = remote_node("127.0.0.4");

        let live_nodes = mill.get_live_nodes();
        assert!(!live_nodes.contains(&remote_node1));

        mill.on_message_received(
            InboundMessage::bare_control_plain_message(remote_node1.0, 
                ControlRequest::JoinAck(vec![remote_node2, remote_node3].into())));

        let live_nodes = mill.get_live_nodes();

        assert!(live_nodes.contains(&remote_node1));
        assert!(live_nodes.contains(&remote_node2));
        assert!(live_nodes.contains(&remote_node3));
        assert_eq!(live_nodes.len(), 4)
    }

    #[test]
    fn health_check_pings_some_known_nodes() {
        let mut message_sender = MockMessageSender::new();
        let mut mill = new_mill(localnode(), message_sender.clone(), MockTimeoutRequester::new(), noop_observer());
        let remote_node = remote_node("127.0.0.2");

        mill.on_message_received(
            InboundMessage::bare_control_plain_message(remote_node.0, 
                ControlRequest::JoinAck(vec![].into())));

        mill.trigger_begin_failure_detection();

        let sent_pings = message_sender.sent_messages_of_type(ControlRequest::Ping);

        assert!(!sent_pings.is_empty())
    }


    #[test]
    fn when_node_responds_to_ping_then_it_remains_healthy() {
        let message_sender = MockMessageSender::new();
        let mut timouter = MockTimeoutRequester::new();
        let mut mill = new_mill(localnode(), message_sender.clone(), timouter.clone(), noop_observer());

        let remote_node = remote_node("127.0.0.2");
        mill.on_message_received(
            InboundMessage::bare_control_plain_message(remote_node.0,
                ControlRequest::JoinAck(vec![].into())));

        mill.trigger_begin_failure_detection();
        mill.on_message_received(InboundMessage::bare_control_plain_message(remote_node.0, ControlRequest::PingAck));

        process_next_timeout(&mut mill, &mut timouter);
        process_next_timeout(&mut mill, &mut timouter);
        process_next_timeout(&mut mill, &mut timouter);

        assert!(mill.get_live_nodes().contains(&remote_node));
        assert!(!mill.get_suspect_nodes().contains(&remote_node));
    }

    #[test]
    fn when_node_doesnt_respond_to_ping_then_ping_req_sent() {
        let mut message_sender = MockMessageSender::new();
        let mut timouter = MockTimeoutRequester::new();
        let mut mill = new_mill(localnode(), message_sender.clone(), timouter.clone(), noop_observer());

        let remote_node1 = remote_node("127.0.0.2");
        let remote_node2 = remote_node("127.0.0.3");

        mill.on_message_received(
            InboundMessage::bare_control_plain_message(remote_node1.0, 
                ControlRequest::JoinAck(vec![remote_node2].into())));

        assert_eq!(mill.get_live_nodes().len(), 3);

        mill.trigger_begin_failure_detection();
        mill.trigger_begin_failure_detection(); // so that it will have health-checked every node

        mill.on_message_received(
            InboundMessage::bare_control_plain_message(remote_node2.0, ControlRequest::PingAck));

        process_next_timeout(&mut mill, &mut timouter); process_next_timeout(&mut mill, &mut timouter); // flush out all ping checks

        assert_eq!(mill.get_live_nodes().len(), 3); // Not dead until ping-req has failed

        assert!(message_sender.control_plain_messages().contains(&(remote_node2.0, ControlRequest::PingRequest(remote_node1))));

    }

    #[test]
    fn when_node_doesnt_respond_to_ping_or_ping_req_then_marked_as_suspect() {
        let message_sender = MockMessageSender::new();
        let mut timouter = MockTimeoutRequester::new();
        let mut mill = new_mill(localnode(), message_sender.clone(), timouter.clone(), noop_observer());

        let remote_node1 = remote_node("127.0.0.2");
        let remote_node2 = remote_node("127.0.0.3");

        mill.on_message_received(
            InboundMessage::bare_control_plain_message(remote_node1.0, 
                ControlRequest::JoinAck(vec![remote_node2].into())));

        assert_eq!(mill.get_live_nodes().len(), 3);

        mill.trigger_begin_failure_detection();

        process_next_timeout(&mut mill, &mut timouter);
        process_next_timeout(&mut mill, &mut timouter); // both sets of pings failed

        assert_eq!(mill.get_live_nodes().len(), 3); // Still not dead, just suspect
        assert!(mill.get_suspect_nodes().contains(&remote_node1));
    }


    #[test]
    fn after_node_has_been_suspect_on_timeout_remove_from_live_set() {
        let message_sender = MockMessageSender::new();
        let mut timouter = MockTimeoutRequester::new();
        let mut mill = new_mill(localnode(), message_sender.clone(), timouter.clone(), noop_observer());

        let remote_node1 = remote_node("127.0.0.2");
        let remote_node2 = remote_node("127.0.0.3");

        mill.on_message_received(InboundMessage::bare_control_plain_message(remote_node1.0, 
            ControlRequest::JoinAck(vec![remote_node2].into())));
        assert_eq!(mill.get_live_nodes().len(), 3);

        mill.trigger_begin_failure_detection();

        progress_timeout(&mut mill, &mut timouter, Duration::from_millis(500));

        assert!(mill.get_suspect_nodes().contains(&remote_node1));

        progress_timeout(&mut mill, &mut timouter, Duration::from_secs(10));

        assert!(mill.get_suspect_nodes().is_empty());
        assert!(!mill.get_live_nodes().contains(&remote_node1));
    }

    #[test]
    fn if_no_peers_then_failed_ping_still_results_in_node_suspected() {
        let mut message_sender = MockMessageSender::new();
        let mut timouter = MockTimeoutRequester::new();
        let mut mill = new_mill(localnode(), message_sender.clone(), timouter.clone(), noop_observer());

        let remote_node = remote_node("127.0.0.2");

        mill.on_message_received(InboundMessage::bare_control_plain_message(remote_node.0, ControlRequest::MemberJoin));

        mill.trigger_begin_failure_detection();

        progress_timeout(&mut mill, &mut timouter, Duration::from_secs(1));

        assert!(mill.get_live_nodes().contains(&remote_node));
        assert!(mill.get_suspect_nodes().contains(&remote_node));

        progress_timeout(&mut mill, &mut timouter, Duration::from_secs(10)); // suspicion timeout expires

        assert!(!mill.get_live_nodes().contains(&remote_node));
        assert!(!mill.get_suspect_nodes().contains(&remote_node)); // both lists are "live-ish" - should be removed from both.

        // The only messages that should have been sent with no peers are pings
        assert!(message_sender.control_plain_messages().iter().all(|m| {
            let &(_,ref msg) = m;
            match msg {
                &ControlRequest::Ping | &ControlRequest::MemberJoin | &ControlRequest::JoinAck(_) => true,
                _ => false
            }
        }));

        mill.trigger_begin_failure_detection();
        let sent_messages_this_time = message_sender.sent_messages();
        assert!(sent_messages_this_time.is_empty()); // Should no longer send health checks to dead node
    }

    #[test]
    fn acks_join_requests_with_any_known_nodes() {
        let mut message_sender = MockMessageSender::new();
        let timouter = MockTimeoutRequester::new();
        let mut mill = new_mill(localnode(), message_sender.clone(), timouter.clone(), noop_observer());

        let remote_node1 = remote_node("127.0.0.2");
        let remote_node2 = remote_node("127.0.0.3");

        mill.on_message_received(bare_join_message(remote_node1));
        mill.on_message_received(bare_join_message(remote_node2));

        let messages = message_sender.control_plain_messages();
        assert!(messages.contains(&(remote_node1.0, ControlRequest::JoinAck(vec![].into()))));
        assert!(messages.contains(&(remote_node2.0, ControlRequest::JoinAck(vec![remote_node1].into()))));

    }

    fn bare_join_message(node: NodeId) -> InboundMessage {
        InboundMessage::bare_control_plain_message(node.0, ControlRequest::MemberJoin)
    }

    fn gossip_from(msg: &OutboundMessage) -> Option<&RumourList> {
        match msg.content() {
            &MessageContent::ControlPlain(_, ref rl) => Some(rl),
        }
    }

    #[test]
    fn other_nodes_will_hear_about_membership_through_control_messages() {
        let mut message_sender = MockMessageSender::new();
        let mut timouter = MockTimeoutRequester::new();
        let mut mill = new_mill(localnode(), message_sender.clone(), timouter.clone(), noop_observer());

        let remote1 = remote_node("127.0.0.2");
        let remote2 = remote_node("127.0.0.3");

        mill.on_message_received(bare_join_message(remote1));
        mill.on_message_received(bare_join_message(remote2));
        mill.trigger_begin_failure_detection();

        progress_timeout(&mut mill, &mut timouter, Duration::from_secs(5)); // enough time for a health check to have been sent to remote1

        let sent_messages = message_sender.sent_messages();
        assert!(sent_messages.iter().any(|m| {
            match gossip_from(&m) {
                Some(rumours) => {
                    (m.destination() == remote1.0) && rumours.contains(&Rumour::NodeHasJoin(remote2))
                },
                _ => false
            }
        }));
    }

    #[test]
    fn once_node_is_dead_stop_gossipping_that_it_has_joined() {
        let mut message_sender = MockMessageSender::new();
        let mut timouter = MockTimeoutRequester::new();
        let mut mill = new_mill(localnode(), message_sender.clone(), timouter.clone(), noop_observer());

        let remote2 = remote_node("127.0.0.2");
        let remote3 = remote_node("127.0.0.3");

        mill.on_message_received(bare_join_message(remote2));
        mill.trigger_begin_failure_detection();

        progress_timeout(&mut mill, &mut timouter, Duration::from_secs(20)); // enough time for remote2 to be marked as failed

        mill.on_message_received(bare_join_message(remote3));
        
        let sent_messages = message_sender.sent_messages();
        assert!(!sent_messages.iter().any(|m| {
            match gossip_from(&m) {
                Some(rumours) => {
                    (m.destination() == remote3.0) && rumours.contains(&Rumour::NodeHasJoin(remote2))
                },
                _ => false
            }
        }));
    }

    #[test]
    fn node_can_pick_up_peer_join_through_control_plane_gossip() {
        let message_sender = MockMessageSender::new();
        let timouter = MockTimeoutRequester::new();
        let mut mill = new_mill(localnode(), message_sender.clone(), timouter.clone(), noop_observer());

        let remote1 = remote_node("127.0.0.2");
        let remote2 = remote_node("127.0.0.3");

        mill.on_message_received(
            InboundMessage::new(remote1.0, ControlRequest::Ping, vec![Rumour::NodeHasJoin(remote2)]));
            
        assert!(mill.get_live_nodes().contains(&remote2));
    }

    #[test]
    fn observer_received_node_lifecycle_events() {
        let observer = noop_observer();
        let message_sender = MockMessageSender::new();
        let mut timouter = MockTimeoutRequester::new();
        let mut mill = new_mill(localnode(), message_sender.clone(), timouter.clone(), observer.clone());

        let remote_node = remote_node("127.0.0.2");
        mill.on_message_received(InboundMessage::bare_control_plain_message(remote_node.0, ControlRequest::MemberJoin));
        assert_eq!(val!(observer.joined_notifications_count), 1);
        assert_eq!(val!(observer.dead_notifications_count), 0);

        mill.trigger_begin_failure_detection();
        progress_timeout(&mut mill, &mut timouter, Duration::from_secs(12));
        assert_eq!(val!(observer.dead_notifications_count), 1);

    }
}