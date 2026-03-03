use claude_chat::matrix::sender::chunk_message;

#[test]
fn chunks_long_message() {
    let msg = "a".repeat(10000);
    let chunks = chunk_message(&msg, 4000);
    assert_eq!(chunks.len(), 3);
    for chunk in &chunks {
        assert!(chunk.len() <= 4000);
    }
}

#[test]
fn short_message_is_single_chunk() {
    let chunks = chunk_message("hello", 4000);
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0], "hello");
}

#[test]
fn empty_message_is_single_chunk() {
    let chunks = chunk_message("", 4000);
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0], "");
}

#[test]
fn exact_boundary_message() {
    let msg = "a".repeat(4000);
    let chunks = chunk_message(&msg, 4000);
    assert_eq!(chunks.len(), 1);
}
