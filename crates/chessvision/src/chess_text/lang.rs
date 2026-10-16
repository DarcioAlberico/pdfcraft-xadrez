//! Which language's piece letters a page (or a whole book) uses.
//!
//! Every word that reads as a piece move in at least one language is a candidate, and each
//! language scores the candidates it can read: `Cf3` scores for Portuguese, Spanish, French,
//! Italian, Romanian and Catalan, `Sf3` for German, the Scandinavian languages and Polish. A move
//! that starts with a figurine reads the same in every language, so it scores for all of them.
//! Pawn moves and castling say nothing about the language and are not counted.
//!
//! Languages that share the letters seen tie. A tie goes to the hint when there is one and
//! otherwise to the earliest language in [`Lang::ALL`] (English, Portuguese, German, Spanish…), and
//! the confidence is divided among the tied languages, so it says how much the letters alone
//! decided.

use pdfcraft_chess::san::Lang;

/// Below this many candidate words a hint is taken as it is.
pub const MIN_EVIDENCE: u32 = 3;

/// The language a page's piece letters were read in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LangGuess {
    pub lang: Lang,
    /// 0 to 1: the share of candidate words this language reads, divided by the number of
    /// languages that read exactly as many. 0 when the page had no piece moves at all.
    pub confidence: f32,
}

/// Language evidence counted over words. Pages add up: merge the tallies of a document's pages
/// and [`guess`](LangTally::guess) once to choose a language for the whole book, then pass it as
/// the hint of every page.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LangTally {
    /// For each language of [`Lang::ALL`], how many candidates it reads.
    hits: [u32; Lang::ALL.len()],
    /// Words that read as a piece move in at least one language, figurine moves included.
    candidates: u32,
}

impl LangTally {
    /// Count one candidate word: `reads(lang)` says whether it reads as a piece move in `lang`.
    /// Words no language reads are not candidates.
    pub(crate) fn add(&mut self, reads: impl Fn(Lang) -> bool) {
        let mut any = false;
        for (hit, lang) in self.hits.iter_mut().zip(Lang::ALL) {
            if reads(lang) {
                *hit = hit.saturating_add(1);
                any = true;
            }
        }
        if any {
            self.candidates = self.candidates.saturating_add(1);
        }
    }

    /// Add another tally (another page) to this one.
    pub fn merge(&mut self, other: &LangTally) {
        for (a, b) in self.hits.iter_mut().zip(other.hits) {
            *a = a.saturating_add(b);
        }
        self.candidates = self.candidates.saturating_add(other.candidates);
    }

    /// How many words were evidence.
    pub fn candidates(&self) -> u32 {
        self.candidates
    }

    fn hits(&self, lang: Lang) -> u32 {
        Lang::ALL.iter().zip(self.hits).find(|(l, _)| **l == lang).map_or(0, |(_, h)| h)
    }

    /// The language the counted words read best in.
    ///
    /// A `hint` (the document's language, or the user's choice) is kept unless the words clearly
    /// contradict it: it wins when there are fewer than [`MIN_EVIDENCE`] candidates, or when it
    /// reads at least half as many as the best language. Without a hint and without evidence the
    /// answer is English with confidence 0.
    pub fn guess(&self, hint: Option<Lang>) -> LangGuess {
        let best = self.hits.iter().copied().max().unwrap_or(0);
        let lang = match hint {
            Some(h) if self.candidates < MIN_EVIDENCE || self.hits(h).saturating_mul(2) >= best => h,
            // `ALL` order breaks ties: the first language with the best score.
            _ => Lang::ALL.iter().zip(self.hits).find(|(_, h)| *h == best).map_or(Lang::En, |(l, _)| *l),
        };
        LangGuess { lang, confidence: self.confidence(lang) }
    }

    fn confidence(&self, lang: Lang) -> f32 {
        let hits = self.hits(lang);
        if self.candidates == 0 || hits == 0 {
            return 0.0;
        }
        let tied = self.hits.iter().filter(|h| **h == hits).count().max(1);
        // Counts fit an f32 closely enough for a confidence; `candidates` is not zero here.
        (hits as f32 / self.candidates as f32 / tied as f32).clamp(0.0, 1.0)
    }
}
