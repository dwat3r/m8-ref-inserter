use m8_file_parser::{Instrument, SamplePlayMode, Sampler};

const SAMPLER_TEMPLATE: &[u8] = include_bytes!("../assets/sampler.m8i");
const REF_INSTRUMENT_SLOT: u8 = 0x7F;
const FIRST_SLICE: u8 = 1;

pub fn blank_sampler(sample_path: &str) -> Result<Sampler, String> {
    let template = Instrument::read(&mut &SAMPLER_TEMPLATE[..]).map_err(|e| format!("{e:?}"))?;
    match template.instrument {
        Instrument::Sampler(sampler) => Ok(Sampler {
            number: REF_INSTRUMENT_SLOT,
            name: "REFERENCE".to_string(),
            sample_path: sample_path.to_string(),
            play_mode: SamplePlayMode::FWD,
            slice: FIRST_SLICE,
            ..sampler
        }),
        _ => Err("assets/sampler.m8i is not a Sampler instrument".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use m8_file_parser::SamplePlayMode;

    const PATH: &str = "/Samples/ref/song_1.wav";

    #[test]
    fn blank_sampler_is_named_reference() {
        assert_eq!(blank_sampler(PATH).unwrap().name, "REFERENCE");
    }

    #[test]
    fn blank_sampler_points_at_sample_path() {
        assert_eq!(blank_sampler(PATH).unwrap().sample_path, PATH);
    }

    #[test]
    fn blank_sampler_plays_forward() {
        assert_eq!(blank_sampler(PATH).unwrap().play_mode, SamplePlayMode::FWD);
    }

    #[test]
    fn blank_sampler_selects_first_slice() {
        assert_eq!(blank_sampler(PATH).unwrap().slice, 1);
    }

    #[test]
    fn blank_sampler_is_instrument_7f() {
        assert_eq!(blank_sampler(PATH).unwrap().number, 0x7F);
    }
}
