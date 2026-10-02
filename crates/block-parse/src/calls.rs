//! Callable blocks: left as their names, or called with some of their
//! parameters. See `documentation/07-calls.md`.

use crate::language::{BlockDef, BlockKind, InputDef, Part, ScopeConfig, SignatureConfig, is_blank};
use crate::program::{Block, Reach};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Callable {
    /// Its inputs and lists, one parameter each.
    Parts,
    /// A procedure reference's: this list, an item per parameter its
    /// declaration gives.
    Arguments(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Extent {
    pub parameters: usize,
    /// Through the last parameter holding something; the block cannot show
    /// fewer.
    pub filled: usize,
    pub shown: usize,
    /// `shown` is then 0.
    pub named: bool,
}

impl BlockDef {
    /// Labels before a hidden parameter go with it; those before the first
    /// name the block and always show.
    pub fn shown_parts(&self, shown: usize) -> &[Part] {
        if self.callable != Some(Callable::Parts) {
            return &self.parts;
        }
        let mut end = None;
        let mut count = 0;
        for (index, part) in self.parts.iter().enumerate() {
            if matches!(part, Part::Input(_) | Part::List(_)) {
                if count == shown {
                    return &self.parts[..end.unwrap_or(index)];
                }
                count += 1;
                end = Some(index + 1);
            }
        }
        &self.parts
    }

    /// `None` unless callable. `arity` is how many parameters a procedure
    /// reference's declaration gives.
    pub fn extent(&self, block: &Block, arity: Option<usize>) -> Option<Extent> {
        let (parameters, filled) = match self.callable.as_ref()? {
            Callable::Parts => {
                let (mut parameters, mut filled) = (0, 0);
                for part in &self.parts {
                    let holds = match part {
                        Part::Input(input) => block.inputs.get(&input.name).is_some_and(|stored| {
                            stored.block.is_some()
                                || stored
                                    .literal
                                    .as_deref()
                                    .is_some_and(|text| !is_blank(text) && Some(text) != input.default.as_deref())
                        }),
                        Part::List(list) => block
                            .lists
                            .get(&list.name)
                            .is_some_and(|items| items.iter().any(|item| !item.is_hole())),
                        Part::Label(_) | Part::Branch(_) => continue,
                    };
                    parameters += 1;
                    if holds {
                        filled = parameters;
                    }
                }
                (parameters, filled)
            }
            // Trimmed, so it ends at the last item holding something.
            Callable::Arguments(list) => {
                let len = block.lists.get(list).map_or(0, Vec::len);
                (arity.unwrap_or(0).max(len), len)
            }
        };
        let (shown, named) = match block.reach {
            None => (parameters, false),
            Some(Reach::Name) if filled == 0 => (0, true),
            Some(Reach::Name) => (filled, false),
            Some(Reach::Call(n)) => (n.clamp(filled, parameters), false),
        };
        Some(Extent {
            parameters,
            filled,
            shown,
            named,
        })
    }
}

impl Extent {
    /// Every reach the block may take, narrowest first. `None`, showing
    /// every parameter, stands for `Call(parameters)`.
    pub fn stops(&self) -> Vec<Option<Reach>> {
        let name = (self.filled == 0).then_some(Some(Reach::Name));
        let calls = (self.filled..self.parameters).map(|n| Some(Reach::Call(n)));
        name.into_iter().chain(calls).chain([None]).collect()
    }

    pub fn reach(&self) -> Option<Reach> {
        match self.shown {
            _ if self.named => Some(Reach::Name),
            shown if shown == self.parameters => None,
            shown => Some(Reach::Call(shown)),
        }
    }
}

/// A block's own `callable` against the language's. A block saying `true`
/// that cannot be callable is refused, unless a signature makes it so.
pub(crate) fn resolve(
    configured: Option<bool>,
    kind: &BlockKind,
    parts: &[Part],
    language: bool,
) -> Result<Option<Callable>, &'static str> {
    let names_it = matches!(parts.first(), Some(Part::Label(_)));
    match (configured, kind.output()) {
        (Some(true), None) => Err("only reporters are callable"),
        (Some(true), Some(_)) if !names_it => Err("a callable block's spec starts with a label, its name"),
        (Some(true), Some(_)) => Ok(Some(Callable::Parts)),
        (None, Some(_)) if language && names_it => Ok(Some(Callable::Parts)),
        (Some(false) | None, _) => Ok(None),
    }
}

/// How many parameters a fresh block shows, when fewer than all.
pub(crate) fn shows(shows: Option<usize>, callable: Option<&Callable>, parts: &[Part]) -> Result<Option<usize>, String> {
    let parameters = parts.iter().filter(|part| matches!(part, Part::Input(_) | Part::List(_))).count();
    match shows {
        Some(_) if callable != Some(&Callable::Parts) => Err("only a callable block shows fewer parameters".into()),
        Some(shows) if shows > parameters => Err(format!("shows {shows} parameters but has {parameters}")),
        shows => Ok(shows.filter(|&shows| shows < parameters)),
    }
}

/// Returns the reference's arguments list and its name input.
pub(crate) fn check_signature<'a>(
    def: &BlockDef,
    scope: &ScopeConfig,
    signature: &SignatureConfig,
    target: Option<&'a BlockDef>,
    problem: &mut impl FnMut(String),
) -> (Option<String>, Option<&'a InputDef>) {
    let target = target.filter(|target| target.inputs().count() == 1 && target.lists().count() == 1);
    if target.is_none() {
        problem(format!(
            "signature reference `{}` must be a reporter with one input and one list",
            signature.reference
        ));
    }
    if def.input(&signature.name).is_none() || !scope.declares.contains(&signature.name) {
        problem(format!("signature name `{}` is no declaring input", signature.name));
    }
    if def.list(&signature.parameters).is_none() || !scope.declares.contains(&signature.parameters) {
        problem(format!("signature parameters `{}` are no declaring list", signature.parameters));
    }
    let list = target.and_then(|target| target.lists().next()).map(|list| list.name.clone());
    (list, target.and_then(|target| target.inputs().next()))
}
