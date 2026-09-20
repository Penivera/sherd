use sherd_desktop::daemon::protocol::{
    decode_line, encode_line, AutoOutcome, CapabilityReport, Event, LinkStatus, Request, Response,
    ServerMessage, StatusReport,
};

#[test]
fn test_request_encoding_decoding() {
    let requests = vec![
        Request::Auto,
        Request::Capability,
        Request::Status,
        Request::HotspotStart {
            ssid: "Sherd-Mesh-01".to_string(),
            key: "secretpass".to_string(),
        },
        Request::HotspotStop,
        Request::StationConnect {
            ssid: "Sherd-Mesh-01".to_string(),
            key: "secretpass".to_string(),
        },
        Request::StationDisconnect,
        Request::SendMessage {
            to: "node-2".to_string(),
            body: "hello mesh".to_string(),
        },
        Request::SendFile {
            to: "node-2".to_string(),
            path: "/tmp/data.bin".to_string(),
        },
    ];

    for req in requests {
        let line = encode_line(&req).expect("Failed to encode request");
        assert!(line.ends_with('\n'), "Line must be newline-terminated");
        let decoded: Request = decode_line(&line).expect("Failed to decode request");
        let re_encoded = encode_line(&decoded).expect("Failed to re-encode");
        assert_eq!(line, re_encoded);
    }
}

#[test]
fn test_response_and_event_server_messages() {
    let resp = Response::Auto(AutoOutcome::Hosting {
        ssid: "Sherd-Mesh-99".to_string(),
        uplink: Some("wlan0".to_string()),
    });
    let server_resp = ServerMessage::Response(resp);
    let line = encode_line(&server_resp).expect("Failed to encode ServerMessage::Response");
    assert!(line.ends_with('\n'));
    let decoded: ServerMessage = decode_line(&line).expect("Failed to decode ServerMessage");
    match decoded {
        ServerMessage::Response(Response::Auto(AutoOutcome::Hosting { ssid, uplink })) => {
            assert_eq!(ssid, "Sherd-Mesh-99");
            assert_eq!(uplink.as_deref(), Some("wlan0"));
        }
        _ => panic!("Unexpected decoded message variant"),
    }

    let event = Event::CapabilityChanged(CapabilityReport {
        level: "full".to_string(),
        detail: "dual band supported".to_string(),
        checked_via: "nl80211".to_string(),
    });
    let server_event = ServerMessage::Event(event);
    let line = encode_line(&server_event).expect("Failed to encode ServerMessage::Event");
    assert!(line.ends_with('\n'));
    let decoded_event: ServerMessage = decode_line(&line).expect("Failed to decode ServerMessage");
    match decoded_event {
        ServerMessage::Event(Event::CapabilityChanged(cap)) => {
            assert_eq!(cap.level, "full");
            assert_eq!(cap.detail, "dual band supported");
            assert_eq!(cap.checked_via, "nl80211");
        }
        _ => panic!("Unexpected decoded event variant"),
    }
}

#[test]
fn test_status_report_serialization() {
    let report = StatusReport {
        capability: CapabilityReport {
            level: "basic".to_string(),
            detail: "station only".to_string(),
            checked_via: "wpa_supplicant".to_string(),
        },
        hotspot: None,
        station: Some(LinkStatus {
            state: "connected".to_string(),
            ssid: Some("Sherd-Net".to_string()),
            interface: Some("wlan0".to_string()),
        }),
    };

    let line = encode_line(&report).expect("Failed to encode StatusReport");
    let decoded: StatusReport = decode_line(&line).expect("Failed to decode StatusReport");
    assert_eq!(decoded.capability.level, "basic");
    assert!(decoded.hotspot.is_none());
    assert_eq!(
        decoded.station.as_ref().unwrap().ssid.as_deref(),
        Some("Sherd-Net")
    );
}
