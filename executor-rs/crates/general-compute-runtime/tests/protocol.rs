use general_compute_runtime::{
    MAX_PROTOCOL_FRAME_BYTES, ProtocolError, decode_frame, encode_frame,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Envelope {
    kind: String,
    value: u64,
}

#[test]
fn framed_json_round_trip_consumes_exactly_one_frame() {
    let first = encode_frame(
        &Envelope {
            kind: "request".into(),
            value: 42,
        },
        MAX_PROTOCOL_FRAME_BYTES,
    )
    .expect("first frame encodes");
    let second = encode_frame(
        &Envelope {
            kind: "cancel".into(),
            value: 7,
        },
        MAX_PROTOCOL_FRAME_BYTES,
    )
    .expect("second frame encodes");

    let mut stream = first.clone();
    stream.extend_from_slice(&second);
    let (decoded, consumed) =
        decode_frame::<Envelope>(&stream, MAX_PROTOCOL_FRAME_BYTES).expect("first frame decodes");

    assert_eq!(decoded.kind, "request");
    assert_eq!(decoded.value, 42);
    assert_eq!(consumed, first.len());

    let (decoded_second, consumed_second) =
        decode_frame::<Envelope>(&stream[consumed..], MAX_PROTOCOL_FRAME_BYTES)
            .expect("second frame decodes");
    assert_eq!(decoded_second.kind, "cancel");
    assert_eq!(consumed_second, second.len());
}

#[test]
fn protocol_rejects_oversized_payload_before_deserialization() {
    let oversized = (MAX_PROTOCOL_FRAME_BYTES as u32 + 1).to_be_bytes();
    let error = decode_frame::<Envelope>(&oversized, MAX_PROTOCOL_FRAME_BYTES)
        .expect_err("declared oversized frame must fail closed");
    assert_eq!(error, ProtocolError::PayloadTooLarge);
}

#[test]
fn protocol_rejects_truncated_header_and_payload() {
    assert_eq!(
        decode_frame::<Envelope>(&[0, 0, 0], MAX_PROTOCOL_FRAME_BYTES),
        Err(ProtocolError::Truncated)
    );

    let encoded = encode_frame(
        &Envelope {
            kind: "request".into(),
            value: 1,
        },
        MAX_PROTOCOL_FRAME_BYTES,
    )
    .expect("frame encodes");
    assert_eq!(
        decode_frame::<Envelope>(&encoded[..encoded.len() - 1], MAX_PROTOCOL_FRAME_BYTES),
        Err(ProtocolError::Truncated)
    );
}

#[test]
fn protocol_rejects_invalid_json_and_encode_size_overflow() {
    let invalid = [0, 0, 0, 2, b'{', b'!'];
    assert_eq!(
        decode_frame::<Envelope>(&invalid, MAX_PROTOCOL_FRAME_BYTES),
        Err(ProtocolError::InvalidJson)
    );

    let error = encode_frame(
        &Envelope {
            kind: "request".into(),
            value: 1,
        },
        1,
    )
    .expect_err("payload cap must apply before frame emission");
    assert_eq!(error, ProtocolError::PayloadTooLarge);
}
