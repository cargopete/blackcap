//! A tiny tracker DSL. `song!{}` captures raw lane strings as `'static` data;
//! they're parsed once into an event timeline at `init()` via [`TrackerSong::compile`]
//! — off the audio thread, so there's no hot-path cost (and a malformed pattern
//! is a friendly `init()` error rather than a proc-macro span).

/// A pitch as a MIDI note number (A4 = 69 = 440 Hz).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Note {
    pub midi: i32,
}

impl Note {
    pub fn from_midi(midi: i32) -> Self {
        Self { midi }
    }

    pub fn hz(&self) -> f32 {
        440.0 * 2f32.powf((self.midi - 69) as f32 / 12.0)
    }

    /// Parse scientific names: `c4` = 60, `a4` = 69. Accidentals: `#`/`s`
    /// (sharp) or `b` (flat) — e.g. `eb5`, `f#3`, `bb2`. Negative octaves OK.
    pub fn parse(s: &str) -> Result<Note, String> {
        let s = s.trim();
        let mut chars = s.chars().peekable();
        let letter = chars.next().ok_or_else(|| "empty note".to_string())?;
        let base = match letter.to_ascii_lowercase() {
            'c' => 0,
            'd' => 2,
            'e' => 4,
            'f' => 5,
            'g' => 7,
            'a' => 9,
            'b' => 11,
            _ => return Err(format!("bad note letter '{letter}' in \"{s}\"")),
        };
        let mut semis = base;
        match chars.peek() {
            Some('#') | Some('s') => {
                semis += 1;
                chars.next();
            }
            Some('b') => {
                semis -= 1;
                chars.next();
            }
            _ => {}
        }
        let oct_str: String = chars.collect();
        let octave: i32 = oct_str
            .parse()
            .map_err(|_| format!("bad octave in note \"{s}\""))?;
        Ok(Note {
            midi: (octave + 1) * 12 + semis,
        })
    }
}

/// One tracker cell (= one row in a lane).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cell {
    /// A pitch — note-on in a melodic lane.
    Note(Note),
    /// `x` (normal) or `X` (accented) — a hit, or gate-open.
    Hit { accent: bool },
    /// `.` — a ghost/quiet cell, or "sustain/continue" in a melodic lane.
    Ghost,
    /// `-` — silence / rest / gate-closed.
    Off,
}

/// A resolved event in the song timeline.
#[derive(Clone, Copy, Debug)]
pub struct Event {
    pub frame: u64,
    pub lane: &'static str,
    pub cell: Cell,
}

/// Tokenise a lane string into one [`Cell`] per row.
///
/// Whitespace separates tokens (and is otherwise free for visual bar grouping).
/// A token containing a digit is one note cell (`eb5`); any other token is read
/// char-by-char (`X-x-` → four cells), so symbol lanes and note lanes share one
/// syntax.
fn parse_lane(s: &str) -> Result<Vec<Cell>, String> {
    let mut cells = Vec::new();
    for token in s.split_whitespace() {
        if token.chars().any(|c| c.is_ascii_digit()) {
            cells.push(Cell::Note(Note::parse(token)?));
        } else {
            for ch in token.chars() {
                cells.push(match ch {
                    'x' => Cell::Hit { accent: false },
                    'X' => Cell::Hit { accent: true },
                    '.' => Cell::Ghost,
                    '-' => Cell::Off,
                    other => return Err(format!("unexpected symbol '{other}'")),
                });
            }
        }
    }
    Ok(cells)
}

type LaneDef = (&'static str, &'static str);
type PatternDef = (&'static str, &'static [LaneDef]);

/// Raw, `const`-constructible song: lane strings plus tempo and a play order.
pub struct TrackerSong {
    bpm: f32,
    rows_per_beat: u32,
    patterns: &'static [PatternDef],
    sequence: &'static [&'static str],
}

impl TrackerSong {
    pub const fn from_parts(
        bpm: f32,
        rows_per_beat: u32,
        patterns: &'static [PatternDef],
        sequence: &'static [&'static str],
    ) -> Self {
        Self {
            bpm,
            rows_per_beat,
            patterns,
            sequence,
        }
    }

    pub fn frames_per_row(&self, sample_rate: u32) -> f64 {
        60.0 * sample_rate as f64 / (self.bpm as f64 * self.rows_per_beat as f64)
    }

    /// Parse all lanes and resolve row positions into absolute frames.
    pub fn compile(&self, sample_rate: u32) -> Result<CompiledSong, String> {
        let fpr = self.frames_per_row(sample_rate);
        let mut events = Vec::new();
        let mut cursor_row: u64 = 0;

        for &pat_name in self.sequence {
            let lanes = self
                .patterns
                .iter()
                .find(|p| p.0 == pat_name)
                .ok_or_else(|| format!("sequence references unknown pattern \"{pat_name}\""))?
                .1;

            let mut pattern_rows = 0u64;
            for &(lane_name, lane_str) in lanes {
                let cells = parse_lane(lane_str)
                    .map_err(|e| format!("pattern \"{pat_name}\" lane \"{lane_name}\": {e}"))?;
                pattern_rows = pattern_rows.max(cells.len() as u64);
                for (row, cell) in cells.into_iter().enumerate() {
                    let frame = ((cursor_row + row as u64) as f64 * fpr) as u64;
                    events.push(Event {
                        frame,
                        lane: lane_name,
                        cell,
                    });
                }
            }
            cursor_row += pattern_rows;
        }

        events.sort_by_key(|e| e.frame);
        Ok(CompiledSong {
            events,
            duration: (cursor_row as f64 * fpr) as u64,
        })
    }
}

/// Parsed timeline: events sorted by frame, queryable per render block.
pub struct CompiledSong {
    events: Vec<Event>,
    duration: u64,
}

impl CompiledSong {
    pub fn duration_frames(&self) -> u64 {
        self.duration
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// Events whose frame is in `[start, start + num)`. The cartridge decides
    /// what each lane's cells mean (note-on, gate, trigger, …).
    pub fn events_in_range(&self, start: u64, num: u64) -> impl Iterator<Item = Event> + '_ {
        let end = start.saturating_add(num);
        let lo = self.events.partition_point(|e| e.frame < start);
        self.events[lo..]
            .iter()
            .take_while(move |e| e.frame < end)
            .copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_parsing() {
        assert_eq!(Note::parse("a4").unwrap().midi, 69);
        assert_eq!(Note::parse("c4").unwrap().midi, 60);
        assert_eq!(Note::parse("c5").unwrap().midi, 72);
        assert_eq!(Note::parse("eb5").unwrap().midi, 75);
        assert_eq!(Note::parse("f#3").unwrap().midi, 54);
        assert_eq!(Note::parse("fs3").unwrap().midi, 54);
        assert_eq!(Note::parse("bb2").unwrap().midi, 46); // B flat, not B-then-flat-letter
        assert_eq!(Note::parse("a1").unwrap().midi, 33); // drop-A, 55 Hz
        assert!(Note::parse("h5").is_err());
        assert!(Note::parse("c").is_err());
    }

    #[test]
    fn a4_is_440() {
        assert!((Note::parse("a4").unwrap().hz() - 440.0).abs() < 0.01);
        assert!((Note::parse("a1").unwrap().hz() - 55.0).abs() < 0.01);
    }

    #[test]
    fn lane_tokeniser_splits_notes_and_symbols() {
        // Notes are whole tokens; symbol runs are per-char.
        let cells = parse_lane("a1 . . -  X-x-").unwrap();
        assert_eq!(cells.len(), 8);
        assert!(matches!(cells[0], Cell::Note(_)));
        assert!(matches!(cells[1], Cell::Ghost));
        assert!(matches!(cells[3], Cell::Off));
        assert!(matches!(cells[4], Cell::Hit { accent: true }));
        assert!(matches!(cells[5], Cell::Off));
        assert!(matches!(cells[6], Cell::Hit { accent: false }));
    }

    #[test]
    fn compile_places_events_on_frames() {
        let song = TrackerSong::from_parts(
            120.0,
            4,
            &[("p", &[("lead", "c4 - - -"), ("hat", "x x x x")])],
            &["p", "p"],
        );
        // 120 bpm, 4 rows/beat, 48k → 6000 frames/row.
        let fpr = song.frames_per_row(48_000);
        assert!((fpr - 6000.0).abs() < 0.5);

        let compiled = song.compile(48_000).unwrap();
        // 2 patterns × 4 rows = 8 rows; duration = 8 × 6000.
        assert_eq!(compiled.duration_frames(), 48_000);

        // First block: row 0 of both lanes (c4 note-on + hat hit).
        let first: Vec<_> = compiled.events_in_range(0, 1024).collect();
        assert!(first.iter().any(|e| e.lane == "lead" && matches!(e.cell, Cell::Note(_))));
        assert!(first.iter().any(|e| e.lane == "hat" && matches!(e.cell, Cell::Hit { .. })));
    }

    #[test]
    fn compile_rejects_unknown_pattern() {
        let song = TrackerSong::from_parts(120.0, 4, &[("p", &[("a", "x")])], &["missing"]);
        assert!(song.compile(48_000).is_err());
    }
}

/// Build a [`TrackerSong`] from a tracker-style block. See module docs.
///
/// ```ignore
/// const SONG: TrackerSong = song! {
///     tempo: 174;
///     rows_per_beat: 4;
///     pattern "a" {
///         lead: "c5 - eb5 g5  bb5 - g5 eb5";
///         hat:  "x x x x  x x x x";
///     }
///     sequence: [a, a];
/// };
/// ```
#[macro_export]
macro_rules! song {
    (
        tempo: $bpm:expr;
        rows_per_beat: $rpb:expr;
        $( pattern $name:literal { $( $lane:ident : $pat:literal ; )+ } )+
        sequence: [ $( $seq:ident ),+ $(,)? ] $(;)?
    ) => {
        $crate::song::TrackerSong::from_parts(
            $bpm as f32,
            $rpb as u32,
            &[ $( ( $name, &[ $( (stringify!($lane), $pat) ),+ ] ) ),+ ],
            &[ $( stringify!($seq) ),+ ],
        )
    };
}
