//! Callable blocks' widths and procedure references' arguments.

use block_parse::language::{Extent, ListDef};
use block_parse::program::{Block, Reach};

use super::{Font, Item, Layout};

/// A block with a scope, as references to it are drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct Declarer {
    pub opcode: String,
    /// Its signature's parameter names, for references to its procedure.
    pub parameters: Option<Vec<String>>,
}

impl Layout<'_> {
    /// ⏴ and ⏵ are in egui's default fonts; ◀ and ▶ are not.
    pub(super) fn reach_marker(&self, extent: Extent) -> Option<Item> {
        let stops = extent.stops();
        let at = stops.iter().position(|&reach| reach == extent.reach()).filter(|_| stops.len() > 1)?;
        let fewer = if at > 0 { "⏴" } else { "" };
        let more = if at + 1 < stops.len() { "⏵" } else { "" };
        let text = format!("{fewer}|{more}");
        let width = self.measure.text_width(&text, Font::Faint);
        Some(Item::Reach { text, width })
    }

    /// Narrowest first. Fewer than two leave nothing to drag.
    pub fn stops(&self, block: &Block) -> Vec<(Option<Reach>, f32)> {
        let Some(extent) = self.extent(block) else { return Vec::new() };
        let mut at = block.clone();
        extent
            .stops()
            .into_iter()
            .map(|reach| {
                at.reach = reach;
                (reach, self.block(&at).size.x)
            })
            .collect()
    }

    fn parameters(&self, block: &Block) -> Option<&[String]> {
        let declaration = block.refers.as_ref()?;
        let declarer = self.declarers.get(&declaration.block)?;
        self.language.block(&declarer.opcode)?.signature_for(&declaration.slot.input)?;
        declarer.parameters.as_deref()
    }

    pub(super) fn extent(&self, block: &Block) -> Option<Extent> {
        let arity = self.parameters(block).map(<[String]>::len);
        self.language.block(&block.opcode)?.extent(block, arity)
    }

    /// Only one that is no reference can grow.
    pub(super) fn arguments(&self, block: &Block, list: &ListDef, extent: Extent) -> Vec<Item> {
        if extent.named {
            return Vec::new();
        }
        let stored = block.lists.get(&list.name).map(Vec::as_slice).unwrap_or(&[]);
        let hints = self.parameters(block).unwrap_or(&[]);
        let mut items: Vec<Item> = (0..extent.shown)
            .map(|index| {
                let hint = hints.get(index).unwrap_or(&list.hint);
                self.slot(block, (&list.name, &list.ty, hint), Some(index), stored.get(index))
            })
            .collect();
        if block.refers.is_none() && extent.shown == extent.parameters {
            items.push(self.append(list, stored.len()));
        }
        items
    }
}
