use m8_file_parser::Song;
use m8_file_parser::writer::Writer;

#[test]
fn song_roundtrip_is_byte_identical() {
    let bytes = std::fs::read("tests/fixtures/V6_6EMPTY.m8s").unwrap();
    let song = Song::read(&mut bytes.as_slice()).unwrap();
    let mut writer = Writer::new(bytes.clone());
    song.write(&mut writer).unwrap();
    assert_eq!(writer.finish(), bytes);
}
