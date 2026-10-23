//! Numbered puzzle diagrams, by chapter, and the solutions that name them.
//!
//! A workbook may number each chapter's exercises from 1 and print the solutions later, all
//! together. The diagrams are kept in **groups**: a diagram whose number is not above the last
//! one's starts a new group (a chapter numbering its exercises again). Each group remembers the
//! headings read since the group before it ([`Key`]): the chapter's number (`Chapter 6`,
//! `CAPÍTULO 7`) and its title.
//!
//! A solution naming a number only one group has is played from that group's diagram. When
//! several have it, the chapter is found from the structure, in this order:
//!
//! 1. the chapter the label gives (`Ex. 6–12`): the last group under a `Chapter 6` heading;
//! 2. the newest heading read since the last numbered diagram that belongs to exactly one group
//!    (the solutions' `Chapter 6` or the chapter's title repeated over its solutions): that group;
//! 3. the chapter order: solutions also number again for each chapter, and each run of them
//!    (a new one when a number is not above the last one's, or when a new group of diagrams came
//!    since) goes to the first group no solution went to yet;
//! 4. else the last diagram given the number, as before groups were kept.
//!
//! Nothing here guesses from the moves: a solution whose moves do not fit still fails, as without
//! the link.

use super::{MAX_PUZZLES, Seed};

/// Most headings a group of diagrams remembers, and most kept since the last numbered diagram.
pub const MAX_GROUP_KEYS: usize = 16;

/// What a heading says about its chapter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Key {
    /// The chapter's number (`Chapter 6`, `Solutions to Chapter 6`).
    Chapter(u32),
    /// The heading's words, lower case, one space apart.
    Title(String),
}

/// How a solution names its puzzle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Name {
    pub number: u32,
    /// The chapter its label gives before the number (`Ex. 6–12`).
    pub chapter: Option<u32>,
    /// The label is the caption of a diagram beside it, not a solution: it takes the last
    /// diagram given the number and does not count as a solution.
    pub caption: bool,
}

#[derive(Debug, Default)]
struct Group {
    keys: Vec<Key>,
    /// A solution went to it.
    solved: bool,
}

/// The run of solutions being read.
#[derive(Debug)]
struct Answers {
    /// The group the run goes to, once one is known.
    group: Option<usize>,
    /// The last number it named.
    last: u32,
    /// How many groups there were when it named it.
    groups: usize,
}

#[derive(Debug, Default)]
pub(crate) struct Puzzles {
    /// (group, diagram), at most [`MAX_PUZZLES`].
    seeds: Vec<(usize, Seed)>,
    groups: Vec<Group>,
    /// The number of the last diagram kept.
    last: Option<u32>,
    /// The keys of the headings read since the last numbered diagram, oldest first.
    recent: Vec<Key>,
    answers: Option<Answers>,
}

/// Add `key` as the newest of `keys` (moved there when it was already in), dropping the oldest
/// past [`MAX_GROUP_KEYS`].
fn push_key(keys: &mut Vec<Key>, key: Key) {
    keys.retain(|k| *k != key);
    if keys.len() >= MAX_GROUP_KEYS {
        keys.remove(0);
    }
    keys.push(key);
}

impl Puzzles {
    /// A diagram with a puzzle number: kept for the solutions that name it, in a new group when
    /// its number is not above the last one's.
    pub fn keep(&mut self, seed: &Seed) {
        let Some(n) = seed.puzzle else { return };
        let restart = self.groups.is_empty() || self.last.is_some_and(|last| n <= last);
        if restart {
            if self.groups.len() >= MAX_PUZZLES {
                return;
            }
            self.groups.push(Group { keys: std::mem::take(&mut self.recent), solved: false });
        } else if let Some(group) = self.groups.last_mut() {
            for key in std::mem::take(&mut self.recent) {
                push_key(&mut group.keys, key);
            }
        }
        let group = self.groups.len().saturating_sub(1);
        self.last = Some(n);
        if let Some((_, old)) = self.seeds.iter_mut().find(|(g, s)| *g == group && s.puzzle == Some(n)) {
            *old = seed.clone();
        } else if self.seeds.len() < MAX_PUZZLES {
            self.seeds.push((group, seed.clone()));
        }
    }

    /// A heading's keys ([`Key`]): its chapter's number and its title.
    pub fn heading(&mut self, keys: Vec<Key>) {
        for key in keys {
            push_key(&mut self.recent, key);
        }
    }

    /// Whether a diagram has the number `n`.
    pub fn has(&self, n: u32) -> bool {
        self.seeds.iter().any(|(_, s)| s.puzzle == Some(n))
    }

    /// The diagram a solution names (see the module's documentation), or `None` when no
    /// diagram has its number.
    pub fn find(&mut self, name: Name) -> Option<Seed> {
        let n = name.number;
        // The groups that have the number, oldest first.
        let mut candidates: Vec<usize> = self.seeds.iter().filter(|(_, s)| s.puzzle == Some(n)).map(|(g, _)| *g).collect();
        candidates.dedup();
        let latest = *candidates.last()?;
        let group = if name.caption {
            latest
        } else {
            let fresh = self.answers.as_ref().is_none_or(|a| n <= a.last || a.groups != self.groups.len());
            if fresh {
                let first_unsolved = self.groups.iter().position(|g| !g.solved);
                self.answers = Some(Answers { group: first_unsolved, last: n, groups: self.groups.len() });
            }
            let ordinal = self.answers.as_ref().and_then(|a| a.group);
            let group = if candidates.len() == 1 {
                latest
            } else {
                self.by_chapter(name.chapter, &candidates)
                    .or_else(|| self.by_heading(&candidates))
                    .or(ordinal.filter(|g| candidates.contains(g)))
                    .unwrap_or(latest)
            };
            if let Some(a) = self.answers.as_mut() {
                a.group = Some(group);
                a.last = n;
                a.groups = self.groups.len();
            }
            if let Some(g) = self.groups.get_mut(group) {
                g.solved = true;
            }
            group
        };
        self.seeds.iter().find(|(g, s)| *g == group && s.puzzle == Some(n)).map(|(_, s)| s.clone())
    }

    /// The last of `candidates` under a heading numbering chapter `chapter`.
    fn by_chapter(&self, chapter: Option<u32>, candidates: &[usize]) -> Option<usize> {
        let key = Key::Chapter(chapter?);
        candidates.iter().rev().copied().find(|g| self.groups.get(*g).is_some_and(|g| g.keys.contains(&key)))
    }

    /// The group of the newest recent heading that belongs to exactly one group, when it is one
    /// of `candidates`.
    fn by_heading(&self, candidates: &[usize]) -> Option<usize> {
        for key in self.recent.iter().rev() {
            let mut owners = self.groups.iter().enumerate().filter(|(_, g)| g.keys.contains(key)).map(|(i, _)| i);
            if let (Some(owner), None) = (owners.next(), owners.next()) {
                return candidates.contains(&owner).then_some(owner);
            }
        }
        None
    }
}
