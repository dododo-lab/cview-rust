
//! `Tests/CViewPlayerTests/HLSManifestParserTests.swift` 포팅.
use super::*;

fn p() -> HlsManifestParser {
    HlsManifestParser::new()
}
fn master_base() -> Url {
    Url::parse("https://cdn.example.com/live/master.m3u8").unwrap()
}
fn media_base() -> Url {
    Url::parse("https://cdn.example.com/live/playlist.m3u8").unwrap()
}
fn master(c: &str) -> MasterPlaylist {
    p().parse_master_playlist(c, &master_base()).unwrap()
}
fn media(c: &str) -> MediaPlaylist {
    p().parse_media_playlist(c, &media_base()).unwrap()
}
fn variant(bw: i64, res: &str, name: Option<&str>) -> Variant {
    Variant {
        bandwidth: bw,
        average_bandwidth: None,
        resolution: res.into(),
        codecs: None,
        frame_rate: None,
        uri: Url::parse("https://example.com/v.m3u8").unwrap(),
        name: name.map(Into::into),
    }
}
fn last_component(u: &Url) -> String {
    u.path_segments()
        .and_then(|mut s| s.next_back().map(str::to_string))
        .unwrap_or_default()
}

#[test]
fn master_single_and_sorted() {
    let pl = master(
        "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=3000000,RESOLUTION=1280x720\n720p/stream.m3u8",
    );
    assert_eq!(pl.variants.len(), 1);
    assert_eq!(pl.variants[0].bandwidth, 3_000_000);
    assert_eq!(pl.variants[0].resolution, "1280x720");
    assert!(pl.variants[0].uri.as_str().contains("720p/stream.m3u8"));
    assert_eq!(pl.uri, master_base());

    let pl = master(
            "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1500000,RESOLUTION=854x480\n480p/stream.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080\n1080p/stream.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=3000000,RESOLUTION=1280x720\n720p/stream.m3u8",
        );
    let bws: Vec<_> = pl.variants.iter().map(|v| v.bandwidth).collect();
    assert_eq!(bws, vec![5_000_000, 3_000_000, 1_500_000]);
}

#[test]
fn master_full_attributes() {
    let pl = master("#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=5000000,AVERAGE-BANDWIDTH=4500000,RESOLUTION=1920x1080,CODECS=\"avc1.64001f,mp4a.40.2\",FRAME-RATE=30.0,NAME=\"1080p\"\n1080p/playlist.m3u8");
    let v = &pl.variants[0];
    assert_eq!(v.codecs.as_deref(), Some("avc1.64001f,mp4a.40.2"));
    assert_eq!(v.frame_rate, Some(30.0));
    assert_eq!(v.name.as_deref(), Some("1080p"));
    assert_eq!(v.average_bandwidth, Some(4_500_000));
}

#[test]
fn master_absolute_uri_and_errors() {
    let pl = master("#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080\nhttps://other-cdn.example.com/1080p/playlist.m3u8");
    assert_eq!(
        pl.variants[0].uri.as_str(),
        "https://other-cdn.example.com/1080p/playlist.m3u8"
    );
    assert!(p()
        .parse_master_playlist("Not a valid manifest", &master_base())
        .is_err());
    assert!(p()
        .parse_master_playlist("#EXTM3U\n", &master_base())
        .is_err());
    // 빈 줄/주석은 크래시하지 않는다.
    let pl = master("#EXTM3U\n## comment\n\n#EXT-X-STREAM-INF:BANDWIDTH=3000000,RESOLUTION=1280x720\n\n720p/stream.m3u8");
    assert_eq!(pl.variants.len(), 1);
}

#[test]
fn master_crlf() {
    let pl = master("#EXTM3U\r\n#EXT-X-STREAM-INF:BANDWIDTH=3000000,RESOLUTION=1280x720\r\n720p/stream.m3u8\r\n");
    assert_eq!(pl.variants.len(), 1);
    assert!(pl.variants[0].uri.as_str().ends_with("720p/stream.m3u8"));
}

#[test]
fn quality_labels() {
    assert_eq!(
        variant(5_000_000, "1920x1080", Some("Full HD")).quality_label(),
        "Full HD"
    );
    assert_eq!(
        variant(5_000_000, "1920x1080", None).quality_label(),
        "1080p"
    );
    assert_eq!(variant(3_000_000, "1280x720", None).quality_label(), "720p");
    assert_eq!(variant(1_500_000, "854x480", None).quality_label(), "480p");
    assert_eq!(variant(800_000, "640x360", None).quality_label(), "360p");
    assert_eq!(
        variant(2_500_000, "960x540", None).quality_label(),
        "2500kbps"
    );
    assert_eq!(
        variant(5_000_000, "1920x1080", None).id(),
        "5000000-1920x1080"
    );
    assert_eq!(
        variant(1000, "720x480", None),
        variant(1000, "720x480", None)
    );
    assert_ne!(
        variant(1000, "720x480", None),
        variant(2000, "1280x720", None)
    );
}

#[test]
fn media_basics() {
    let pl =
        media("#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:5.0,\nseg0.ts");
    assert_eq!(pl.target_duration, 6.0);
    let pl = media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:100\n#EXTINF:4.0,\nseg100.ts\n#EXTINF:4.0,\nseg101.ts\n#EXTINF:4.0,\nseg102.ts");
    assert_eq!(pl.media_sequence, 100);
    let ids: Vec<_> = pl.segments.iter().map(|s| s.id).collect();
    assert_eq!(ids, vec![100, 101, 102]);
}

#[test]
fn media_times() {
    let pl = media("#EXTM3U\n#EXT-X-TARGETDURATION:5\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:4.0,\nseg0.ts\n#EXTINF:3.5,\nseg1.ts\n#EXTINF:5.0,\nseg2.ts");
    let starts: Vec<_> = pl.segments.iter().map(|s| s.start_time).collect();
    assert_eq!(starts, vec![0.0, 4.0, 7.5]);
    assert!((pl.total_duration() - 12.5).abs() < 1e-3);
    assert!((pl.end_time() - 12.5).abs() < 1e-3);
    let empty = media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:0");
    assert!(empty.segments.is_empty());
    assert_eq!(empty.total_duration(), 0.0);
    assert_eq!(empty.end_time(), 0.0);
}

#[test]
fn media_endlist_discontinuity_invalid() {
    let pl = media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4.0,\nseg0.ts\n#EXT-X-ENDLIST");
    assert!(pl.is_end_list);
    let pl = media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4.0,\nseg0.ts");
    assert!(!pl.is_end_list);
    let pl = media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4.0,\nseg0.ts\n#EXT-X-DISCONTINUITY\n#EXTINF:4.0,\nseg1.ts");
    assert!(!pl.segments[0].discontinuity);
    assert!(pl.segments[1].discontinuity);
    assert!(p()
        .parse_media_playlist("This is not M3U8", &media_base())
        .is_err());
}

#[test]
fn llhls() {
    let pl = media(
        "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-PART-INF:PART-TARGET=0.5\n#EXTINF:4.0,\nseg0.ts",
    );
    assert!(pl.is_low_latency);
    assert_eq!(pl.part_target_duration, Some(0.5));

    let pl = media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:100\n#EXT-X-PART-INF:PART-TARGET=0.5\n#EXTINF:4.0,\nseg100.ts\n#EXT-X-PART:DURATION=0.5,URI=\"part101-0.ts\",INDEPENDENT=YES\n#EXT-X-PART:DURATION=0.5,URI=\"part101-1.ts\"\n#EXT-X-PART:DURATION=0.5,URI=\"part101-2.ts\",INDEPENDENT=YES");
    let ind: Vec<_> = pl.partial_segments.iter().map(|p| p.independent).collect();
    assert_eq!(ind, vec![true, false, true]);

    let pl = media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-SERVER-CONTROL:CAN-BLOCK-RELOAD=YES,PART-HOLD-BACK=1.5,CAN-SKIP-UNTIL=24.0,HOLD-BACK=6.0\n#EXTINF:4.0,\nseg0.ts");
    let sc = pl.server_control.unwrap();
    assert!(sc.can_block_reload);
    assert_eq!(sc.part_hold_back, Some(1.5));
    assert_eq!(sc.can_skip_until, Some(24.0));
    assert_eq!(sc.hold_back, Some(6.0));

    let pl = media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-PART-INF:PART-TARGET=0.5\n#EXTINF:4.0,\nseg0.ts\n#EXT-X-PRELOAD-HINT:TYPE=PART,URI=\"next-part.ts\"");
    let hint = pl.preload_hint.unwrap();
    assert_eq!(hint.hint_type, "PART");
    assert_eq!(last_component(&hint.uri), "next-part.ts");

    let pl = media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4.0,\nseg100.ts\n#EXT-X-PART:DURATION=0.5,URI=\"part101-0.ts\"");
    assert!(pl.is_low_latency);

    let pl =
        media("#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6.0,\nseg0.ts\n#EXTINF:6.0,\nseg1.ts");
    assert!(!pl.is_low_latency);
    assert!(pl.part_target_duration.is_none());
    assert!(pl.server_control.is_none());
    assert!(pl.preload_hint.is_none());
}

#[test]
fn pdt_and_byte_range() {
    let pl = media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-PROGRAM-DATE-TIME:2024-01-15T10:00:00.000Z\n#EXTINF:4.0,\nseg0.ts");
    assert!(pl.segments[0].program_date_time.is_some());
    let pl = media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-PROGRAM-DATE-TIME:2024-01-15T10:00:00+09:00\n#EXTINF:4.0,\nseg0.ts");
    assert!(pl.segments[0].program_date_time.is_some());
    let pl =
        media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-BYTERANGE:1000@0\n#EXTINF:4.0,\nseg0.ts");
    assert_eq!(
        pl.segments[0].byte_range,
        Some(ByteRange {
            length: 1000,
            offset: Some(0)
        })
    );
    let pl =
        media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-BYTERANGE:2048\n#EXTINF:4.0,\nseg0.ts");
    assert_eq!(
        pl.segments[0].byte_range,
        Some(ByteRange {
            length: 2048,
            offset: None
        })
    );
    assert_eq!(parse_byte_range("@"), None);
    assert_eq!(parse_byte_range(""), None);
}

const TOKEN_BASE: &str = "https://livecloud.pstatic.net/chzzk/lip2_kr/cflexnmss2u0001/warrpu9abc/720p/hdntl=exp=1790086400~acl=*%2Fwarrpu9abc%2F*~data=hdntl~hmac=0f0f/chunklist.m3u8";

fn token_media(c: &str) -> MediaPlaylist {
    p().parse_media_playlist(c, &Url::parse(TOKEN_BASE).unwrap())
        .unwrap()
}

#[test]
fn map_without_byte_range_keeps_token_path() {
    let pl = token_media("#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:10\n#EXT-X-MEDIA-SEQUENCE:100\n#EXT-X-MAP:URI=\"720p_init.m4v\"\n#EXT-X-PROGRAM-DATE-TIME:2026-09-25T12:00:00.000Z\n#EXTINF:2.000,\n720p_0_0_100.m4v\n#EXT-X-PROGRAM-DATE-TIME:2026-09-25T12:00:02.000Z\n#EXTINF:2.000,\n720p_0_0_101.m4v");
    assert_eq!(pl.segments.len(), 2);
    for s in &pl.segments {
        let init = s.init_segment.as_ref().unwrap();
        assert!(init.byte_range.is_none());
        assert!(init.uri.as_str().ends_with(
            "/720p/hdntl=exp=1790086400~acl=*%2Fwarrpu9abc%2F*~data=hdntl~hmac=0f0f/720p_init.m4v"
        ));
        assert!(!s.is_gap);
    }
}

#[test]
fn map_with_byte_range_and_changes() {
    let pl = token_media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MAP:URI=\"main.mp4\",BYTERANGE=\"720@0\"\n#EXTINF:4.0,\n#EXT-X-BYTERANGE:50000@720\nmain.mp4");
    let init = pl.segments[0].init_segment.as_ref().unwrap();
    assert_eq!(
        init.byte_range,
        Some(ByteRange {
            length: 720,
            offset: Some(0)
        })
    );
    assert_eq!(last_component(&init.uri), "main.mp4");
    assert_eq!(
        pl.segments[0].byte_range,
        Some(ByteRange {
            length: 50000,
            offset: Some(720)
        })
    );

    let pl = token_media("#EXTM3U\n#EXT-X-TARGETDURATION:10\n#EXT-X-MEDIA-SEQUENCE:10\n#EXT-X-MAP:URI=\"a_init.m4v\"\n#EXTINF:2.0,\na_10.m4v\n#EXTINF:2.0,\na_11.m4v\n#EXT-X-DISCONTINUITY\n#EXT-X-MAP:URI=\"b_init.m4v\"\n#EXTINF:2.0,\nb_12.m4v\n#EXTINF:2.0,\nb_13.m4v");
    let names: Vec<_> = pl
        .segments
        .iter()
        .map(|s| last_component(&s.init_segment.as_ref().unwrap().uri))
        .collect();
    assert_eq!(
        names,
        ["a_init.m4v", "a_init.m4v", "b_init.m4v", "b_init.m4v"]
    );
    assert!(pl.segments[2].discontinuity);
}

#[test]
fn missing_or_invalid_map_and_gap() {
    let pl = token_media("#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6.0,\nseg0.ts");
    assert!(pl.segments[0].init_segment.is_none());
    let pl = token_media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MAP:URI=\"good_init.mp4\"\n#EXTINF:2.0,\ns0.m4v\n#EXT-X-MAP:BYTERANGE=\"100@0\"\n#EXTINF:2.0,\ns1.m4v");
    let init = pl.segments[1].init_segment.as_ref().unwrap();
    assert_eq!(last_component(&init.uri), "good_init.mp4");
    assert!(init.byte_range.is_none());

    let pl = token_media("#EXTM3U\n#EXT-X-TARGETDURATION:10\n#EXT-X-MAP:URI=\"init.m4v\"\n#EXTINF:2.0,\ns0.m4v\n#EXT-X-GAP\n#EXTINF:2.0,\ns1.m4v\n#EXTINF:2.0,\ns2.m4v");
    let gaps: Vec<_> = pl.segments.iter().map(|s| s.is_gap).collect();
    assert_eq!(gaps, vec![false, true, false]);
    assert!(pl
        .segments
        .iter()
        .all(|s| last_component(&s.init_segment.as_ref().unwrap().uri) == "init.m4v"));
}

#[test]
fn llhls_yields_only_complete_segments() {
    let pl = token_media("#EXTM3U\n#EXT-X-VERSION:9\n#EXT-X-TARGETDURATION:10\n#EXT-X-SERVER-CONTROL:CAN-BLOCK-RELOAD=YES,PART-HOLD-BACK=1.5,CAN-SKIP-UNTIL=12.0\n#EXT-X-PART-INF:PART-TARGET=0.5\n#EXT-X-MEDIA-SEQUENCE:200\n#EXT-X-DISCONTINUITY-SEQUENCE:3\n#EXT-X-MAP:URI=\"1080p_init.m4v\"\n#EXT-X-PROGRAM-DATE-TIME:2026-09-25T12:00:00.000Z\n#EXTINF:2.000,\n1080p_0_0_200.m4v\n#EXT-X-PART:DURATION=0.500,URI=\"1080p_0_0_201.0.m4v\",INDEPENDENT=YES\n#EXT-X-PART:DURATION=0.500,URI=\"1080p_0_0_201.1.m4v\"\n#EXT-X-PART:DURATION=0.500,URI=\"1080p_0_0_201.2.m4v\"\n#EXT-X-PART:DURATION=0.500,URI=\"1080p_0_0_201.3.m4v\"\n#EXT-X-PROGRAM-DATE-TIME:2026-09-25T12:00:02.000Z\n#EXTINF:2.000,\n1080p_0_0_201.m4v\n#EXT-X-PART:DURATION=0.500,URI=\"1080p_0_0_202.0.m4v\",INDEPENDENT=YES\n#EXT-X-PART:DURATION=0.500,URI=\"1080p_0_0_202.1.m4v\"\n#EXT-X-PRELOAD-HINT:TYPE=PART,URI=\"1080p_0_0_202.2.m4v\"\n#EXT-X-RENDITION-REPORT:URI=\"../720p/chunklist.m3u8\",LAST-MSN=202,LAST-PART=1");
    assert!(pl.is_low_latency);
    let ids: Vec<_> = pl.segments.iter().map(|s| s.id).collect();
    assert_eq!(ids, vec![200, 201]);
    assert_eq!(
        pl.segments.iter().map(|s| s.duration).collect::<Vec<_>>(),
        vec![2.0, 2.0]
    );
    assert_eq!(pl.segments[1].parts.len(), 4);
    assert_eq!(pl.partial_segments.len(), 6);
    assert_eq!(
        last_component(&pl.preload_hint.unwrap().uri),
        "1080p_0_0_202.2.m4v"
    );
    assert!(pl.segments.iter().all(|s| s.discontinuity_sequence == 3));
}

#[test]
fn key_handling() {
    let segs = token_media("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:7\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"\n#EXTINF:4.0,\ns7.ts\n#EXT-X-KEY:METHOD=FUTURE-CIPHER,URI=\"other.bin\"\n#EXTINF:4.0,\ns8.ts\n#EXT-X-KEY:METHOD=AES-128,URI=\"key2.bin\",IV=0x000000000000000000000000000000FF\n#EXTINF:4.0,\ns9.ts\n#EXT-X-KEY:METHOD=NONE\n#EXTINF:4.0,\ns10.ts").segments;
    let mut derived = [0u8; 16];
    derived[15] = 7;
    assert_eq!(segs[0].encryption_info.as_ref().unwrap().iv, Some(derived));
    assert_eq!(
        last_component(&segs[0].encryption_info.as_ref().unwrap().uri),
        "key.bin"
    );
    assert_eq!(
        last_component(&segs[1].encryption_info.as_ref().unwrap().uri),
        "key.bin"
    );
    assert_eq!(segs[1].encryption_info.as_ref().unwrap().iv.unwrap()[15], 8);
    assert_eq!(
        segs[2].encryption_info.as_ref().unwrap().iv.unwrap()[15],
        0xFF
    );
    assert!(segs[3].encryption_info.is_none());
}
