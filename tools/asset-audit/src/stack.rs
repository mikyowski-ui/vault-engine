//! SPEC-017 §8: the script stack audit. Every `.INT` that
//! `scripts/scripts.lst` names is decoded by SPEC-004's layout and its
//! units (the initial stream, and each distinct procedure body) are run
//! through the abstract model of the value-stack depth (§8.2) in
//! [`vault_vm::stack_check`], with the host words popping and pushing by a
//! profile's table: Fallout 1's (SPEC-005), or Fallout 2's (SPEC-005 as
//! amended by SPEC-017). It reports
//! the model's findings, signature inference for the host words involved
//! (§8.3) and operand-type evidence, as counts and signatures only: no
//! script byte is printed. §8.4: with the Fallout 1 table over a Fallout 1
//! corpus it must find nothing, apart from the recorded exceptions.
//! Revision 2: host results that statement calls leave on the stack are
//! absorbed where paths rejoin.
use std::collections::{BTreeMap, BTreeSet};

use vault_formats::{LstKind, LstTable, MAX_ASSET_BYTES, Vfs};
use vault_vm::stack_check::{
    Exception, Finding, GAME_WORDS, Image, Table, Unit, UnitReport, analyse, clean, consistent,
    excuse,
};
use vault_vm::{IntProgram, WordSet, extensions};

/// SPEC-017's words: the six §3.2 reassigns and the four of §4.1.
pub const SPEC017_WORDS: [u16; 11] = [
    0x80b2, 0x80cf, 0x80e1, 0x80e2, 0x80e5, 0x80e6, 0x8109, 0x8156, 0x8157, 0x8158, 0x8159,
];
/// SPEC-017 §3.2: the Fallout 1 stubs whose Fallout 2 use is reported.
pub const STUB_WORDS: [u16; 4] = [0x80ad, 0x80b3, 0x80b5, 0x80d1];

/// The whole audit's tallies.
#[derive(Debug, Default)]
pub struct StackAudit {
    pub scripts: usize,
    /// Scripts `scripts.lst` names that the mount does not hold.
    pub absent: usize,
    /// Listed scripts that could not be read or decoded by SPEC-004's
    /// layout.
    pub undecodable: usize,
    pub units: usize,
    pub empty_bodies: usize,
    /// Units in which a merge removed a leaked host result (not a
    /// finding).
    pub leaked_units: usize,
    /// The findings by kind, the recorded exceptions' excluded.
    pub findings: BTreeMap<Finding, u64>,
    /// Findings by kind and word (0 for none).
    pub finding_words: BTreeMap<(Finding, u16), u64>,
    /// §8.4 each recorded exception with its observed count, `None` when
    /// the script is not in the mount; it matches when the counts agree.
    pub exceptions: Vec<(Exception, Option<u64>)>,
    /// Instruction occurrences of each host word in the linear decode.
    pub occurrences: BTreeMap<u16, u64>,
    pub operands: BTreeMap<(u16, usize), [u64; 2]>,
    /// §8.3 per word: its consistent candidates.
    pub inferred: BTreeMap<u16, Vec<(u8, u8)>>,
}
impl StackAudit {
    /// The stale exceptions: each one counts as a finding.
    pub fn stale(&self) -> u64 {
        self.exceptions
            .iter()
            .filter(|(exception, observed)| *observed != Some(exception.count))
            .count() as u64
    }
    pub fn total(&self) -> u64 {
        self.findings.values().sum::<u64>() + self.stale()
    }
    /// Run over `images` with `table` and its recorded `exceptions`, then
    /// infer the signatures of the host words in units with findings, and
    /// of the `extra` words that occur.
    pub fn run(images: &[Image], table: &Table, extra: &[u16], exceptions: &[Exception]) -> Self {
        let mut audit = Self::default();
        let reports: Vec<Vec<UnitReport>> =
            images.iter().map(|image| analyse(image, table)).collect();
        // §8.4 a matched exception excuses its findings; a stale one
        // excuses none.
        let (excused, observed) = excuse(images, &reports, exceptions);
        audit.exceptions = observed;
        let mut containing: BTreeMap<u16, Vec<(&Image, Unit)>> = BTreeMap::new();
        let mut flagged: BTreeSet<u16> = BTreeSet::new();
        for (image, reports) in images.iter().zip(&reports) {
            audit.scripts += 1;
            audit.empty_bodies += image.empty_bodies;
            for word in image.linear_words() {
                if word >= 0x804c && word & 0x3ff != 1 {
                    *audit.occurrences.entry(word).or_default() += 1;
                }
            }
            let excused_here = excused.get(&image.name);
            for (unit, report) in image.units.iter().zip(reports) {
                audit.units += 1;
                audit.leaked_units += usize::from(report.leaked);
                for (finding, word) in &report.findings {
                    if excused_here.is_some_and(|keys| keys.contains(&(*finding, *word))) {
                        continue;
                    }
                    *audit.findings.entry(*finding).or_default() += 1;
                    *audit.finding_words.entry((*finding, *word)).or_default() += 1;
                }
                for (key, [constants, strings]) in &report.operands {
                    let evidence = audit.operands.entry(*key).or_default();
                    evidence[0] += constants;
                    evidence[1] += strings;
                }
                let found = !clean(image, report, &excused);
                for word in report
                    .reached
                    .keys()
                    .filter(|word| GAME_WORDS.contains(word))
                {
                    containing.entry(*word).or_default().push((image, *unit));
                    if found {
                        flagged.insert(*word);
                    }
                }
            }
        }
        flagged.extend(extra.iter().filter(|word| containing.contains_key(word)));
        for word in flagged {
            audit
                .inferred
                .insert(word, consistent(&containing[&word], table, word, &excused));
        }
        audit
    }
    /// Print the counts and signatures (never script bytes). `words` are
    /// listed even when they do not occur.
    pub fn report(&self, profile: &str, table: &Table, words: &[u16]) {
        println!(
            "Script stack ({profile} table): {} scripts, {} listed but absent, {} undecodable, \
             {} units, {} empty procedure bodies",
            self.scripts, self.absent, self.undecodable, self.units, self.empty_bodies
        );
        println!(
            "Script stack findings: {} ({}, stale exception {})",
            self.total(),
            Finding::ALL
                .iter()
                .map(|kind| format!(
                    "{} {}",
                    kind.name(),
                    self.findings.get(kind).copied().unwrap_or(0)
                ))
                .collect::<Vec<_>>()
                .join(", "),
            self.stale()
        );
        println!(
            "Script stack units with leaked results: {}",
            self.leaked_units
        );
        for (exception, observed) in &self.exceptions {
            let Exception {
                script,
                finding,
                word,
                count,
            } = exception;
            let finding = finding.name();
            match observed {
                Some(observed) if observed == count => println!(
                    "Script stack recorded exception: {script} {finding} at word {word:#06x} x{count}"
                ),
                Some(observed) => println!(
                    "Script stack stale exception: {script} {finding} at word {word:#06x} \
                     recorded x{count}, found x{observed}"
                ),
                None => println!(
                    "Script stack stale exception: {script} {finding} at word {word:#06x} \
                     recorded x{count}, script not in the mount"
                ),
            }
        }
        for ((kind, word), count) in &self.finding_words {
            if *word == 0 {
                println!("Script stack finding: {} x{count}", kind.name());
            } else {
                println!(
                    "Script stack finding: {} at word {word:#06x} x{count}",
                    kind.name()
                );
            }
        }
        let signature = |pair: Option<(u8, u8)>| {
            pair.map_or_else(
                || "variable".into(),
                |(pops, push)| format!("pops {pops} push {push}"),
            )
        };
        let listed: BTreeSet<u16> = self
            .inferred
            .keys()
            .copied()
            .chain(words.iter().copied())
            .collect();
        for word in &listed {
            let consistent = match self.inferred.get(word) {
                None => "not inferred".to_string(),
                Some(candidates) if candidates.is_empty() => "none".to_string(),
                Some(candidates) => candidates
                    .iter()
                    .map(|pair| signature(Some(*pair)))
                    .collect::<Vec<_>>()
                    .join("; "),
            };
            println!(
                "Script stack word {word:#06x} ({}): {} occurrences, table {}, consistent: {consistent}",
                extensions::opcode_in(&table.words, *word).map_or("reserved", |spec| spec.name),
                self.occurrences.get(word).copied().unwrap_or(0),
                signature(table.signature(*word)),
            );
        }
        for ((word, position), [constants, strings]) in &self.operands {
            if listed.contains(word) {
                println!(
                    "Script stack operands {word:#06x} operand {position}: \
                     {constants} constants, {strings} strings"
                );
            }
        }
    }
}

/// The scripts `scripts/scripts.lst` names, decoded by SPEC-004's layout
/// with every bit-15 word accepted, so that the model, not the loader,
/// reports words outside the profile's set. Returns the images, the
/// count of listed scripts the mount does not hold, and the count that
/// could not be decoded.
pub fn listed_images(vfs: &dyn Vfs) -> Result<(Vec<Image>, usize, usize), String> {
    let list = vfs
        .read_bounded("scripts/scripts.lst", MAX_ASSET_BYTES)
        .map_err(|error| format!("scripts/scripts.lst: {error}"))?;
    let list = LstTable::parse(&list, LstKind::Script).map_err(|error| error.to_string())?;
    let every = WordSet {
        words: 0x8000..=0xffff,
    };
    let names: BTreeSet<String> = list
        .entries()
        .iter()
        .filter_map(|entry| entry.name.as_deref())
        .map(|name| String::from_utf8_lossy(name).to_ascii_lowercase())
        .collect();
    let mut images = Vec::new();
    let (mut absent, mut undecodable) = (0, 0);
    for name in names {
        let path = format!("scripts/{name}");
        let Ok(bytes) = vfs.read_bounded(&path, MAX_ASSET_BYTES) else {
            absent += 1;
            continue;
        };
        match IntProgram::parse_in(&bytes, &every, false) {
            Ok(program) => images.push(Image::of(name, &program)),
            Err(error) => {
                undecodable += 1;
                eprintln!("SCRIPT STACK UNDECODABLE {path}: {error}");
            }
        }
    }
    Ok((images, absent, undecodable))
}

/// SPEC-024b §3.2: every `.INT` under `scripts/`, listed or not, with sfall
/// mode's table: a mod's hook and global scripts appear in no `scripts.lst`.
pub fn audit_sfall(vfs: &dyn Vfs) -> Result<StackAudit, String> {
    let every = WordSet {
        words: 0x8000..=0xffff,
    };
    let paths: Vec<String> = vfs
        .files()
        .map(|path| path.as_str().to_owned())
        .filter(|path| path.starts_with("scripts/") && path.ends_with(".int"))
        .collect();
    let mut images = Vec::new();
    let mut undecodable = 0;
    for path in paths {
        let bytes = vfs
            .read_bounded(&path, MAX_ASSET_BYTES)
            .map_err(|error| format!("{path}: {error}"))?;
        match IntProgram::parse_in(&bytes, &every, false) {
            Ok(program) => {
                let name = path.trim_start_matches("scripts/").to_owned();
                images.push(Image::of(name, &program));
            }
            Err(error) => {
                undecodable += 1;
                eprintln!("SCRIPT STACK UNDECODABLE {path}: {error}");
            }
        }
    }
    let table = Table::new(WordSet::FALLOUT2_SFALL);
    let mut audit = StackAudit::run(&images, &table, &[], &[]);
    audit.undecodable = undecodable;
    Ok(audit)
}

/// The audit over a mounted corpus with `words`' table and its recorded
/// exceptions. Under Fallout 2, SPEC-017's ten words are inferred whenever
/// they occur.
pub fn audit(vfs: &dyn Vfs, words: WordSet) -> Result<(StackAudit, Table), String> {
    let (images, absent, undecodable) = listed_images(vfs)?;
    let table = Table::new(words);
    let extra: &[u16] = if table.words.fallout2() {
        &SPEC017_WORDS
    } else {
        &[]
    };
    let exceptions = vault_vm::stack_check::exceptions(&table.words);
    let mut audit = StackAudit::run(&images, &table, extra, exceptions);
    audit.absent = absent;
    audit.undecodable = undecodable;
    Ok((audit, table))
}

#[cfg(test)]
mod tests {
    use super::*;
    use vault_vm::stack_check::FALLOUT1_EXCEPTIONS;

    fn hex(text: &str) -> Vec<u8> {
        text.split_whitespace()
            .map(|byte| u8::from_str_radix(byte, 16).unwrap())
            .collect()
    }

    #[test]
    fn spec017_vector_n_the_fallout1_audit_counts_the_merge_and_infers_5_0_and_5_1() {
        // SPEC-017 §8.6 Vector N; the model's own vectors are
        // `vault_vm::stack_check`'s tests.
        let image = Image::body(
            &hex("80 2B C0 01 00 00 00 30 C0 01 00 00 00 01 80 2F \
                 C0 01 00 00 00 00 C0 01 00 00 00 05 C0 01 00 00 \
                 00 01 C0 01 00 00 4E 84 C0 01 00 00 00 02 80 E6 \
                 80 2A 80 29 80 1C"),
            0,
        );
        let table = Table::new(WordSet::FALLOUT1);
        let audit = StackAudit::run(&[image], &table, &[], &[]);
        assert_eq!(audit.total(), 1);
        assert_eq!(audit.findings[&Finding::MergeConflict], 1);
        assert_eq!(audit.leaked_units, 0);
        assert_eq!(audit.inferred[&0x80e6], [(5, 0), (5, 1)]);
        assert_eq!(audit.occurrences[&0x80e6], 1);
    }

    #[test]
    fn spec017_vector_o_the_audit_counts_a_unit_with_leaked_results() {
        let image = Image::body(
            &hex("80 2B C0 01 00 00 00 24 C0 01 00 00 00 01 80 2F \
                 C0 01 00 00 00 01 C0 01 00 00 00 02 C0 01 00 00 \
                 00 03 80 B6 C0 01 00 00 00 00 90 01 00 00 00 00 \
                 80 28 80 05 80 1A 80 2A 80 29 80 1C"),
            0,
        );
        let table = Table::new(WordSet::FALLOUT1);
        let audit = StackAudit::run(&[image], &table, &[], &[]);
        assert_eq!(audit.total(), 0);
        assert_eq!(audit.leaked_units, 1);
        assert!(audit.inferred.is_empty());
    }

    #[test]
    fn recorded_exceptions_match_exactly_and_stale_ones_count() {
        // A four-operand start_gdialog: one underflow at 0x80DE.
        let mut image = Image::body(
            &hex(
                "80 2B C0 01 00 00 00 00 C0 01 00 00 00 00 C0 01 00 00 00 00 \
                 C0 01 00 00 00 00 80 DE 80 1C",
            ),
            0,
        );
        image.name = "gatedemo.int".into();
        let table = Table::new(WordSet::FALLOUT1);
        let exception = |script, count| Exception {
            script,
            finding: Finding::Underflow,
            word: 0x80de,
            count,
        };
        // Matched: not a finding, and the unit is not one with a finding.
        let matched = StackAudit::run(
            std::slice::from_ref(&image),
            &table,
            &[],
            &[exception("gatedemo.int", 1)],
        );
        assert_eq!(matched.total(), 0);
        assert_eq!(matched.stale(), 0);
        assert!(matched.findings.is_empty() && matched.finding_words.is_empty());
        assert_eq!(
            matched.exceptions,
            [(exception("gatedemo.int", 1), Some(1))]
        );
        assert!(matched.inferred.is_empty());
        // FALLOUT1_EXCEPTIONS is that row.
        assert_eq!(FALLOUT1_EXCEPTIONS, [exception("gatedemo.int", 1)]);
        // The count differs: one stale exception, and the finding counts.
        let count = StackAudit::run(
            std::slice::from_ref(&image),
            &table,
            &[],
            &[exception("gatedemo.int", 2)],
        );
        assert_eq!(count.total(), 2);
        assert_eq!(count.stale(), 1);
        assert_eq!(count.findings[&Finding::Underflow], 1);
        assert!(count.inferred.contains_key(&0x80de));
        // The script is not in the mount.
        let absent = StackAudit::run(
            std::slice::from_ref(&image),
            &table,
            &[],
            &[exception("absent.int", 1)],
        );
        assert_eq!(absent.total(), 2);
        assert_eq!(absent.exceptions, [(exception("absent.int", 1), None)]);
        // A different kind or word is not excused.
        let other = StackAudit::run(
            &[image],
            &table,
            &[],
            &[Exception {
                word: 0x80df,
                ..exception("gatedemo.int", 1)
            }],
        );
        assert_eq!(other.total(), 2);
    }
}
