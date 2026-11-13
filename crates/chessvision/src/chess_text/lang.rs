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

/// Fewest function words a vote needs to say a prose language ([`ProseTally::guess`]).
pub const MIN_PROSE_WORDS: u32 = 5;

/// Words that are common in one language's prose and rare as words of another's (articles,
/// conjunctions, prepositions, a few verbs), lower case. A word several languages share counts for
/// each; the vote goes by the most counted.
const FUNCTION_WORDS: [(Lang, &[&str]); 9] = [
    (Lang::En, &["the", "and", "of", "is", "with", "this", "that", "for", "which", "after", "but", "not", "from", "would"]),
    (Lang::Pt, &["o", "os", "as", "do", "da", "dos", "das", "não", "com", "uma", "um", "para", "é", "mas", "pelo", "pela", "após", "são"]),
    (Lang::Es, &["el", "los", "las", "del", "y", "con", "una", "para", "es", "pero", "por", "tras", "después", "son"]),
    (Lang::Fr, &["le", "les", "des", "du", "et", "est", "avec", "une", "pour", "dans", "mais", "pas", "après", "sont"]),
    (Lang::De, &["der", "die", "das", "und", "ist", "nicht", "mit", "ein", "eine", "auf", "nach", "für", "aber", "den"]),
    (Lang::It, &["il", "gli", "di", "che", "non", "per", "è", "della", "del", "con", "una", "ma", "dopo", "sono"]),
    (Lang::Nl, &["het", "een", "en", "van", "niet", "met", "op", "voor", "maar", "na", "zijn"]),
    (Lang::Ru, &["и", "в", "не", "на", "что", "с", "после", "но", "это", "для"]),
    (Lang::Pl, &["i", "w", "nie", "na", "się", "z", "że", "po", "ale", "jest", "dla"]),
];

/// The language of a page's prose (plan A9-L6a, A2-08), voted by its function words: not the
/// language of its piece letters, which a book with figurines does not show (a Brazilian book read
/// as English). Pages add up, as [`LangTally`]'s do.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProseTally {
    hits: [u32; FUNCTION_WORDS.len()],
}

impl ProseTally {
    /// Count the function words of `text`.
    pub fn add_text(&mut self, text: &str) {
        for word in text.split(|c: char| !c.is_alphabetic()).filter(|w| !w.is_empty()) {
            let lower = word.to_lowercase();
            for (hit, (_, words)) in self.hits.iter_mut().zip(FUNCTION_WORDS) {
                if words.contains(&lower.as_str()) {
                    *hit = hit.saturating_add(1);
                }
            }
        }
    }

    /// Add another tally (another page) to this one.
    pub fn merge(&mut self, other: &ProseTally) {
        for (a, b) in self.hits.iter_mut().zip(other.hits) {
            *a = a.saturating_add(b);
        }
    }

    /// The language with the most function words, when it has at least [`MIN_PROSE_WORDS`] and
    /// more than any other; `None` when the prose says too little or two languages tie.
    pub fn guess(&self) -> Option<Lang> {
        let best = self.hits.iter().copied().max().unwrap_or(0);
        if best < MIN_PROSE_WORDS || self.hits.iter().filter(|h| **h == best).count() > 1 {
            return None;
        }
        FUNCTION_WORDS.iter().zip(self.hits).find(|(_, h)| *h == best).map(|((l, _), _)| *l)
    }
}

#[cfg(test)]
mod prose_tests {
    use super::*;

    #[test]
    fn the_prose_votes_its_language_by_its_function_words() {
        let guess = |text: &str| {
            let mut t = ProseTally::default();
            t.add_text(text);
            t.guess()
        };
        assert_eq!(guess("As brancas não podem jogar o lance com a dama, mas a torre é forte para o ataque."), Some(Lang::Pt));
        assert_eq!(guess("The knight is strong and the bishop is weak, but White has the better of it."), Some(Lang::En));
        assert_eq!(guess("Der Springer ist stark und die Dame steht nicht gut auf dem Brett."), Some(Lang::De));
        assert_eq!(guess("1.e4 e5 2.Nf3"), None, "moves are no prose");
        assert_eq!(guess("the and"), None, "too few words");
    }
}
