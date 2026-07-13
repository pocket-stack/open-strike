//! Compile-time deterministic controller scripts used by Vita3K goldens.
//!
//! Entries are `frame:mask:lx:ly:rx:ry`; axes are optional and default to
//! 128. At any frame, the latest entry at or before it wins. This is a strict
//! superset of the PSP E2E format and makes dual-stick camera motion explicit.

use crate::input::PadSample;

#[derive(Clone, Debug, Default)]
pub struct CaptureScript {
    entries: Vec<(u32, PadSample)>,
}

impl CaptureScript {
    pub fn parse(source: &str) -> Self {
        let mut entries = source
            .split([',', ';'])
            .filter_map(parse_entry)
            .collect::<Vec<_>>();
        // Stable sort means a later duplicate frame in the source wins.
        entries.sort_by_key(|entry| entry.0);
        Self { entries }
    }

    pub fn sample(&self, frame: u32, fallback: PadSample) -> PadSample {
        self.entries
            .iter()
            .rev()
            .find(|(at, _)| *at <= frame)
            .map(|(_, sample)| *sample)
            .unwrap_or(fallback)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn parse_entry(source: &str) -> Option<(u32, PadSample)> {
    let mut fields = source.trim().split(':');
    let frame = parse_number(fields.next()?)?;
    let buttons = parse_number(fields.next()?)?;
    let axis = |value: Option<&str>| {
        value
            .and_then(parse_number)
            .map(|n| n.min(u8::MAX as u32) as u8)
            .unwrap_or(128)
    };
    Some((
        frame,
        PadSample {
            buttons,
            lx: axis(fields.next()),
            ly: axis(fields.next()),
            rx: axis(fields.next()),
            ry: axis(fields.next()),
        },
    ))
}

fn parse_number(source: &str) -> Option<u32> {
    let source = source.trim();
    if let Some(hex) = source
        .strip_prefix("0x")
        .or_else(|| source.strip_prefix("0X"))
    {
        u32::from_str_radix(hex, 16).ok()
    } else {
        source.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_entry_controls_each_frame() {
        let script = CaptureScript::parse("0:0,10:0x200:128:20,20:0:128:128:220:96");
        assert_eq!(script.sample(9, PadSample::default()).buttons, 0);
        assert_eq!(script.sample(10, PadSample::default()).buttons, 0x200);
        assert_eq!(script.sample(19, PadSample::default()).ly, 20);
        assert_eq!(script.sample(20, PadSample::default()).rx, 220);
        assert_eq!(script.sample(20, PadSample::default()).ry, 96);
    }

    #[test]
    fn axes_default_to_center_and_bad_entries_are_ignored() {
        let fallback = PadSample {
            buttons: 99,
            ..PadSample::default()
        };
        let script = CaptureScript::parse("bad,4:0x40");
        assert_eq!(script.sample(3, fallback), fallback);
        assert_eq!(script.sample(4, fallback).lx, 128);
        assert_eq!(script.sample(4, fallback).ry, 128);
    }
}
