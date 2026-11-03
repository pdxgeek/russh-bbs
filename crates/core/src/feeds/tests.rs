//! Rules of feeds that need no storage and no network: the address rules, the HTML reduction,
//! the parser against hostile documents, the dates, and the read-only rule of a feed board.
#![allow(clippy::unwrap_used, clippy::panic, clippy::too_many_lines)]

use crate::access::{Held, Permission, Scope, Slug, Snapshot, Source, Via};
use crate::forums::{BoardId, BoardRow, rules};
use crate::sanitize::RESERVED_CHROME;
use crate::store::{AccountId, UnixSeconds};

use super::html::to_plain_text;
use super::out::rfc2822;
use super::parse::{self, MAX_DEPTH, MAX_ITEMS, ParseError, parse};
use super::poll::is_due;
use super::{FeedId, FeedRow, FeedUrl, Outcome, UrlError};

const NOW: i64 = 1_800_000_000;

fn now() -> UnixSeconds {
    UnixSeconds::from_raw(NOW)
}

// ---- addresses ---------------------------------------------------------------------------

#[test]
fn an_address_must_be_http_or_https_with_a_host_and_no_credentials() {
    assert!(FeedUrl::parse("https://example.org/feed.xml", false).is_ok());
    assert_eq!(
        FeedUrl::parse("http://example.org/feed.xml", false),
        Err(UrlError::NotHttps)
    );
    assert!(FeedUrl::parse("http://example.org/feed.xml", true).is_ok());
    for bad in [
        "ftp://example.org/x",
        "file:///etc/passwd",
        "gopher://example.org",
        "javascript:alert(1)",
        "data:text/xml,<rss/>",
    ] {
        assert!(FeedUrl::parse(bad, true).is_err(), "{bad}");
    }
    assert_eq!(
        FeedUrl::parse("https://user:pw@example.org/", false),
        Err(UrlError::Credentials)
    );
    assert_eq!(
        FeedUrl::parse("https://user@example.org/", false),
        Err(UrlError::Credentials)
    );
}

#[test]
fn control_characters_and_oversized_addresses_are_malformed() {
    for bad in [
        "https://exam\tple.org/",
        "https://example.org/\nHost: evil",
        "https://example.org/a b",
        "https://example.org/\u{0}",
        "",
        "   ",
    ] {
        assert_eq!(
            FeedUrl::parse(bad, true),
            Err(UrlError::Malformed),
            "{bad:?}"
        );
    }
    let long = format!("https://example.org/{}", "a".repeat(2100));
    assert_eq!(FeedUrl::parse(&long, true), Err(UrlError::Malformed));
}

#[test]
fn numeric_host_tricks_are_normalised_to_the_address_they_mean() {
    // The guard judges the parsed host, so these must come out as the literal addresses.
    for (raw, expected) in [
        ("https://2130706433/", "127.0.0.1"),
        ("https://0x7f.1/", "127.0.0.1"),
        ("https://0177.0.0.1/", "127.0.0.1"),
        ("https://127.1/", "127.0.0.1"),
        ("https://[::ffff:127.0.0.1]/", "[::ffff:7f00:1]"),
    ] {
        let url = FeedUrl::parse(raw, false).unwrap();
        assert_eq!(url.url().host_str(), Some(expected), "{raw}");
    }
}

#[test]
fn a_redirect_target_is_resolved_and_checked_like_an_address() {
    let base = FeedUrl::parse("https://example.org/a/feed.xml", false).unwrap();
    assert_eq!(
        base.join("/other.xml", false).unwrap().as_str(),
        "https://example.org/other.xml"
    );
    assert_eq!(
        base.join("http://example.org/x", false),
        Err(UrlError::NotHttps)
    );
    assert!(base.join("//evil.example/x", false).is_ok());
    assert!(base.join("javascript:alert(1)", false).is_err());
    assert!(base.join("https://u:p@evil.example/", false).is_err());
    assert!(base.join("/a\r\nb", false).is_err());
}

// ---- HTML ----------------------------------------------------------------------------------

#[test]
fn html_becomes_text_with_the_links_listed() {
    let got = to_plain_text(
        "<p>Hello <b>world</b> &amp; <a href=\"https://a.example/x?y=1&amp;z=2\">a link</a>.</p>\
         <script>alert('x')</script><style>p{color:red}</style><!-- hidden -->\
         <ul><li>one</li><li>two</li></ul><img src=x onerror=alert(1)>\
         <a href=\"javascript:alert(1)\">bad</a> <a href='http://b.example/'>ok</a>",
        8,
    );
    assert_eq!(
        got.text,
        "Hello world & a link [1].\n- one\n- two\nbad ok [2]"
    );
    assert_eq!(
        got.links,
        ["https://a.example/x?y=1&z=2", "http://b.example/"]
    );
    assert!(!got.text.contains('<') && !got.text.contains("alert"));
}

#[test]
fn html_edge_cases_never_panic_and_never_keep_markup() {
    for input in [
        "<",
        "<<<<>>>>",
        "<a href=",
        "<a href=\"unterminated",
        "<script>never closed",
        "&",
        "&#xZZ;",
        "&#99999999999;",
        "&#0;",
        "a < b and c > d",
        "<!-- unterminated",
        "<![CDATA[x]]>",
        "<?php echo 1; ?>",
        &"<div>".repeat(5_000),
        &"&amp;".repeat(5_000),
    ] {
        let got = to_plain_text(input, 4);
        assert!(!got.text.contains("<script"), "{input:?}");
    }
    assert_eq!(to_plain_text("a < b and c > d", 1).text, "a < b and c > d");
    // Numeric entities become the character, or the replacement for a bad code.
    assert_eq!(
        to_plain_text("&#65;&#x42;&#99999999999;", 1).text,
        "AB&#99999999999;"
    );
}

#[test]
fn only_the_link_limit_many_links_are_listed() {
    let html: String = (0..20)
        .map(|i| format!("<a href=\"https://e.example/{i}\">l{i}</a> "))
        .collect::<Vec<_>>()
        .concat();
    let got = to_plain_text(&html, 3);
    assert_eq!(got.links.len(), 3);
    assert!(got.text.contains("l0 [1]") && !got.text.contains("l5 ["));
}

// ---- parsing hostile documents ----------------------------------------------------------

fn rss(items: &str) -> String {
    format!(
        "<?xml version=\"1.0\"?><rss version=\"2.0\"><channel><title>Feed</title>\
         <link>https://example.org/</link><description>d</description>{items}</channel></rss>"
    )
}

fn item(title: &str, body: &str) -> String {
    format!(
        "<item><title>{title}</title><link>https://example.org/p</link>\
         <guid>g-{title}</guid><description><![CDATA[{body}]]></description>\
         <pubDate>Mon, 02 Jan 2006 15:04:05 GMT</pubDate></item>"
    )
}

fn assert_clean(text: &str) {
    for c in text.chars() {
        assert!(
            !(c.is_control() && c != '\n' && c != '\t'),
            "control character {c:?} in {text:?}"
        );
        assert!(
            !RESERVED_CHROME.contains(&c),
            "reserved character {c:?} in {text:?}"
        );
        assert!(
            !('\u{E000}'..='\u{F8FF}').contains(&c) && !('\u{F0000}'..='\u{10FFFF}').contains(&c),
            "private-use character {c:?} in {text:?}"
        );
        assert!(
            !matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200B}' | '\u{FEFF}'),
            "invisible or bidi character {c:?} in {text:?}"
        );
    }
}

#[test]
fn a_well_formed_rss_document_becomes_items() {
    let xml = rss(&format!(
        "{}{}",
        item(
            "First",
            "<p>Hello <a href=\"https://a.example/\">there</a></p>"
        ),
        item("Second", "plain text")
    ));
    let got = parse(xml.as_bytes(), now()).unwrap();
    assert_eq!(got.title.as_str(), "Feed");
    assert_eq!(got.items.len(), 2);
    let first = got
        .items
        .iter()
        .find(|i| i.subject.as_str() == "First")
        .unwrap();
    assert!(
        first.body.as_str().contains("Hello there [1]"),
        "{}",
        first.body.as_str()
    );
    assert!(first.body.as_str().contains("Link: https://example.org/p"));
    assert!(first.body.as_str().contains("[1] https://a.example/"));
    assert_eq!(first.at.as_raw(), 1_136_214_245);
    assert_ne!(first.key_hash, first.content_hash);
    assert_eq!(first.key_hash.len(), 64);
}

#[test]
fn an_atom_document_is_read_too() {
    let xml = "<?xml version=\"1.0\"?><feed xmlns=\"http://www.w3.org/2005/Atom\">\
        <title>Atom feed</title><id>urn:x</id><updated>2024-01-01T00:00:00Z</updated>\
        <entry><title>Entry</title><id>urn:e1</id><updated>2024-01-02T00:00:00Z</updated>\
        <author><name>Ann</name></author><link href=\"https://example.org/e1\"/>\
        <content type=\"html\">&lt;p&gt;Body &lt;b&gt;text&lt;/b&gt;&lt;/p&gt;</content></entry></feed>";
    let got = parse(xml.as_bytes(), now()).unwrap();
    assert_eq!(got.items.len(), 1);
    let body = got.items[0].body.as_str();
    assert!(body.starts_with("By Ann"), "{body}");
    assert!(body.contains("Body text"), "{body}");
    assert!(!body.contains('<'));
}

#[test]
fn escape_sequences_c1_controls_and_markers_in_every_field_are_neutralised() {
    let evil = "\u{1b}[31mred\u{1b}[0m\u{1b}]0;title\u{7}\u{9b}31m\u{85}\u{200b}\u{202e}rtl\u{e000}\u{f8ff}\
                \u{27e6}\u{27e7}\u{2299}\u{229b}\u{270e}\u{26a0}\u{feff}\u{0}";
    let evil_xml = evil.replace('\u{0}', ""); // NUL is not legal XML
    let xml = format!(
        "<?xml version=\"1.0\"?><rss version=\"2.0\"><channel><title>{evil_xml}</title>\
         <item><title>{evil_xml}</title><link>https://example.org/{evil_xml}</link>\
         <guid>{evil_xml}</guid><author>{evil_xml}@example.org ({evil_xml})</author>\
         <description><![CDATA[{evil_xml} <a href=\"https://e.example/{evil_xml}\">{evil_xml}</a>]]></description>\
         </item></channel></rss>"
    );
    // Some of these characters are not legal in XML 1.0; the parser may refuse the document or
    // pass the rest. Either is acceptable; what matters is that nothing unclean gets through.
    if let Ok(got) = parse(xml.as_bytes(), now()) {
        assert_clean(got.title.as_str());
        for item in &got.items {
            assert_clean(item.subject.as_str());
            assert_clean(item.body.as_str());
        }
    }
    // The same text through a document that is legal XML (entities, not raw bytes).
    let entities = "&#x1b;[31mred&#x9b;31m&#x85;&#x200b;&#x202e;rtl&#xe000;&#xf8ff;&#x27e6;&#x27e7;&#x2299;&#x229b;&#x270e;&#x26a0;&#xfeff;";
    let body = format!("{evil} {entities}");
    let xml = rss(&item("Title", &body));
    let got = parse(xml.as_bytes(), now()).unwrap();
    for item in &got.items {
        assert_clean(item.subject.as_str());
        assert_clean(item.body.as_str());
    }
    // And the html layer: entities that decode to controls or markers.
    let xml = rss(&item(
        "T",
        "&#27;[31m&#x9b;&#xe000;&#x27e6;x&#x27e7; &#x202e;",
    ));
    let got = parse(xml.as_bytes(), now()).unwrap();
    for item in &got.items {
        assert_clean(item.body.as_str());
    }
}

#[test]
fn a_huge_body_is_capped_not_stored_whole() {
    let big = "word ".repeat(200_000);
    let xml = rss(&item("Big", &big));
    let got = parse(xml.as_bytes(), now()).unwrap();
    assert_eq!(got.items.len(), 1);
    assert!(got.items[0].body.chars() <= 8_000);
    assert!(got.items[0].body.lines() <= 200);
    // And a body of a very large number of short lines.
    let lines = "x\n".repeat(100_000);
    let xml = rss(&item("Lines", &format!("<pre>{lines}</pre>")));
    let got = parse(xml.as_bytes(), now()).unwrap();
    assert!(got.items[0].body.lines() <= 200);
}

#[test]
fn a_billion_laughs_document_is_refused_by_its_document_type() {
    let xml = "<?xml version=\"1.0\"?><!DOCTYPE lolz [\
        <!ENTITY lol \"lol\">\
        <!ENTITY lol2 \"&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;\">\
        <!ENTITY lol3 \"&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;\">\
        <!ENTITY lol4 \"&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;\">\
        ]><rss version=\"2.0\"><channel><title>&lol4;</title></channel></rss>";
    assert_eq!(parse(xml.as_bytes(), now()), Err(ParseError::DocType));
    // An external entity is the same door.
    let xxe = "<?xml version=\"1.0\"?><!DOCTYPE x [<!ENTITY e SYSTEM \"file:///etc/passwd\">]>\
        <rss version=\"2.0\"><channel><title>&e;</title></channel></rss>";
    assert_eq!(parse(xxe.as_bytes(), now()), Err(ParseError::DocType));
    // Even a bare declaration with no entities is refused: no feed needs one.
    assert_eq!(
        parse(
            b"<!DOCTYPE rss><rss version=\"2.0\"><channel/></rss>",
            now()
        ),
        Err(ParseError::DocType)
    );
}

#[test]
fn deep_nesting_and_token_floods_are_refused() {
    let deep = format!(
        "<rss version=\"2.0\"><channel>{}{}</channel></rss>",
        "<a>".repeat(MAX_DEPTH + 5),
        "</a>".repeat(MAX_DEPTH + 5)
    );
    assert_eq!(parse(deep.as_bytes(), now()), Err(ParseError::TooComplex));
    // A megabyte of opening tags: refused for depth without recursing.
    let bomb = format!("<rss>{}", "<a>".repeat(300_000));
    assert_eq!(parse(bomb.as_bytes(), now()), Err(ParseError::TooComplex));
    // Many tokens at a legal depth.
    let flat = format!(
        "<rss version=\"2.0\"><channel>{}</channel></rss>",
        "<x/>".repeat(parse::MAX_EVENTS)
    );
    assert_eq!(parse(flat.as_bytes(), now()), Err(ParseError::TooComplex));
}

#[test]
fn things_that_are_not_feeds_are_refused() {
    for input in [
        &b""[..],
        b"not xml at all",
        b"<html><body>hello</body></html>",
        b"<rss><channel><title>unclosed",
        b"{\"version\":\"https://jsonfeed.org/version/1\",\"items\":[]}",
        &[0xff, 0xfe, 0x00, 0x01],
    ] {
        assert!(parse(input, now()).is_err(), "{input:?}");
    }
}

#[test]
fn at_most_the_newest_items_are_kept_and_they_are_oldest_first() {
    let items: String = (0..MAX_ITEMS + 30)
        .map(|i| {
            format!(
                "<item><title>t{i}</title><guid>g{i}</guid><description>b{i}</description>\
                 <pubDate>Mon, 02 Jan 2006 15:{:02}:{:02} GMT</pubDate></item>",
                (i / 60) % 60,
                i % 60
            )
        })
        .collect::<Vec<_>>()
        .concat();
    let got = parse(rss(&items).as_bytes(), now()).unwrap();
    assert_eq!(got.items.len(), MAX_ITEMS);
    assert!(got.items.windows(2).all(|w| w[0].at <= w[1].at));
}

#[test]
fn a_date_in_the_future_or_missing_is_the_time_it_was_read() {
    let xml = rss(
        "<item><title>future</title><guid>f</guid><description>x</description>\
         <pubDate>Mon, 02 Jan 2090 00:00:00 GMT</pubDate></item>\
         <item><title>none</title><guid>n</guid><description>y</description></item>\
         <item><title>ancient</title><guid>a</guid><description>z</description>\
         <pubDate>Mon, 02 Jan 1960 00:00:00 GMT</pubDate></item>",
    );
    let got = parse(xml.as_bytes(), now()).unwrap();
    for item in &got.items {
        assert!(item.at.as_raw() >= 0 && item.at.as_raw() <= NOW);
    }
    let by = |s: &str| {
        got.items
            .iter()
            .find(|i| i.subject.as_str() == s)
            .unwrap()
            .at
            .as_raw()
    };
    assert_eq!(by("future"), NOW);
    assert_eq!(by("none"), NOW);
    assert_eq!(by("ancient"), 0);
}

#[test]
fn identical_items_hash_alike_and_different_ones_do_not() {
    let a = parse(rss(&item("Same", "body")).as_bytes(), now()).unwrap();
    let b = parse(rss(&item("Same", "body")).as_bytes(), now()).unwrap();
    assert_eq!(a.items[0].key_hash, b.items[0].key_hash);
    assert_eq!(a.items[0].content_hash, b.items[0].content_hash);
    let c = parse(rss(&item("Other", "body")).as_bytes(), now()).unwrap();
    assert_ne!(a.items[0].key_hash, c.items[0].key_hash);
    assert_ne!(a.items[0].content_hash, c.items[0].content_hash);
}

#[test]
fn links_that_are_not_web_addresses_are_not_listed() {
    let xml = rss(
        "<item><title>t</title><link>javascript:alert(1)</link><guid>g</guid>\
         <description><![CDATA[<a href=\"file:///etc/passwd\">x</a> \
         <a href=\"data:text/html,hi\">y</a> <a href=\"https://ok.example/\">z</a>]]></description></item>",
    );
    let got = parse(xml.as_bytes(), now()).unwrap();
    let body = got.items[0].body.as_str();
    assert!(!body.contains("javascript") && !body.contains("file:") && !body.contains("data:"));
    assert!(body.contains("https://ok.example/"));
}

// ---- schedule, dates, read-only -------------------------------------------------------

fn row(failures: u32, last: Option<i64>, interval: u32) -> FeedRow {
    FeedRow {
        id: FeedId::from_raw(1),
        board: BoardId::from_raw(1),
        board_name: "f".into(),
        url: "https://example.org/".into(),
        allow_http: false,
        interval_secs: interval,
        last_attempt_at: last.map(UnixSeconds::from_raw),
        last_success_at: None,
        last_outcome: Some(Outcome::Ok),
        failures,
        public: false,
    }
}

#[test]
fn a_feed_is_due_when_new_or_after_its_interval_and_backs_off_when_failing() {
    let at = |t: i64| UnixSeconds::from_raw(t);
    assert!(is_due(&row(0, None, 3_600), at(0)));
    assert!(!is_due(&row(0, Some(1_000), 3_600), at(1_000 + 3_599)));
    assert!(is_due(&row(0, Some(1_000), 3_600), at(1_000 + 3_600)));
    // Each failure doubles the wait.
    assert!(!is_due(&row(1, Some(1_000), 3_600), at(1_000 + 7_199)));
    assert!(is_due(&row(1, Some(1_000), 3_600), at(1_000 + 7_200)));
    assert!(is_due(&row(3, Some(1_000), 3_600), at(1_000 + 28_800)));
    // At most 64 times the interval however long it has failed, and never more than a week.
    assert!(is_due(&row(1_000, Some(1_000), 3_600), at(1_000 + 230_400)));
    assert!(!is_due(
        &row(1_000, Some(1_000), 3_600),
        at(1_000 + 230_399)
    ));
    assert!(is_due(
        &row(1_000, Some(1_000), 86_400),
        at(1_000 + 604_800)
    ));
    assert!(!is_due(
        &row(1_000, Some(1_000), 86_400),
        at(1_000 + 604_799)
    ));
}

#[test]
fn rss_dates_are_rfc_822() {
    assert_eq!(
        rfc2822(UnixSeconds::from_raw(0)),
        "Thu, 01 Jan 1970 00:00:00 +0000"
    );
    assert_eq!(
        rfc2822(UnixSeconds::from_raw(1_136_214_245)),
        "Mon, 02 Jan 2006 15:04:05 +0000"
    );
    assert_eq!(
        rfc2822(UnixSeconds::from_raw(951_782_400)),
        "Tue, 29 Feb 2000 00:00:00 +0000"
    );
    assert_eq!(
        rfc2822(UnixSeconds::from_raw(-5)),
        "Thu, 01 Jan 1970 00:00:00 +0000"
    );
}

fn member_holds() -> Snapshot {
    let held = [
        Permission::ForumList,
        Permission::ForumRead,
        Permission::ForumPost,
        Permission::ForumReply,
    ]
    .into_iter()
    .map(|permission| Held {
        permission,
        scope: Scope::Global,
        source: Source::Role {
            role: Slug::MEMBER,
            rank: 10,
            via: Via::Account,
        },
    })
    .collect();
    Snapshot::new(Some(AccountId::from_raw(1)), held)
}

#[test]
fn a_feed_board_refuses_post_and_reply_whatever_the_session_holds() {
    let mut board = BoardRow {
        id: BoardId::from_raw(7),
        name: "News".into(),
        description: String::new(),
        restricted: false,
        threads: 0,
        feed: true,
    };
    let ann = member_holds();
    assert!(rules::can(&ann, Permission::ForumList, &board));
    assert!(rules::can(&ann, Permission::ForumRead, &board));
    assert!(!rules::can(&ann, Permission::ForumPost, &board));
    assert!(!rules::can(&ann, Permission::ForumReply, &board));
    // The same session on an ordinary board may.
    board.feed = false;
    assert!(rules::can(&ann, Permission::ForumPost, &board));
    assert!(rules::can(&ann, Permission::ForumReply, &board));
    // Even a session scoped to the board, even one holding forum.manage and forum.moderate.
    board.feed = true;
    let resource = board.id.resource().unwrap();
    let mut held = member_holds().held().to_vec();
    for permission in [
        Permission::ForumPost,
        Permission::ForumReply,
        Permission::ForumManage,
    ] {
        held.push(Held {
            permission,
            scope: Scope::Resource(resource),
            source: Source::Allow { via: Via::Account },
        });
        held.push(Held {
            permission,
            scope: Scope::Global,
            source: Source::Allow { via: Via::Account },
        });
    }
    let staff = Snapshot::new(Some(AccountId::from_raw(2)), held);
    assert!(!rules::can(&staff, Permission::ForumPost, &board));
    assert!(!rules::can(&staff, Permission::ForumReply, &board));
}
