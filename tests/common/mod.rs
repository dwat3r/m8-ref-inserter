pub fn wav_bytes(sample_rate: u32, frames: u32) -> Vec<u8> {
    let data_len = frames * 2;
    let mut bytes = b"RIFF".to_vec();
    bytes.extend((36 + data_len).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend([1, 0, 1, 0]);
    bytes.extend(sample_rate.to_le_bytes());
    bytes.extend((sample_rate * 2).to_le_bytes());
    bytes.extend([2, 0, 16, 0]);
    bytes.extend(b"data");
    bytes.extend(data_len.to_le_bytes());
    bytes.extend(vec![0u8; data_len as usize]);
    bytes
}
