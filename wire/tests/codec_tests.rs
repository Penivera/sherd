use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use wire::{
    get_hop_count, set_hop_count, HandshakeAckPayload, HandshakePayload, Header, MessagePayload,
    MessageType, NodeId, Packet, PeerAnnouncePayload, PeerInfoWire, PeerRequestPayload,
    PingPayload, PongPayload, TaskAnnouncePayload, TaskClaimAckPayload, TaskClaimPayload,
    TaskDataPayload, TaskId, TaskRequestPayload, TaskResultPayload, WireError, FLAG_GOSSIP,
    MAGIC_BYTES, MAX_PACKET_SIZE, PROTOCOL_VERSION,
};

#[test]
fn test_header_encode_decode() {
    let header = Header::new(MessageType::Ping, FLAG_GOSSIP, 0x123456789abcdef0, 8);
    let mut buf = [0u8; 18];
    header.encode(&mut buf).expect("Encode header should succeed");

    assert_eq!(&buf[0..4], &MAGIC_BYTES);
    assert_eq!(buf[4], PROTOCOL_VERSION);
    assert_eq!(buf[5], MessageType::Ping.to_u8());

    let decoded = Header::decode(&buf).expect("Decode header should succeed");
    assert_eq!(header, decoded);
}

#[test]
fn test_header_rejections() {
    // Bad magic
    let mut bad_magic = [0u8; 18];
    bad_magic[0..4].copy_from_slice(b"BAD!");
    bad_magic[4] = PROTOCOL_VERSION;
    bad_magic[5] = MessageType::Ping.to_u8();
    assert!(matches!(Header::decode(&bad_magic), Err(WireError::InvalidMagic { .. })));

    // Bad version
    let mut bad_ver = [0u8; 18];
    bad_ver[0..4].copy_from_slice(&MAGIC_BYTES);
    bad_ver[4] = 99;
    bad_ver[5] = MessageType::Ping.to_u8();
    assert!(matches!(Header::decode(&bad_ver), Err(WireError::UnsupportedVersion(99))));

    // Truncated header
    assert!(matches!(Header::decode(&[0u8; 10]), Err(WireError::PacketTooShort(10))));
}

#[test]
fn test_ping_pong_packet_roundtrip() {
    let ping_pkt = Packet::new(0, 100, MessagePayload::Ping(PingPayload { nonce: 42 }));
    let encoded = ping_pkt.encode().expect("Encode ping packet");
    let decoded = Packet::decode(&encoded).expect("Decode ping packet");
    assert_eq!(ping_pkt, decoded);

    let pong_pkt = Packet::new(0, 101, MessagePayload::Pong(PongPayload { nonce: 42 }));
    let encoded = pong_pkt.encode().expect("Encode pong packet");
    let decoded = Packet::decode(&encoded).expect("Decode pong packet");
    assert_eq!(pong_pkt, decoded);
}

#[test]
fn test_handshake_packet_roundtrip() {
    let node_id = NodeId::new([0xaa; 32]);
    let challenge = [0x55; 32];
    let signature = [0x77; 64];

    let handshake_payload = HandshakePayload {
        node_id,
        listen_port: 9000,
        version: PROTOCOL_VERSION,
        timestamp: 1700000000,
        challenge,
        signature,
    };

    let pkt = Packet::new(0, 1, MessagePayload::Handshake(handshake_payload));
    let encoded = pkt.encode().expect("Encode handshake packet");
    let decoded = Packet::decode(&encoded).expect("Decode handshake packet");
    assert_eq!(pkt, decoded);
}

#[test]
fn test_handshake_ack_packet_roundtrip() {
    let node_id = NodeId::new([0xbb; 32]);
    let challenge = [0x33; 32];
    let signature = [0x88; 64];

    let ack_payload = HandshakeAckPayload {
        node_id,
        status: 0,
        timestamp: 1700000005,
        challenge,
        signature,
    };

    let pkt = Packet::new(0, 2, MessagePayload::HandshakeAck(ack_payload));
    let encoded = pkt.encode().expect("Encode handshake ack packet");
    let decoded = Packet::decode(&encoded).expect("Decode handshake ack packet");
    assert_eq!(pkt, decoded);
}

#[test]
fn test_peer_announce_roundtrip_v4_and_v6() {
    let peer1 = PeerInfoWire {
        node_id: NodeId::new([1u8; 32]),
        addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8000),
    };
    let peer2 = PeerInfoWire {
        node_id: NodeId::new([2u8; 32]),
        addr: SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 8001),
    };

    let announce = PeerAnnouncePayload {
        peers: vec![peer1, peer2],
    };

    let pkt = Packet::new(FLAG_GOSSIP, 55, MessagePayload::PeerAnnounce(announce));
    let encoded = pkt.encode().expect("Encode peer announce packet");
    let decoded = Packet::decode(&encoded).expect("Decode peer announce packet");
    assert_eq!(pkt, decoded);
}

#[test]
fn test_peer_request_roundtrip() {
    let req = PeerRequestPayload { max_peers: 16 };
    let pkt = Packet::new(0, 60, MessagePayload::PeerRequest(req));
    let encoded = pkt.encode().expect("Encode peer request packet");
    let decoded = Packet::decode(&encoded).expect("Decode peer request packet");
    assert_eq!(pkt, decoded);
}

#[test]
fn test_task_lifecycle_packets_roundtrip() {
    let author = NodeId::new([0x01; 32]);
    let worker = NodeId::new([0x02; 32]);
    let raw_payload = b"compute_primes_1_to_10000".to_vec();
    let task_id = TaskId::compute(&author, 10, 1234567, &raw_payload);

    // 1. TaskAnnounce
    let announce = TaskAnnouncePayload {
        task_id,
        author,
        priority: 10,
        created_at: 1234567,
        payload_size: raw_payload.len() as u32,
    };
    let announce_pkt = Packet::new(FLAG_GOSSIP, 200, MessagePayload::TaskAnnounce(announce));
    let encoded = announce_pkt.encode().expect("Encode task announce");
    let decoded = Packet::decode(&encoded).expect("Decode task announce");
    assert_eq!(announce_pkt, decoded);

    // 2. TaskRequest
    let req = TaskRequestPayload { task_id };
    let req_pkt = Packet::new(0, 201, MessagePayload::TaskRequest(req));
    let encoded = req_pkt.encode().expect("Encode task request");
    let decoded = Packet::decode(&encoded).expect("Decode task request");
    assert_eq!(req_pkt, decoded);

    // 3. TaskData
    let data = TaskDataPayload {
        task_id,
        author,
        priority: 10,
        created_at: 1234567,
        payload: raw_payload,
    };
    let data_pkt = Packet::new(0, 202, MessagePayload::TaskData(data));
    let encoded = data_pkt.encode().expect("Encode task data");
    let decoded = Packet::decode(&encoded).expect("Decode task data");
    assert_eq!(data_pkt, decoded);

    // 4. TaskClaim
    let claim = TaskClaimPayload {
        task_id,
        claimant: worker,
        claimed_at: 1234570,
        signature: [0xcc; 64],
    };
    let claim_pkt = Packet::new(0, 203, MessagePayload::TaskClaim(claim));
    let encoded = claim_pkt.encode().expect("Encode task claim");
    let decoded = Packet::decode(&encoded).expect("Decode task claim");
    assert_eq!(claim_pkt, decoded);

    // 5. TaskClaimAck
    let claim_ack = TaskClaimAckPayload {
        task_id,
        claimant: worker,
        accepted: true,
        reason: 0,
        owner: author,
        signature: [0xdd; 64],
    };
    let claim_ack_pkt = Packet::new(0, 204, MessagePayload::TaskClaimAck(claim_ack));
    let encoded = claim_ack_pkt.encode().expect("Encode task claim ack");
    let decoded = Packet::decode(&encoded).expect("Decode task claim ack");
    assert_eq!(claim_ack_pkt, decoded);

    // 6. TaskResult
    let result = TaskResultPayload {
        task_id,
        worker,
        success: true,
        completed_at: 1234599,
        result_data: b"primes_computed:1229".to_vec(),
    };
    let result_pkt = Packet::new(0, 205, MessagePayload::TaskResult(result));
    let encoded = result_pkt.encode().expect("Encode task result");
    let decoded = Packet::decode(&encoded).expect("Decode task result");
    assert_eq!(result_pkt, decoded);
}

#[test]
fn test_task_id_deterministic_compute() {
    let author = NodeId::new([0x77; 32]);
    let payload = b"hello_world";
    let id1 = TaskId::compute(&author, 5, 1000, payload);
    let id2 = TaskId::compute(&author, 5, 1000, payload);
    let id3 = TaskId::compute(&author, 6, 1000, payload);

    assert_eq!(id1, id2);
    assert_ne!(id1, id3);
}

#[test]
fn test_hop_count_flag_helpers() {
    let flags = FLAG_GOSSIP;
    assert_eq!(get_hop_count(flags), 0);

    let updated = set_hop_count(flags, 5);
    assert_eq!(get_hop_count(updated), 5);
    assert_eq!(updated & FLAG_GOSSIP, FLAG_GOSSIP);
}

#[test]
fn test_packet_oversized_rejection() {
    let author = NodeId::new([0x01; 32]);
    let huge_payload = vec![0x42; MAX_PACKET_SIZE + 10];
    let data = TaskDataPayload {
        task_id: TaskId::new([0x03; 32]),
        author,
        priority: 1,
        created_at: 100,
        payload: huge_payload,
    };
    let pkt = Packet::new(0, 999, MessagePayload::TaskData(data));
    assert!(matches!(pkt.encode(), Err(WireError::PayloadTooLarge { .. })));
}

#[test]
fn test_packet_payload_length_mismatch() {
    let ping_pkt = Packet::new(0, 100, MessagePayload::Ping(PingPayload { nonce: 42 }));
    let mut encoded = ping_pkt.encode().expect("Encode ping packet");

    // Corrupt payload length header to be 100 instead of 8
    encoded[16] = 0;
    encoded[17] = 100;

    assert!(matches!(
        Packet::decode(&encoded),
        Err(WireError::PayloadLengthMismatch { header_len: 100, actual_len: 8 })
    ));
}
