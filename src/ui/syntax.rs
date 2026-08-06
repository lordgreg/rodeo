//! Which palette colour each syntax scope gets.
//!
//! Pure data, kept out of `theme.rs` so the module there is the logic that
//! loads and validates a palette. A rule names its colour by pointing at the
//! getter, so a typo is a compile error rather than a silently uncoloured
//! scope.

use ratatui::style::Color;
use syntect::highlighting::FontStyle;

use crate::ui::theme::Colors;

/// How a syntax rule names its colour: the palette getter to call.
///
/// A `Role` enum plus a match mapping it back to a field used to sit here.
/// Pointing at the getter directly removes both, and the compiler checks the
/// reference.
pub type PaletteSlot = fn(&Colors) -> Color;

/// Scope → palette mapping for syntax highlighting.
///
/// One colour per *role* (keyword, type, function, string, number, …) so a
/// file reads consistently. syntect scores selectors by specificity, so a more
/// specific rule wins regardless of the order here.
///
/// Scope names were read out of the bundled Sublime grammars rather than
/// guessed — guessing is what left type names and macros uncoloured before
/// (Rust emits `entity.name.struct`, not `entity.name.type.struct`, and
/// `support.macro`, not `support.function.macro`).
///
/// Note on `storage.type`: these grammars use it both for declaration keywords
/// (Rust `let`/`fn`, Python `class`/`def`) and for primitive type names
/// (`u64`, `str`) — `let` and `u64` carry the *identical* scope. It is
/// therefore mapped to the keyword colour, since declaration keywords are far
/// more common; real type names arrive as `entity.name.*` / `support.type` and
/// get the type colour.
pub const SYNTAX_RULES: &[(&str, PaletteSlot, FontStyle)] = &[
    // Fallback for identifiers; anything unmatched uses settings.foreground.
    (
        "source, text, variable",
        Colors::foreground,
        FontStyle::empty(),
    ),
    // Comments.
    (
        "comment, punctuation.definition.comment",
        Colors::muted,
        FontStyle::ITALIC,
    ),
    // Punctuation and separators stay quiet, uniformly.
    (
        "punctuation, punctuation.separator, punctuation.terminator, \
         punctuation.accessor, punctuation.section, punctuation.definition, \
         meta.brace",
        Colors::muted,
        FontStyle::empty(),
    ),
    // Keywords: `use`, `pub`, `let`, `fn`, `struct`, `impl`, `class`, `def`.
    (
        "keyword, keyword.other, keyword.declaration, storage, \
         storage.modifier, storage.type",
        Colors::primary,
        FontStyle::empty(),
    ),
    (
        "keyword.control, keyword.control.flow, keyword.control.conditional, \
         keyword.control.import, keyword.control.exception",
        Colors::primary,
        FontStyle::BOLD,
    ),
    // Operators.
    (
        "keyword.operator, punctuation.definition.generic",
        Colors::secondary,
        FontStyle::empty(),
    ),
    // Type names — distinct from the keywords that introduce them.
    (
        "entity.name.type, entity.name.class, entity.name.struct, \
         entity.name.enum, entity.name.trait, entity.name.interface, \
         entity.name.impl, entity.name.union, entity.name.namespace, \
         support.type, support.class, entity.other.inherited-class",
        Colors::accent2,
        FontStyle::empty(),
    ),
    // Functions and macros: definitions and calls share a colour.
    (
        "entity.name.function, variable.function, support.function, \
         support.macro, entity.name.macro",
        Colors::info,
        FontStyle::empty(),
    ),
    // Strings.
    (
        "string, string.quoted, string.regexp, markup.raw, markup.inserted",
        Colors::success,
        FontStyle::empty(),
    ),
    // Interpolation inside strings must not look like string content.
    (
        "constant.character.escape, punctuation.definition.template-expression, \
         meta.interpolation, string.interpolated",
        Colors::warning,
        FontStyle::empty(),
    ),
    // Numbers, booleans, null, self/this.
    (
        "constant, constant.numeric, constant.language, constant.other, \
         variable.language, support.constant",
        Colors::accent1,
        FontStyle::empty(),
    ),
    // Parameters and attributes.
    (
        "variable.parameter, entity.other.attribute-name, \
         entity.name.label, meta.annotation, meta.attribute",
        Colors::accent3,
        FontStyle::empty(),
    ),
    // Preprocessor / attributes-as-metadata.
    (
        "meta.preprocessor, keyword.other.preprocessor",
        Colors::warning,
        FontStyle::empty(),
    ),
    // Markup tags (HTML/XML/JSX) — previously coloured as operators.
    (
        "entity.name.tag, punctuation.definition.tag",
        Colors::primary,
        FontStyle::empty(),
    ),
    // Anything the grammar flags as broken.
    (
        "invalid, invalid.illegal, markup.deleted",
        Colors::error,
        FontStyle::empty(),
    ),
    ("invalid.deprecated", Colors::warning, FontStyle::empty()),
    // Markdown and friends.
    (
        "markup.heading, entity.name.section",
        Colors::highlight,
        FontStyle::BOLD,
    ),
    (
        "markup.list, markup.quote",
        Colors::muted,
        FontStyle::empty(),
    ),
    (
        "markup.underline.link, markup.link",
        Colors::info,
        FontStyle::UNDERLINE,
    ),
    ("markup.italic", Colors::foreground, FontStyle::ITALIC),
    ("markup.bold", Colors::foreground, FontStyle::BOLD),
    (
        "markup.changed, meta.diff",
        Colors::warning,
        FontStyle::empty(),
    ),
];
