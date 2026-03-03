use claude_chat::matrix::client::derive_session_id;

#[test]
fn session_id_derived_from_room_alias() {
    assert_eq!(derive_session_id("#nixos-agent:matrix.pin"), "nixos-agent");
    assert_eq!(derive_session_id("nixos-agent:matrix.pin"), "nixos-agent");
    assert_eq!(derive_session_id("nixos-agent"), "nixos-agent");
}
