//! The telnet wire protocol, as much of it as a full-screen terminal session needs (RFC 854,
//! 855, 857, 858, 1073): a decoder for what the client sends and an encoder for what we send.
//!
//! **Everything the client sends is hostile.** The decoder is a small state machine with fixed
//! memory: a subnegotiation is buffered up to [`MAX_SUBNEGOTIATION`] bytes and the rest is
//! skipped, a refusal for an option is sent at most once per option (so a client cannot make us
//! chat forever), and only two things come out of it: plain data bytes and a window size. Every
//! other command (`IP`, `AO`, `AYT`, `EC`, `EL`, `GA`, `BRK`, `NOP`, `DM`), every option we did
//! not ask for and every subnegotiation but NAWS is consumed and dropped. No byte of the
//! protocol reaches the key parser, so none can pass for a key.
//!
//! Why this is not a crate: the subset is a few dozen lines, the crates on offer are either
//! synchronous or pull in a framework, and what matters here is the exact bound on every state,
//! which is easier to show in code that is this small (D85).

/// Interpret As Command: the escape byte of the protocol.
pub(super) const IAC: u8 = 255;
/// Refuse to perform an option.
const DONT: u8 = 254;
/// Ask the peer to perform an option.
const DO: u8 = 253;
/// Refuse to ask for an option.
const WONT: u8 = 252;
/// Offer to perform an option.
const WILL: u8 = 251;
/// Start of a subnegotiation.
const SB: u8 = 250;
/// Interrupt Process, Erase Character and Erase Line.
const IP: u8 = 244;
const EC: u8 = 247;
const EL: u8 = 248;
/// End of a subnegotiation.
const SE: u8 = 240;

/// Echo (RFC 857): the server echoes, so the client must not.
pub(super) const OPT_ECHO: u8 = 1;
/// Suppress go-ahead (RFC 858): full duplex, character at a time.
pub(super) const OPT_SGA: u8 = 3;
/// Negotiate about window size (RFC 1073).
pub(super) const OPT_NAWS: u8 = 31;
/// Terminal type (RFC 1091): asked for only to choose how many colours to draw (UI-1).
pub(super) const OPT_TTYPE: u8 = 24;
/// Subnegotiation codes of the terminal-type option.
const TTYPE_IS: u8 = 0;
const TTYPE_SEND: u8 = 1;

/// Most bytes of one subnegotiation that are kept. NAWS needs 4; the rest of a longer one is
/// skipped, not stored.
const MAX_SUBNEGOTIATION: usize = 48;

const CR: u8 = b'\r';
const LF: u8 = b'\n';
const NUL: u8 = 0;

/// What we send first: we echo (so the client does not), both sides suppress go-ahead (full
/// duplex, character mode), and we ask for the window size and the terminal type. Nothing else
/// is ever offered or requested. The terminal type picks the colour depth only (UI-1); the glyph
/// tier is the member's own setting.
pub(super) const OPENING: [u8; 15] = [
    IAC, WILL, OPT_ECHO, IAC, WILL, OPT_SGA, IAC, DO, OPT_SGA, IAC, DO, OPT_NAWS, IAC, DO,
    OPT_TTYPE,
];

/// Where the decoder is in the byte stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Data,
    /// After a CR: a following LF or NUL belongs to it (RFC 854: CR LF and CR NUL are one end
    /// of line) and is dropped.
    AfterCr,
    /// After IAC.
    Command,
    /// After IAC and WILL, WONT, DO or DONT: the next byte is the option.
    Option(u8),
    /// Inside IAC SB, reading the option number.
    SbOption,
    /// Inside a subnegotiation of this option: collecting its data.
    Sb(u8),
    /// Inside a subnegotiation, after an IAC.
    SbIac(u8),
}

/// What one call to [`Decoder::feed`] produced.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Decoded {
    /// Plain data from the client, telnet framing removed. End of line is a single CR.
    pub(super) data: Vec<u8>,
    /// The latest window size reported (columns, rows), unclamped and unvalidated.
    pub(super) window: Option<(u16, u16)>,
    /// Bytes to send back: refusals of options we do not use, and the request for the terminal
    /// type once the client agrees to send it.
    pub(super) reply: Vec<u8>,
    /// The terminal type the client reported (RFC 1091 `IS`): at most 40 characters of a
    /// terminal name's alphabet, or nothing. Only ever used to choose a colour depth.
    pub(super) terminal: Option<String>,
}

/// The decoder for one connection. Fixed memory whatever the client sends.
#[derive(Debug)]
pub(super) struct Decoder {
    state: State,
    sb: Vec<u8>,
    /// The current subnegotiation grew past the bound: it is skipped to its end and ignored.
    sb_over: bool,
    /// Options already refused with `DONT` (the client said `WILL`).
    refused_will: [bool; 256],
    /// Options already refused with `WONT` (the client said `DO`).
    refused_do: [bool; 256],
    /// The terminal type was asked for (once per connection).
    asked_terminal: bool,
}

impl Decoder {
    pub(super) fn new() -> Self {
        Self {
            state: State::Data,
            sb: Vec::with_capacity(MAX_SUBNEGOTIATION),
            sb_over: false,
            refused_will: [false; 256],
            refused_do: [false; 256],
            asked_terminal: false,
        }
    }

    /// Decode `bytes` into `out`.
    pub(super) fn feed(&mut self, bytes: &[u8], out: &mut Decoded) {
        for &byte in bytes {
            self.step(byte, out);
        }
    }

    fn step(&mut self, byte: u8, out: &mut Decoded) {
        match self.state {
            State::Data => self.data(byte, out),
            State::AfterCr => {
                self.state = State::Data;
                if byte != LF && byte != NUL {
                    self.data(byte, out);
                }
            }
            State::Command => match byte {
                IAC => {
                    // An escaped 0xFF is a data byte.
                    out.data.push(IAC);
                    self.state = State::Data;
                }
                WILL | WONT | DO | DONT => self.state = State::Option(byte),
                SB => {
                    self.sb.clear();
                    self.sb_over = false;
                    self.state = State::SbOption;
                }
                // A client in character mode turns its Ctrl-C, erase and kill keys into these
                // commands. Each becomes the one key it stands for, not a command of ours.
                IP => self.key(0x03, out),
                EC => self.key(0x7f, out),
                EL => self.key(0x15, out),
                // NOP, DM, BRK, AO, AYT, GA and anything undefined: dropped.
                _ => self.state = State::Data,
            },
            State::Option(verb) => {
                self.state = State::Data;
                self.option(verb, byte, out);
            }
            State::SbOption => self.state = State::Sb(byte),
            State::Sb(option) => {
                if byte == IAC {
                    self.state = State::SbIac(option);
                } else {
                    self.keep(byte);
                }
            }
            State::SbIac(option) => match byte {
                SE => {
                    self.end_subnegotiation(option, out);
                    self.state = State::Data;
                }
                IAC => {
                    self.keep(IAC);
                    self.state = State::Sb(option);
                }
                // IAC followed by anything else inside SB is malformed: abandon the
                // subnegotiation and read the byte as a command.
                _ => {
                    self.sb.clear();
                    self.state = State::Command;
                    self.step(byte, out);
                }
            },
        }
    }

    fn key(&mut self, byte: u8, out: &mut Decoded) {
        out.data.push(byte);
        self.state = State::Data;
    }

    /// Keep one byte of a subnegotiation, up to the bound.
    fn keep(&mut self, byte: u8) {
        if self.sb.len() < MAX_SUBNEGOTIATION {
            self.sb.push(byte);
        } else {
            self.sb_over = true;
        }
    }

    fn data(&mut self, byte: u8, out: &mut Decoded) {
        match byte {
            IAC => self.state = State::Command,
            CR => {
                out.data.push(CR);
                self.state = State::AfterCr;
            }
            // A bare LF is a line end too.
            LF => out.data.push(CR),
            // A stray NUL is padding.
            NUL => {}
            _ => out.data.push(byte),
        }
    }

    /// A negotiation request from the client. What we asked for ourselves (echo, go-ahead and
    /// window size) is accepted without an answer; anything else is refused once.
    fn option(&mut self, verb: u8, option: u8, out: &mut Decoded) {
        let slot = usize::from(option);
        match verb {
            // The client will send its terminal type: ask for it, once.
            WILL if option == OPT_TTYPE => {
                if !self.asked_terminal {
                    self.asked_terminal = true;
                    out.reply
                        .extend_from_slice(&[IAC, SB, OPT_TTYPE, TTYPE_SEND, IAC, SE]);
                }
            }
            WILL if !matches!(option, OPT_SGA | OPT_NAWS) => {
                refuse(&mut self.refused_will[slot], DONT, option, out);
            }
            DO if !matches!(option, OPT_ECHO | OPT_SGA) => {
                refuse(&mut self.refused_do[slot], WONT, option, out);
            }
            // A refusal or a withdrawal needs no answer (answering them is how loops start).
            _ => {}
        }
    }

    fn end_subnegotiation(&mut self, option: u8, out: &mut Decoded) {
        // NAWS is exactly four bytes: width and height, 16 bits each, big-endian. A bare
        // 0xFF in it is doubled on the wire and was collapsed above.
        if option == OPT_NAWS
            && !self.sb_over
            && let [w_hi, w_lo, h_hi, h_lo] = self.sb.as_slice()
        {
            out.window = Some((
                u16::from_be_bytes([*w_hi, *w_lo]),
                u16::from_be_bytes([*h_hi, *h_lo]),
            ));
        }
        // Terminal type: `IS` and the name. Anything else, or a name outside a terminal name's
        // alphabet or over 40 characters, is ignored; the first answer wins.
        if option == OPT_TTYPE
            && !self.sb_over
            && out.terminal.is_none()
            && let Some((&TTYPE_IS, name)) = self.sb.split_first()
            && !name.is_empty()
            && name.len() <= 40
            && name
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'+'))
        {
            out.terminal = Some(name.iter().map(|b| char::from(*b)).collect());
        }
        self.sb.clear();
    }
}

fn refuse(already: &mut bool, verb: u8, option: u8, out: &mut Decoded) {
    if !*already {
        *already = true;
        out.reply.extend_from_slice(&[IAC, verb, option]);
    }
}

/// Encode screen output for the wire: a literal 0xFF is doubled, and a CR that is not followed
/// by LF gets a NUL after it (RFC 854), so the client never reads a bare CR as a command.
pub(super) fn encode(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + bytes.len() / 16);
    let mut iter = bytes.iter().copied().peekable();
    while let Some(byte) = iter.next() {
        match byte {
            IAC => out.extend_from_slice(&[IAC, IAC]),
            CR => {
                out.push(CR);
                if iter.peek() != Some(&LF) {
                    out.push(NUL);
                }
            }
            _ => out.push(byte),
        }
    }
    out
}

#[cfg(test)]
mod fuzz;
#[cfg(test)]
mod tests;
