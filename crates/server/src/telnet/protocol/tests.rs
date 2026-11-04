//! The decoder is exercised byte by byte, in pieces and with hostile input: it must give back
//! only data and a window size, refuse each unknown option once, and never grow.
#![allow(clippy::expect_used, clippy::assert_is_empty)]

use super::{
    DO, DONT, Decoded, Decoder, EC, EL, IAC, IP, OPENING, OPT_ECHO, OPT_NAWS, OPT_SGA, OPT_TTYPE,
    SB, SE, WILL, WONT, encode,
};

fn decode(bytes: &[u8]) -> Decoded {
    let mut decoder = Decoder::new();
    let mut out = Decoded::default();
    decoder.feed(bytes, &mut out);
    out
}

#[test]
fn plain_bytes_pass_through_and_line_ends_are_one_cr() {
    assert_eq!(decode(b"abc").data, b"abc");
    assert_eq!(decode(b"a\r\nb").data, b"a\rb");
    assert_eq!(decode(b"a\r\0b").data, b"a\rb");
    assert_eq!(decode(b"a\nb").data, b"a\rb");
    assert_eq!(decode(b"a\0b").data, b"ab", "a stray NUL is padding");
}

#[test]
fn a_crlf_split_between_reads_is_still_one_line_end() {
    let mut decoder = Decoder::new();
    let mut out = Decoded::default();
    decoder.feed(b"a\r", &mut out);
    decoder.feed(b"\nb", &mut out);
    assert_eq!(out.data, b"a\rb");
}

#[test]
fn an_escaped_iac_is_a_data_byte() {
    assert_eq!(decode(&[b'a', IAC, IAC, b'b']).data, [b'a', 0xFF, b'b']);
}

#[test]
fn naws_gives_the_window_size() {
    let out = decode(&[IAC, SB, OPT_NAWS, 0, 100, 0, 30, IAC, SE]);
    assert_eq!(out.window, Some((100, 30)));
    assert!(out.data.is_empty() && out.reply.is_empty());
}

#[test]
fn naws_with_a_doubled_ff_byte_and_split_reads() {
    // 255 columns: the 0xFF is doubled on the wire.
    let wire = [IAC, SB, OPT_NAWS, 0, IAC, IAC, 0, 24, IAC, SE];
    assert_eq!(decode(&wire).window, Some((255, 24)));
    let mut decoder = Decoder::new();
    let mut out = Decoded::default();
    for byte in wire {
        decoder.feed(&[byte], &mut out);
    }
    assert_eq!(out.window, Some((255, 24)));
}

#[test]
fn naws_of_the_wrong_length_or_for_another_option_is_ignored() {
    assert_eq!(decode(&[IAC, SB, OPT_NAWS, 0, 80, IAC, SE]).window, None);
    assert_eq!(
        decode(&[IAC, SB, OPT_NAWS, 0, 80, 0, 24, 0, IAC, SE]).window,
        None
    );
    assert_eq!(decode(&[IAC, SB, 24, 0, 80, 0, 24, IAC, SE]).window, None);
}

#[test]
fn a_huge_subnegotiation_is_skipped_and_the_stream_recovers() {
    let mut wire = vec![IAC, SB, OPT_NAWS];
    wire.extend(std::iter::repeat_n(7_u8, 100_000));
    wire.extend([IAC, SE]);
    wire.extend(b"after");
    let out = decode(&wire);
    assert_eq!(out.window, None);
    assert_eq!(out.data, b"after");
}

#[test]
fn a_subnegotiation_that_ends_wrongly_is_dropped_and_the_byte_is_a_command() {
    // IAC DO 200 inside an SB: the SB is abandoned and the DO is read as a command.
    let out = decode(&[IAC, SB, OPT_NAWS, 0, IAC, DO, 200, b'x']);
    assert_eq!(out.window, None);
    assert_eq!(out.reply, [IAC, WONT, 200]);
    assert_eq!(out.data, b"x");
}

#[test]
fn an_unknown_option_is_refused_once_however_often_it_is_asked() {
    let mut wire = Vec::new();
    for _ in 0..1000 {
        wire.extend([IAC, WILL, 200, IAC, DO, 201]);
    }
    let out = decode(&wire);
    assert_eq!(out.reply, [IAC, DONT, 200, IAC, WONT, 201]);
}

#[test]
fn the_options_we_asked_for_are_accepted_without_an_answer() {
    let out = decode(&[
        IAC, DO, OPT_ECHO, IAC, DO, OPT_SGA, IAC, WILL, OPT_SGA, IAC, WILL, OPT_NAWS,
    ]);
    assert!(out.reply.is_empty(), "{:?}", out.reply);
}

#[test]
fn refusals_and_withdrawals_get_no_answer() {
    let out = decode(&[
        IAC, WONT, 5, IAC, DONT, 5, IAC, WONT, OPT_NAWS, IAC, DONT, OPT_ECHO,
    ]);
    assert!(out.reply.is_empty());
}

#[test]
fn only_the_character_mode_commands_become_keys_and_the_rest_are_dropped() {
    let out = decode(&[IAC, IP, IAC, EC, IAC, EL]);
    assert_eq!(out.data, [0x03, 0x7f, 0x15]);
    // NOP, DM, BRK, AO, AYT, GA and undefined commands.
    for command in [241, 242, 243, 245, 246, 249, 0, 1, 100] {
        let out = decode(&[b'a', IAC, command, b'b']);
        assert_eq!(out.data, b"ab", "command {command}");
        assert!(out.reply.is_empty());
    }
}

#[test]
fn nothing_hostile_can_make_the_decoder_grow_or_answer_much() {
    // A long pseudo-random stream, as one read and in odd pieces.
    let mut seed = 0x1234_5678_u32;
    let wire: Vec<u8> = (0..200_000)
        .map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            seed.to_be_bytes()[0]
        })
        .collect();
    let mut decoder = Decoder::new();
    let mut out = Decoded::default();
    for piece in wire.chunks(7) {
        decoder.feed(piece, &mut out);
    }
    // At most one DONT and one WONT per option, three bytes each.
    assert!(out.reply.len() <= 2 * 256 * 3, "{} bytes", out.reply.len());
    assert!(out.data.len() <= wire.len());
    assert!(decoder.sb.len() <= 48);
}

#[test]
fn escape_sequences_in_data_stay_data_for_the_key_parser() {
    // Telnet does not touch ESC: arrow keys are for the key parser, not for us.
    assert_eq!(decode(b"\x1b[A").data, b"\x1b[A");
}

#[test]
fn encoding_doubles_iac_and_pads_a_bare_cr() {
    assert_eq!(encode(&[b'a', 0xFF, b'b']), [b'a', IAC, IAC, b'b']);
    assert_eq!(encode(b"a\r\nb"), b"a\r\nb");
    assert_eq!(encode(b"a\rb"), b"a\r\0b");
    assert_eq!(encode(b"a\r"), b"a\r\0");
    assert_eq!(encode(b"\x1b[2J"), b"\x1b[2J");
}

// ---- UI-1: the terminal type (RFC 1091), asked for only to choose a colour depth ----

#[test]
fn the_opening_asks_for_the_terminal_type_and_nothing_more_than_before() {
    assert!(OPENING.ends_with(&[IAC, DO, OPT_TTYPE]));
    assert_eq!(OPENING.len(), 15);
}

#[test]
fn a_client_that_will_send_its_terminal_type_is_asked_once() {
    let mut decoder = Decoder::new();
    let mut out = Decoded::default();
    decoder.feed(&[IAC, WILL, OPT_TTYPE], &mut out);
    assert_eq!(out.reply, [IAC, SB, OPT_TTYPE, 1, IAC, SE]);
    // Saying it again does not make us ask again (no loops).
    let mut again = Decoded::default();
    decoder.feed(&[IAC, WILL, OPT_TTYPE], &mut again);
    assert!(again.reply.is_empty());
}

#[test]
fn the_terminal_type_is_read_from_is_and_a_terminal_name_only() {
    let reply = |name: &[u8]| {
        let mut bytes = vec![IAC, SB, OPT_TTYPE, 0];
        bytes.extend_from_slice(name);
        bytes.extend_from_slice(&[IAC, SE]);
        decode(&bytes).terminal
    };
    assert_eq!(reply(b"xterm-256color").as_deref(), Some("xterm-256color"));
    assert_eq!(reply(b"XTERM-KITTY").as_deref(), Some("XTERM-KITTY"));
    // Not a terminal name: spaces, escapes, controls, empty, over 40 characters.
    assert_eq!(reply(b"x term"), None);
    assert_eq!(reply(b"\x1b[31m"), None);
    assert_eq!(reply(b""), None);
    assert_eq!(reply(&[b'a'; 41]), None);
    assert_eq!(reply(&[b'a'; 40]).as_deref(), Some(&*"a".repeat(40)));
    assert_eq!(
        reply(&[b'a'; 400]),
        None,
        "a long subnegotiation is skipped"
    );
    // `SEND` from a client is not an answer, and a reply for another option is not this one.
    assert_eq!(decode(&[IAC, SB, OPT_TTYPE, 1, IAC, SE]).terminal, None);
    assert_eq!(
        decode(&[IAC, SB, OPT_NAWS, 0, b'x', b'y', b'z', IAC, SE]).terminal,
        None
    );
}

#[test]
fn only_the_first_terminal_type_in_a_read_counts() {
    let mut bytes = vec![IAC, SB, OPT_TTYPE, 0];
    bytes.extend_from_slice(b"first");
    bytes.extend_from_slice(&[IAC, SE, IAC, SB, OPT_TTYPE, 0]);
    bytes.extend_from_slice(b"second");
    bytes.extend_from_slice(&[IAC, SE]);
    assert_eq!(decode(&bytes).terminal.as_deref(), Some("first"));
}
