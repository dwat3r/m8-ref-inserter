const FORMAT_PCM: u16 = 1;
const FORMAT_EXTENSIBLE: u16 = 0xFFFE;
const BEATS_PER_SLICE: f64 = 16.0;
const SECONDS_PER_MINUTE: f64 = 60.0;
const RIFF_HEADER_SIZE: usize = 12;
const CHUNK_HEADER_SIZE: usize = 8;

pub struct Wav {
    pub sample_rate: u32,
    pub block_align: u16,
    pub fmt: Vec<u8>,
    pub data: Vec<u8>,
}

fn chunk_size(body: &[u8], offset: usize) -> Option<usize> {
    body.get(offset + 4..offset + CHUNK_HEADER_SIZE)
        .map(|size| u32::from_le_bytes(size.try_into().unwrap()) as usize)
}

fn chunks(body: &[u8]) -> impl Iterator<Item = (&[u8], &[u8])> {
    std::iter::successors(Some(0usize), move |&offset| {
        let size = chunk_size(body, offset)?;
        Some(offset + CHUNK_HEADER_SIZE + size + size % 2)
    })
    .map_while(move |offset| {
        let size = chunk_size(body, offset)?;
        let payload = body.get(offset + CHUNK_HEADER_SIZE..offset + CHUNK_HEADER_SIZE + size)?;
        Some((&body[offset..offset + 4], payload))
    })
}

pub fn parse(bytes: &[u8]) -> Result<Wav, String> {
    if bytes.len() < RIFF_HEADER_SIZE || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a RIFF/WAVE file".to_string());
    }
    let body = &bytes[RIFF_HEADER_SIZE..];
    let find = |id: &[u8; 4]| {
        chunks(body)
            .find(|(chunk_id, _)| *chunk_id == id.as_slice())
            .map(|(_, payload)| payload.to_vec())
    };
    let fmt = find(b"fmt ").ok_or("missing fmt chunk")?;
    let data = find(b"data").ok_or("missing data chunk")?;
    if fmt.len() < 16 {
        return Err("fmt chunk too short".to_string());
    }
    let format = u16::from_le_bytes([fmt[0], fmt[1]]);
    if format != FORMAT_PCM && format != FORMAT_EXTENSIBLE {
        return Err("not a PCM wav, convert it first".to_string());
    }
    let block_align = u16::from_le_bytes([fmt[12], fmt[13]]);
    if block_align == 0 {
        return Err("fmt chunk has zero block align".to_string());
    }
    Ok(Wav {
        sample_rate: u32::from_le_bytes(fmt[4..8].try_into().unwrap()),
        block_align,
        fmt,
        data,
    })
}

pub fn slice_frames(sample_rate: u32, bpm: f32) -> u32 {
    (BEATS_PER_SLICE * SECONDS_PER_MINUTE * f64::from(sample_rate) / f64::from(bpm)).round() as u32
}

pub fn slice_count(wav: &Wav, slice_frames: u32) -> usize {
    let frames = wav.data.len() / usize::from(wav.block_align);
    frames.div_ceil(slice_frames as usize)
}

fn chunk(id: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let padding = vec![0u8; payload.len() % 2];
    [id.as_slice(), &(payload.len() as u32).to_le_bytes(), payload, &padding].concat()
}

fn cue_point(id: u32, start_frame: u32) -> Vec<u8> {
    let zero = 0u32.to_le_bytes();
    [id.to_le_bytes(), zero, *b"data", zero, zero, start_frame.to_le_bytes()].concat()
}

pub fn with_cue_points(wav: &Wav, slice_frames: u32, slices: usize) -> Vec<u8> {
    let points: Vec<u8> = (0..slices as u32)
        .flat_map(|slice| cue_point(slice, slice * slice_frames))
        .collect();
    let cue_payload = [(slices as u32).to_le_bytes().as_slice(), &points].concat();
    let body = [
        b"WAVE".to_vec(),
        chunk(b"fmt ", &wav.fmt),
        chunk(b"data", &wav.data),
        chunk(b"cue ", &cue_payload),
    ]
    .concat();
    [b"RIFF".as_slice(), &(body.len() as u32).to_le_bytes(), &body].concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SLICE_FRAMES: u32 = 352800;

    fn wav_bytes(frames: u32) -> Vec<u8> {
        let data_len = frames * 2;
        let mut bytes = b"RIFF".to_vec();
        bytes.extend((36 + data_len).to_le_bytes());
        bytes.extend(b"WAVEfmt ");
        bytes.extend(16u32.to_le_bytes());
        bytes.extend([1, 0, 1, 0]);
        bytes.extend(44100u32.to_le_bytes());
        bytes.extend(88200u32.to_le_bytes());
        bytes.extend([2, 0, 16, 0]);
        bytes.extend(b"data");
        bytes.extend(data_len.to_le_bytes());
        bytes.extend(vec![0u8; data_len as usize]);
        bytes
    }

    fn cue_chunk_offset(wav: &[u8]) -> usize {
        wav.windows(4).position(|w| w == b"cue ").unwrap()
    }

    fn u32_at(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    fn sliced(frames: u32, slices: usize) -> Vec<u8> {
        let wav = parse(&wav_bytes(frames)).unwrap();
        with_cue_points(&wav, SLICE_FRAMES, slices)
    }

    #[test]
    fn slice_frames_at_120_bpm_is_8_seconds() {
        assert_eq!(slice_frames(44100, 120.0), SLICE_FRAMES);
    }

    #[test]
    fn slice_count_counts_partial_tail() {
        let wav = parse(&wav_bytes(SLICE_FRAMES + 1)).unwrap();
        assert_eq!(slice_count(&wav, SLICE_FRAMES), 2);
    }

    #[test]
    fn slice_count_of_exact_multiple_has_no_extra_slice() {
        let wav = parse(&wav_bytes(SLICE_FRAMES * 2)).unwrap();
        assert_eq!(slice_count(&wav, SLICE_FRAMES), 2);
    }

    #[test]
    fn parse_reads_sample_rate() {
        assert_eq!(parse(&wav_bytes(10)).unwrap().sample_rate, 44100);
    }

    #[test]
    fn cue_chunk_holds_one_point_per_slice() {
        let out = sliced(SLICE_FRAMES * 2, 2);
        assert_eq!(u32_at(&out, cue_chunk_offset(&out) + 8), 2);
    }

    #[test]
    fn cue_chunk_size_covers_count_and_points() {
        let out = sliced(SLICE_FRAMES * 2, 2);
        assert_eq!(u32_at(&out, cue_chunk_offset(&out) + 4), 4 + 24 * 2);
    }

    #[test]
    fn cue_points_have_zero_based_ids() {
        let out = sliced(SLICE_FRAMES * 2, 2);
        assert_eq!(u32_at(&out, cue_chunk_offset(&out) + 12 + 24), 1);
    }

    #[test]
    fn cue_point_sample_offset_is_slice_start_frame() {
        let out = sliced(SLICE_FRAMES * 2, 2);
        assert_eq!(u32_at(&out, cue_chunk_offset(&out) + 12 + 24 + 20), SLICE_FRAMES);
    }

    #[test]
    fn cue_points_refer_to_the_data_chunk() {
        let out = sliced(SLICE_FRAMES * 2, 1);
        let point = cue_chunk_offset(&out) + 12;
        assert_eq!(&out[point + 8..point + 12], b"data");
    }

    #[test]
    fn cue_chunk_follows_data_chunk() {
        let out = sliced(SLICE_FRAMES * 2, 1);
        let data = out.windows(4).position(|w| w == b"data").unwrap();
        assert!(cue_chunk_offset(&out) > data);
    }

    #[test]
    fn riff_size_covers_whole_file() {
        let out = sliced(SLICE_FRAMES * 2, 2);
        assert_eq!(u32_at(&out, 4) as usize, out.len() - 8);
    }

    #[test]
    fn audio_data_survives_cue_embedding() {
        let original = parse(&wav_bytes(SLICE_FRAMES * 2)).unwrap();
        let out = with_cue_points(&original, SLICE_FRAMES, 2);
        assert_eq!(parse(&out).unwrap().data, original.data);
    }

    #[test]
    fn non_wav_is_rejected() {
        assert!(parse(b"ID3 not a wav").is_err());
    }
}
