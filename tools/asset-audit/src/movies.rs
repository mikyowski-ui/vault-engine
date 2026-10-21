//! SPEC-012 corpus checks against the oracle tables: every `.MVE` summary
//! (file hash, frame size, chunk/frame counts, timer, audio format, PCM
//! hash, block and opcode histograms), every show-frame (push range, device
//! palette, pixels, and the post-fade palette for movies with a fade file),
//! every track-0 audio opcode, every fade effect, and every `.SVE` entry
//! with the frame that draws it.
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    path::Path,
};
use vault_formats::{
    LocalDirProvider, MAX_ASSET_BYTES, Vfs,
    moviefx::{self, FadeEffect, FadeHook},
    mve::{self, Event, Movie},
    sve::{self, SubtitleTrack},
};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn h16(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))[..16].to_owned()
}
/// Tab-separated rows after the header line. The oracle is user-supplied,
/// so a row with fewer than `columns` fields is an error, not a panic.
fn table(path: &Path, columns: usize) -> Result<Vec<Vec<String>>, Box<dyn Error>> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut rows = Vec::new();
    for (index, line) in text.lines().enumerate().skip(1) {
        if line.is_empty() {
            continue;
        }
        let row: Vec<String> = line.split('\t').map(str::to_owned).collect();
        if row.len() < columns {
            return Err(format!(
                "{} line {}: {} column(s), expected at least {columns}",
                path.display(),
                index + 1,
                row.len()
            )
            .into());
        }
        rows.push(row);
    }
    Ok(rows)
}
/// Rows grouped by their first column, in file order.
fn grouped(rows: Vec<Vec<String>>, key: usize) -> BTreeMap<String, Vec<Vec<String>>> {
    let mut groups: BTreeMap<String, Vec<Vec<String>>> = BTreeMap::new();
    for row in rows {
        groups.entry(row[key].clone()).or_default().push(row);
    }
    groups
}
fn compare(names: &[&str], observed: &[String], expected: &[String]) -> Result<(), String> {
    for (index, name) in names.iter().enumerate() {
        let (value, wanted) = (&observed[index], expected.get(index + 1));
        if Some(value) != wanted {
            return Err(format!("{name}: {value} != {}", wanted.map_or("-", |w| w)));
        }
    }
    Ok(())
}

/// Everything decoded from one movie that the oracle lists.
struct Decoded {
    summary: Vec<String>,
    frames: Vec<Vec<String>>,
    audio: Vec<Vec<String>>,
    count: u32,
}

fn decode(bytes: Vec<u8>, effects: Option<&[FadeEffect]>) -> Result<Decoded, String> {
    let (size, file_sha) = (bytes.len(), hex(&Sha256::digest(&bytes)));
    let mut movie = Movie::open(bytes).map_err(|error| error.to_string())?;
    let mut device = [0_u8; 768];
    let mut faded = [0_u8; 768];
    let mut hook = effects.and_then(|effects| FadeHook::new(effects.to_vec()));
    let (mut frames, mut audio) = (Vec::new(), Vec::new());
    let mut pcm = Sha256::new();
    let mut pcm_bytes = 0_usize;
    let mut timer = None;
    loop {
        match movie.next_event().map_err(|error| error.to_string())? {
            Event::Timer { rate, subdivision } => timer = Some((rate, subdivision)),
            Event::Audio { silence, pcm: out } => {
                audio.push(vec![
                    (audio.len() + 1).to_string(),
                    if silence { "9" } else { "8" }.to_owned(),
                    out.len().to_string(),
                    h16(&out),
                ]);
                pcm.update(&out);
                pcm_bytes += out.len();
            }
            Event::Frame(frame) => {
                let table = movie.palette_table();
                let (start, count) = (frame.palette_start, frame.palette_count);
                mve::push_palette(table, &mut device, start, count);
                let fx = match &mut hook {
                    Some(hook) => {
                        hook.push(table, &mut faded, start, count);
                        hook.update(frame.number, &mut faded);
                        h16(&faded)
                    }
                    None => String::new(),
                };
                let pixels = movie.video().ok_or("frame shown before video")?.current();
                frames.push(vec![
                    frame.number.to_string(),
                    u8::from(frame.video).to_string(),
                    u8::from(frame.suppressed).to_string(),
                    start.to_string(),
                    count.to_string(),
                    h16(&device),
                    h16(pixels),
                    fx,
                ]);
            }
            Event::Finished => break,
            Event::AudioFormat(_) | Event::Sync => {}
        }
    }
    let [width, height] = movie.video().map_or([0, 0], |video| video.size());
    let stats = movie.stats();
    let (rate, subdivision) = timer.unwrap_or((0, 0));
    let format = movie.audio_format();
    let flag = |value: Option<bool>| value.map_or("0", |set| if set { "1" } else { "0" });
    let joined = |values: &mut dyn Iterator<Item = String>| values.collect::<Vec<_>>().join(",");
    let summary = vec![
        size.to_string(),
        file_sha,
        width.to_string(),
        height.to_string(),
        stats.chunks.to_string(),
        movie.frames().to_string(),
        stats.video_frames.to_string(),
        rate.to_string(),
        subdivision.to_string(),
        format.map_or("-1".into(), |f| f.rate.to_string()),
        flag(format.map(|f| f.stereo)).into(),
        flag(format.map(|f| f.sixteen_bit)).into(),
        flag(format.map(|f| f.compressed)).into(),
        format.map_or("-1".into(), |f| f.buffer.to_string()),
        pcm_bytes.to_string(),
        if pcm_bytes == 0 {
            "-".into()
        } else {
            hex(&pcm.finalize())
        },
        joined(&mut stats.block_counts.iter().map(u64::to_string)),
        joined(
            &mut stats
                .opcode_counts
                .iter()
                .enumerate()
                .filter(|(_, count)| **count != 0)
                .map(|(kind, count)| format!("{kind}:{count}")),
        ),
    ];
    Ok(Decoded {
        summary,
        frames,
        audio,
        count: movie.frames(),
    })
}

/// Outcome counts of the SPEC-012 audit.
#[derive(Default)]
pub struct MovieAudit {
    pub movies: [u64; 2],
    pub frames: [u64; 2],
    pub audio: [u64; 2],
    pub effects: [u64; 2],
    pub subtitles: [u64; 2],
    pub corpus_movies: u64,
    pub corpus_subtitles: u64,
    /// Fade files without a movie of the same name.
    pub unused_fades: u64,
}
impl MovieAudit {
    pub fn failed(&self) -> u64 {
        self.movies[1] + self.frames[1] + self.audio[1] + self.effects[1] + self.subtitles[1]
    }
    pub fn report(&self) {
        println!(
            "MVE files: {} in corpus; summaries {} passed, {} failed",
            self.corpus_movies, self.movies[0], self.movies[1]
        );
        println!(
            "MVE frames: {} passed, {} failed; audio opcodes: {} passed, {} failed",
            self.frames[0], self.frames[1], self.audio[0], self.audio[1]
        );
        println!(
            "Fade effects: {} passed, {} failed; {} fade file(s) without a movie",
            self.effects[0], self.effects[1], self.unused_fades
        );
        println!(
            "SVE files: {} in corpus; entries {} passed, {} failed",
            self.corpus_subtitles, self.subtitles[0], self.subtitles[1]
        );
    }
}

/// Count one row result, reporting a failure.
fn tally(counts: &mut [u64; 2], what: &str, result: Result<(), String>) {
    match result {
        Ok(()) => counts[0] += 1,
        Err(error) => {
            counts[1] += 1;
            eprintln!("FAIL {what}: {error}");
        }
    }
}

const SUMMARY: [&str; 18] = [
    "bytes",
    "file SHA-256",
    "width",
    "height",
    "chunks",
    "frames",
    "video frames",
    "timer rate",
    "timer subdivision",
    "audio rate",
    "stereo",
    "16-bit",
    "compressed",
    "audio buffer",
    "PCM bytes",
    "PCM SHA-256",
    "block counts",
    "opcode counts",
];
const FRAME: [&str; 8] = [
    "frame",
    "video",
    "suppressed",
    "palette start",
    "palette count",
    "device palette",
    "pixels",
    "fade palette",
];

pub fn audit_movies(vfs: &LocalDirProvider, oracle: &Path) -> Result<MovieAudit, Box<dyn Error>> {
    let summaries = table(&oracle.join("mve-summary.tsv"), 1)?;
    let mut frame_rows = grouped(table(&oracle.join("mve-frames.tsv"), 1)?, 0);
    let mut audio_rows = grouped(table(&oracle.join("mve-audio.tsv"), 1)?, 0);
    let fx_rows = grouped(table(&oracle.join("moviefx.tsv"), 1)?, 0);
    // Language and file name.
    let subtitle_rows = table(&oracle.join("sve-subtitles.tsv"), 2)?;
    let mut audit = MovieAudit::default();
    let corpus = |extension: &str| -> BTreeSet<String> {
        vfs.files()
            .filter(|path| path.extension() == Some(extension))
            .map(|path| path.as_str().to_owned())
            .collect()
    };
    let (movies, subtitles) = (corpus("mve"), corpus("sve"));
    audit.corpus_movies = movies.len() as u64;
    audit.corpus_subtitles = subtitles.len() as u64;
    let listed: BTreeSet<String> = summaries
        .iter()
        .map(|row| format!("art/cuts/{}", row[0]))
        .collect();
    for missing in movies.difference(&listed) {
        tally(&mut audit.movies, missing, Err("no oracle row".into()));
    }
    // Fade files: each `art/cuts/*.cfg` against its moviefx rows.
    let mut effects_by_movie = BTreeMap::new();
    for path in corpus("cfg") {
        let Some(stem) = path
            .strip_prefix("art/cuts/")
            .and_then(|file| file.strip_suffix(".cfg"))
        else {
            continue;
        };
        let movie = format!("{stem}.mve");
        // A fade file that cannot be read or parsed is one failed effect
        // check; the remaining files and movies are still audited.
        let effects = match vfs
            .read_bounded(&path, MAX_ASSET_BYTES)
            .map_err(|error| error.to_string())
            .and_then(|bytes| moviefx::parse(&bytes).map_err(|error| error.to_string()))
        {
            Ok(effects) => effects,
            Err(error) => {
                tally(&mut audit.effects, &path, Err(error));
                continue;
            }
        };
        // A fade file applies only to the movie with its name (the corpus
        // `movie.cfg` is a commented example that no movie uses).
        if !vfs
            .files()
            .any(|file| file.as_str() == format!("art/cuts/{movie}"))
        {
            audit.unused_fades += 1;
            continue;
        }
        let rows = fx_rows.get(&movie).map_or(&[][..], Vec::as_slice);
        if effects.len() != rows.len() {
            tally(
                &mut audit.effects,
                &path,
                Err(format!(
                    "{} effects, oracle lists {}",
                    effects.len(),
                    rows.len()
                )),
            );
        }
        for (effect, row) in effects.iter().zip(rows) {
            let observed = [
                effect.start.to_string(),
                (i64::from(effect.start) + i64::from(effect.steps) - 1).to_string(),
                effect.steps.to_string(),
                if effect.out { "out" } else { "in" }.to_owned(),
                effect.color.map(|c| c.to_string()).join(","),
            ];
            let names = ["start", "end", "steps", "fade type", "color"];
            tally(&mut audit.effects, &path, compare(&names, &observed, row));
        }
        effects_by_movie.insert(movie, effects);
    }
    let mut frame_counts = BTreeMap::new();
    for row in &summaries {
        let file = &row[0];
        let path = format!("art/cuts/{file}");
        let decoded = vfs
            .read_bounded(&path, MAX_ASSET_BYTES)
            .map_err(|error| error.to_string())
            .and_then(|bytes| decode(bytes, effects_by_movie.get(file).map(Vec::as_slice)));
        let decoded = match decoded {
            Ok(decoded) => decoded,
            Err(error) => {
                tally(&mut audit.movies, &path, Err(error));
                continue;
            }
        };
        frame_counts.insert(file.clone(), decoded.count);
        tally(
            &mut audit.movies,
            &path,
            compare(&SUMMARY, &decoded.summary, row),
        );
        let expected = frame_rows.remove(file).unwrap_or_default();
        if expected.len() != decoded.frames.len() {
            tally(
                &mut audit.frames,
                &path,
                Err(format!(
                    "{} frames, oracle lists {}",
                    decoded.frames.len(),
                    expected.len()
                )),
            );
        }
        for (observed, wanted) in decoded.frames.iter().zip(&expected) {
            let what = format!("{path} frame {}", observed[0]);
            tally(&mut audit.frames, &what, compare(&FRAME, observed, wanted));
        }
        let expected = audio_rows.remove(file).unwrap_or_default();
        if expected.len() != decoded.audio.len() {
            tally(
                &mut audit.audio,
                &path,
                Err(format!(
                    "{} audio opcodes, oracle lists {}",
                    decoded.audio.len(),
                    expected.len()
                )),
            );
        }
        for (observed, wanted) in decoded.audio.iter().zip(&expected) {
            let what = format!("{path} audio {}", observed[0]);
            let names = ["index", "opcode", "bytes", "PCM hash"];
            tally(&mut audit.audio, &what, compare(&names, observed, wanted));
        }
    }
    for file in frame_rows.keys().chain(audio_rows.keys()) {
        tally(
            &mut audit.frames,
            file,
            Err("oracle rows without a summary".into()),
        );
    }
    // Subtitles: per language and file, entries in file order and the frame
    // that draws each one.
    let listed: BTreeSet<String> = subtitle_rows
        .iter()
        .map(|row| format!("text/{}/cuts/{}", row[0], row[1]))
        .collect();
    for missing in subtitles.difference(&listed) {
        tally(&mut audit.subtitles, missing, Err("no oracle rows".into()));
    }
    let mut by_file: BTreeMap<String, Vec<Vec<String>>> = BTreeMap::new();
    for row in subtitle_rows {
        by_file
            .entry(format!("text/{}/cuts/{}", row[0], row[1]))
            .or_default()
            .push(row);
    }
    for (path, rows) in by_file {
        let bytes = match vfs.read_bounded(&path, MAX_ASSET_BYTES) {
            Ok(bytes) => bytes,
            Err(error) => {
                tally(&mut audit.subtitles, &path, Err(error.to_string()));
                continue;
            }
        };
        let entries = sve::parse(&bytes);
        let movie = rows[0][1].replace(".sve", ".mve");
        let frames = frame_counts.get(&movie).copied().unwrap_or(0);
        let mut track = SubtitleTrack::new(entries.clone());
        let mut drawn = vec![None; entries.len()];
        let mut next = 0;
        for frame in 1..=frames {
            for _ in track.update(frame) {
                if let Some(slot) = drawn.get_mut(next) {
                    *slot = Some(frame);
                }
                next += 1;
            }
        }
        if entries.len() != rows.len() {
            tally(
                &mut audit.subtitles,
                &path,
                Err(format!(
                    "{} entries, oracle lists {}",
                    entries.len(),
                    rows.len()
                )),
            );
        }
        for (index, (entry, row)) in entries.iter().zip(&rows).enumerate() {
            let observed = [
                (index + 1).to_string(),
                entry.frame.to_string(),
                drawn[index].map_or("-".into(), |frame| frame.to_string()),
                entry.text.len().to_string(),
                h16(&entry.text),
            ];
            let names = [
                "entry",
                "frame number",
                "display frame",
                "text bytes",
                "text hash",
            ];
            tally(
                &mut audit.subtitles,
                &format!("{path} entry {}", index + 1),
                compare(&names, &observed, &row[1..]),
            );
        }
    }
    Ok(audit)
}
